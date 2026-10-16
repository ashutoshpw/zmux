import Foundation

/// What a person answered when asked about a file tab's unsaved text before it closes.
enum FileCloseChoice: Sendable { case save, discard, cancel }

/// Decides whether a file panel may mirror its dirty bit into the Rust model. The decision is a
/// pure function of the panel's state so the macOS fixture can drive it directly
/// (`scripts/check-file-close-resolution`).
///
/// A panel whose Don't Save answer is being applied is held here. While it is held the model is
/// told the text is clean although the panel still has it, so a snapshot poll must not start a
/// new observation that would mark the model dirty again: the core close guard would then refuse
/// the close of a panel that already reads as clean, and a later close would lose the edits
/// without a prompt. Holds are counted so that two overlapping closes of one panel cannot
/// release each other's.
struct DirtyMirrorGate: Sendable, Equatable {
    private(set) var holds = 0

    var isDiscarding: Bool { holds > 0 }

    mutating func hold() { holds += 1 }

    mutating func release() {
        guard holds > 0 else {
            assertionFailure("DirtyMirrorGate released without a matching hold")
            return
        }
        holds -= 1
    }

    /// True when the model's dirty bit must be written to match the panel.
    func observationNeeded(attached: Bool, editable: Bool, closed: Bool, gaveUp: Bool, isDirty: Bool, modelDirty: Bool) -> Bool {
        attached && editable && !closed && !gaveUp && !isDiscarding && isDirty != modelDirty
    }

    /// Whether the close-time sync must send a write. A dirty-mirror loop that was running (or
    /// was cancelled by a hold) may have a write in flight that lands after the last snapshot, so
    /// the snapshot's value cannot be trusted to skip the write in that case.
    static func closeWriteNeeded(wanted: Bool, modelDirty: Bool, loopMayHaveWritten: Bool) -> Bool {
        loopMayHaveWritten || wanted != modelDirty
    }
}

/// The panel operations the close decision needs. `FilePanelView` is the production conformer;
/// the macOS fixture supplies scripted ones so the real decision below runs without AppKit.
@MainActor
protocol FileCloseParticipant: AnyObject {
    var hasUnsavedChanges: Bool { get }
    func performSave(force: Bool) async -> Bool
    /// Synchronously stops the dirty mirror from restarting; paired with release or apply.
    func holdDiscard()
    /// The close was aborted: nothing was discarded. Lifts the hold and resumes the mirror.
    func releaseDiscard()
    /// Makes the model's dirty bit agree with the panel (or with "clean" when Don't Save is held).
    func settleDirtyMirror(assumingDiscarded: Bool) async -> Bool
    /// Starts the mirror again after an aborted close left the model out of step.
    func resumeDirtyMirror()
    /// The close can no longer be aborted: drops the text's dirty state and lifts the hold.
    func applyDiscard()
}

/// Settles every file panel of an app-initiated close. Invariants:
/// - a Cancel or a failed Save aborts the close and nothing is discarded;
/// - Don't Save is applied only after every panel is settled, and every discarded panel is held
///   before the first awaited settle, so a snapshot poll in between cannot undo the settle;
/// - an aborted settle lifts every hold and puts the model right again.
@MainActor
enum FileCloseResolution {
    static func resolve<Panel: FileCloseParticipant>(
        _ panels: [Panel],
        choose: @MainActor (Panel) async -> FileCloseChoice
    ) async -> Bool {
        var discards: [Panel] = []
        for panel in panels where panel.hasUnsavedChanges {
            switch await choose(panel) {
            case .cancel: return false
            case .save: guard await panel.performSave(force: false) else { return false }
            case .discard: discards.append(panel)
            }
        }
        // No await between holding the first panel and the first settle: the poll cannot see a
        // half-held set.
        discards.forEach { $0.holdDiscard() }
        for panel in panels {
            let discarded = discards.contains { $0 === panel }
            guard await panel.settleDirtyMirror(assumingDiscarded: discarded) else {
                discards.forEach { $0.releaseDiscard() }
                panels.forEach { $0.resumeDirtyMirror() }
                return false
            }
        }
        discards.forEach { $0.applyDiscard() }
        return true
    }
}

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
    /// The close did not happen: nothing was discarded. Lifts the hold and resumes the mirror.
    func releaseDiscard()
    /// Makes the model's dirty bit agree with the panel (or with "clean" when Don't Save is held).
    func settleDirtyMirror(assumingDiscarded: Bool) async -> Bool
    /// Starts the mirror again after an aborted close left the model out of step.
    func resumeDirtyMirror()
    /// The tab is gone: drops the text's dirty state and lifts the hold.
    func applyDiscard()
}

/// The result of one command sent to the core, awaited by a caller that must not act before the
/// core has accepted or refused it. The first `finish` wins; later ones are ignored, so a command
/// that is both completed and abandoned (shutdown) resolves exactly once.
@MainActor
final class CommandOutcome {
    private var result: Bool?
    private var waiter: CheckedContinuation<Bool, Never>?

    /// An outcome that already failed: the command could not even be sent.
    static func failed() -> CommandOutcome {
        let outcome = CommandOutcome()
        outcome.finish(false)
        return outcome
    }

    var isFinished: Bool { result != nil }
    var hasWaiter: Bool { waiter != nil }

    func finish(_ succeeded: Bool) {
        guard result == nil else { return }
        result = succeeded
        waiter?.resume(returning: succeeded)
        waiter = nil
    }

    /// One waiter at a time: the close queue is the only caller.
    func wait() async -> Bool {
        if let result { return result }
        precondition(waiter == nil, "CommandOutcome supports a single waiter")
        return await withCheckedContinuation { waiter = $0 }
    }
}

/// Every panel of an app-initiated close has been asked and settled; the Don't Save answers are
/// still pending. The text is dropped only once the core has actually closed the tab:
/// `finish(closeSucceeded:)` applies the discards, or, when the core refused the close or never
/// received it, releases them so the panel keeps its edits, its edited marker and its guard in
/// the model. The first call decides; later ones do nothing.
@MainActor
final class FileCloseSettlement {
    private let discards: [any FileCloseParticipant]
    private(set) var isResolved = false

    init(discards: [any FileCloseParticipant]) { self.discards = discards }

    /// Panels whose Don't Save is waiting for the core.
    var pendingDiscardCount: Int { isResolved ? 0 : discards.count }

    /// Whether `panel`'s Don't Save answer is still waiting in this settlement.
    func isDiscarding(_ panel: AnyObject) -> Bool {
        !isResolved && discards.contains { $0 === panel }
    }

    func finish(closeSucceeded: Bool) {
        guard !isResolved else { return }
        isResolved = true
        if closeSucceeded { discards.forEach { $0.applyDiscard() } }
        else { discards.forEach { $0.releaseDiscard() } }
    }
}

/// Settles every file panel of an app-initiated close. Invariants:
/// - a Cancel or a failed Save aborts the close and nothing is discarded;
/// - every discarded panel is held before the first awaited settle, so a snapshot poll in between
///   cannot undo the settle;
/// - an aborted settle lifts every hold and puts the model right again;
/// - Don't Save is not applied here: the returned settlement applies it only after the core has
///   closed the tab, and reverts it when the close fails.
@MainActor
enum FileCloseResolution {
    /// Nil means the close must not proceed and nothing was discarded.
    static func resolve<Panel: FileCloseParticipant>(
        _ panels: [Panel],
        choose: @MainActor (Panel) async -> FileCloseChoice
    ) async -> FileCloseSettlement? {
        var discards: [Panel] = []
        for panel in panels where panel.hasUnsavedChanges {
            switch await choose(panel) {
            case .cancel: return nil
            case .save: guard await panel.performSave(force: false) else { return nil }
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
                return nil
            }
        }
        return FileCloseSettlement(discards: discards)
    }
}

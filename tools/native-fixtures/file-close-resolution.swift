import Foundation

/// Runs the production close decision (`FileCloseResolution`, `DirtyMirrorGate`) against scripted
/// panels. Everything is synchronous on the main actor: a "snapshot poll" is an explicit call made
/// at each suspension point of a settle, so there are no threads, timers or sleeps and every run
/// is the same run.
@MainActor
final class ScriptedPanel: FileCloseParticipant {
    let name: String
    /// The panel's own dirty state (`FilePanelView.isDirty`).
    var isDirty: Bool
    /// The Rust model's dirty bit for this file.
    var model: Bool
    /// What the panel last learned from a snapshot (`FilePanelView.lastModelDirty`).
    var lastModelDirty: Bool
    var gate = DirtyMirrorGate()
    var saveSucceeds = true
    var settleSucceeds = true
    /// A defective panel whose hold does nothing; proves the fixture can see the race.
    var ignoresHold = false
    /// Observations a poll started after the panel was asked to discard.
    var observationsStartedWhileDiscarding = 0
    var settleCalls = 0
    var applied = 0
    var released = 0
    var onSuspend: (@MainActor () -> Void)?

    init(_ name: String, dirty: Bool) {
        self.name = name
        isDirty = dirty
        model = dirty
        lastModelDirty = dirty
    }

    var hasUnsavedChanges: Bool { isDirty }

    /// `FilePanelView.update`: adopt the snapshot's bit, then start a mirror write when needed.
    func poll() {
        lastModelDirty = model
        if gate.observationNeeded(attached: true, editable: true, closed: false, gaveUp: false, isDirty: isDirty, modelDirty: lastModelDirty) {
            model = isDirty
            if gate.isDiscarding { observationsStartedWhileDiscarding += 1 }
        }
    }

    func performSave(force: Bool) async -> Bool {
        guard saveSucceeds else { return false }
        isDirty = false
        model = false
        lastModelDirty = false
        return true
    }

    func holdDiscard() { if !ignoresHold { gate.hold() } }

    func releaseDiscard() {
        released += 1
        if !ignoresHold { gate.release() }
        poll()
    }

    func settleDirtyMirror(assumingDiscarded: Bool) async -> Bool {
        settleCalls += 1
        onSuspend?()
        guard settleSucceeds else { return false }
        let wanted = assumingDiscarded ? false : isDirty
        if DirtyMirrorGate.closeWriteNeeded(wanted: wanted, modelDirty: lastModelDirty, loopMayHaveWritten: assumingDiscarded) {
            model = wanted
            lastModelDirty = wanted
        }
        onSuspend?()
        return true
    }

    func resumeDirtyMirror() { poll() }

    /// No poll here: a started observation's write lands later, after the close was allowed.
    func applyDiscard() {
        applied += 1
        isDirty = false
        if !ignoresHold { gate.release() }
    }
}

@main
@MainActor
struct FileCloseResolutionProbe {
    static var failures: [String] = []

    static func expect(_ condition: Bool, _ message: String) {
        if !condition { failures.append(message) }
    }

    /// A scenario cannot continue without its settlement; fail loudly instead of skipping the rest.
    static func abort(_ message: String) -> Never {
        FileHandle.standardError.write(Data(("file close resolution fixture failed: " + message + "\n").utf8))
        exit(1)
    }

    static func panels(_ names: [String], clean: Set<String> = []) -> [ScriptedPanel] {
        let made = names.map { ScriptedPanel($0, dirty: !clean.contains($0)) }
        for panel in made { panel.onSuspend = { made.forEach { $0.poll() } } }
        return made
    }

    /// Asks the questions and settles the panels; the core's answer is still pending.
    static func settle(_ panels: [ScriptedPanel], _ answers: [String: FileCloseChoice]) async -> FileCloseSettlement? {
        await FileCloseResolution.resolve(panels) { panel in answers[panel.name] ?? .cancel }
    }

    /// The whole close: settle, then the core answers `coreCloses`. False when the close was aborted
    /// before it reached the core.
    static func resolve(_ panels: [ScriptedPanel], _ answers: [String: FileCloseChoice], coreCloses: Bool = true) async -> Bool {
        guard let settlement = await settle(panels, answers) else { return false }
        settlement.finish(closeSucceeded: coreCloses)
        return true
    }

    static func gateCases() {
        var gate = DirtyMirrorGate()
        func needed(_ g: DirtyMirrorGate, attached: Bool = true, editable: Bool = true, closed: Bool = false,
                    gaveUp: Bool = false, dirty: Bool = true, model: Bool = false) -> Bool {
            g.observationNeeded(attached: attached, editable: editable, closed: closed, gaveUp: gaveUp, isDirty: dirty, modelDirty: model)
        }
        expect(needed(gate), "a dirty panel the model does not know about must be observed")
        expect(!needed(gate, model: true), "agreeing bits need no observation")
        expect(!needed(gate, attached: false), "no owner, no observation")
        expect(!needed(gate, editable: false), "a preview is never observed")
        expect(!needed(gate, closed: true), "a closed panel is never observed")
        expect(!needed(gate, gaveUp: true), "a refused owner stops observation")
        gate.hold()
        expect(gate.isDiscarding && !needed(gate), "a held panel must not be observed")
        expect(!needed(gate, dirty: false, model: true), "a held panel must not be observed even to mark it clean")
        gate.hold()
        gate.release()
        expect(gate.isDiscarding && !needed(gate), "holds are counted: one release must not lift two holds")
        gate.release()
        expect(!gate.isDiscarding && needed(gate), "observation resumes once every hold is released")
        expect(DirtyMirrorGate.closeWriteNeeded(wanted: false, modelDirty: true, loopMayHaveWritten: false), "a differing bit needs a write")
        expect(!DirtyMirrorGate.closeWriteNeeded(wanted: true, modelDirty: true, loopMayHaveWritten: false), "agreeing bits with no loop need no write")
        expect(DirtyMirrorGate.closeWriteNeeded(wanted: true, modelDirty: true, loopMayHaveWritten: true), "a possibly landed loop write forces a write")
    }

    static func main() async {
        gateCases()

        // Two Don't Saves: a poll lands before and after every settle write.
        do {
            let p = panels(["a", "b"])
            let ok = await resolve(p, ["a": .discard, "b": .discard])
            expect(ok, "two Don't Saves must close")
            for panel in p {
                expect(!panel.isDirty && !panel.model, "\(panel.name): a discarded panel and its model must both be clean")
                expect(panel.observationsStartedWhileDiscarding == 0, "\(panel.name): a poll restarted the mirror while the discard was held")
                expect(!panel.gate.isDiscarding, "\(panel.name): the hold must be lifted once the discard is applied")
                panel.poll()
                expect(!panel.model, "\(panel.name): a poll after the close must not mark the model dirty again")
            }
        }

        // The same close with a panel that does not hold: the model is re-marked dirty, which is the
        // defect. This proves the scenario above can fail.
        do {
            let p = panels(["a", "b"])
            p[0].ignoresHold = true
            p[1].ignoresHold = true
            let ok = await resolve(p, ["a": .discard, "b": .discard])
            expect(ok, "control: the close itself still reports success")
            expect(p[0].model && !p[0].isDirty, "control: without a hold the model is dirty for a panel that reads clean, so the core would refuse the close")
        }

        // Cancel after Don't Save: nothing was settled, nothing is discarded.
        do {
            let p = panels(["a", "b"])
            let ok = await resolve(p, ["a": .discard, "b": .cancel])
            expect(!ok, "Cancel must abort the close")
            expect(p.allSatisfy { $0.isDirty && $0.model && !$0.gate.isDiscarding && $0.settleCalls == 0 }, "Cancel after Don't Save must leave every panel exactly as it was")
        }

        // A failed Save aborts the close and discards nothing.
        do {
            let p = panels(["a", "b"])
            p[1].saveSucceeds = false
            let ok = await resolve(p, ["a": .discard, "b": .save])
            expect(!ok, "a failed Save must abort the close")
            expect(p[0].isDirty && p[0].model && p[1].isDirty && p[1].model, "a failed Save must leave every panel dirty and guarded")
            expect(p.allSatisfy { $0.settleCalls == 0 && !$0.gate.isDiscarding }, "a failed Save must abort before anything is settled or held")
        }

        // A successful Save followed by a Don't Save closes both.
        do {
            let p = panels(["a", "b"])
            let ok = await resolve(p, ["a": .save, "b": .discard])
            expect(ok, "Save then Don't Save must close")
            expect(!p[0].model && !p[1].model && !p[0].isDirty && !p[1].isDirty, "Save then Don't Save must leave both clean")
        }

        // A settle that fails after the first discarded panel was already told it is clean: the
        // close is aborted, the holds are lifted and the model is put right.
        do {
            let p = panels(["a", "b"], clean: ["b"])
            p[1].settleSucceeds = false
            let ok = await resolve(p, ["a": .discard])
            expect(!ok, "a failed settle must abort the close")
            expect(p[0].isDirty && p[0].model, "a failed settle must restore the first panel's dirty bit in the model")
            expect(p.allSatisfy { !$0.gate.isDiscarding }, "a failed settle must lift every hold")
        }

        // A clean panel among dirty ones is settled but never held.
        do {
            let p = panels(["a", "c"], clean: ["c"])
            let ok = await resolve(p, ["a": .discard])
            expect(ok && !p[0].model && !p[1].gate.isDiscarding && p[1].settleCalls == 1, "a clean panel is settled without a hold")
        }

        // Don't Save is pending until the core answers: the panel keeps its text, its edited marker
        // and a clean model (so the core accepts the close), and no poll marks the model dirty.
        do {
            let p = panels(["a", "b"])
            guard let settlement = await settle(p, ["a": .discard, "b": .discard]) else {
                abort("two Don't Saves must settle")
            }
            for panel in p {
                expect(panel.isDirty && panel.hasUnsavedChanges, "\(panel.name): the text must stay on the panel until the core has closed the tab")
                expect(!panel.model, "\(panel.name): the model reads clean so the core accepts the close")
                expect(panel.gate.isDiscarding && panel.applied == 0 && panel.released == 0, "\(panel.name): the discard is held, neither applied nor released")
                panel.poll()
                expect(!panel.model && panel.observationsStartedWhileDiscarding == 0, "\(panel.name): a poll while the close is in flight must not re-mark the model")
            }
            expect(settlement.pendingDiscardCount == 2 && !settlement.isResolved, "both discards wait for the core")
            settlement.finish(closeSucceeded: true)
            expect(p.allSatisfy { !$0.isDirty && $0.applied == 1 && $0.released == 0 && !$0.gate.isDiscarding }, "the core closing the tab applies every discard once")
            expect(settlement.isResolved && settlement.pendingDiscardCount == 0, "a finished settlement has nothing pending")
        }

        // The core refused the close (or it never reached the core): the discard is given back.
        // This is the data-loss path: the tab stays open and must keep guarding its text.
        do {
            let p = panels(["a", "b"])
            let ok = await resolve(p, ["a": .discard, "b": .discard], coreCloses: false)
            expect(ok, "the close reached the core")
            for panel in p {
                expect(panel.isDirty && panel.hasUnsavedChanges, "\(panel.name): a refused close must leave the edited text and its marker")
                expect(panel.model, "\(panel.name): a refused close must mark the model dirty again so a later close still asks")
                expect(panel.applied == 0 && panel.released == 1 && !panel.gate.isDiscarding, "\(panel.name): the discard is released once, never applied")
            }
            // The next close asks again, because the panel still has unsaved changes.
            var asked = 0
            let again = await FileCloseResolution.resolve(p) { _ in asked += 1; return .cancel }
            expect(again == nil && asked == 1, "a later close of a panel whose first close was refused must prompt again")
        }

        // Only the first resolution counts: a retire path or a late completion cannot undo it, and
        // a second finish must not lift a hold it no longer owns.
        do {
            let p = panels(["a"])
            guard let settlement = await settle(p, ["a": .discard]) else { abort("a Don't Save settle was refused") }
            settlement.finish(closeSucceeded: true)
            settlement.finish(closeSucceeded: false)
            settlement.finish(closeSucceeded: true)
            expect(p[0].applied == 1 && p[0].released == 0 && !p[0].isDirty && !p[0].model, "a settlement resolves exactly once; a late refusal must not revive the text")
            let q = panels(["b"])
            guard let refused = await settle(q, ["b": .discard]) else { abort("a Don't Save settle was refused") }
            refused.finish(closeSucceeded: false)
            refused.finish(closeSucceeded: true)
            expect(q[0].applied == 0 && q[0].released == 1 && q[0].isDirty && q[0].model, "a late success must not drop text after a refusal")
        }

        // Two closes of one panel in a row: the first is refused, the second succeeds. Holds are
        // counted, so the first outcome cannot release the second's hold.
        do {
            let p = panels(["a"])
            guard let first = await settle(p, ["a": .discard]) else { abort("a Don't Save settle was refused") }
            guard let second = await settle(p, ["a": .discard]) else { abort("a Don't Save settle was refused") }
            expect(p[0].gate.holds == 2, "overlapping closes of one panel hold it twice")
            first.finish(closeSucceeded: false)
            expect(p[0].gate.isDiscarding && p[0].isDirty, "the first refusal must not lift the second close's hold")
            p[0].poll()
            expect(p[0].observationsStartedWhileDiscarding == 0, "no observation starts while the second close is in flight")
            second.finish(closeSucceeded: true)
            expect(!p[0].gate.isDiscarding && !p[0].isDirty, "the second close applies its discard and lifts the last hold")
        }

        // A panel that is saved is never part of the pending set, and a save-only close needs no
        // core answer to be correct.
        do {
            let p = panels(["a", "b"])
            guard let settlement = await settle(p, ["a": .save, "b": .save]) else { abort("a Don't Save settle was refused") }
            expect(settlement.pendingDiscardCount == 0, "Save answers leave nothing pending")
            settlement.finish(closeSucceeded: false)
            expect(p.allSatisfy { !$0.isDirty && !$0.model && $0.applied == 0 && $0.released == 0 }, "a refused close after Save changes nothing: the file is already on disk")
        }

        // The command outcome the close waits for.
        do {
            let done = CommandOutcome()
            done.finish(true)
            done.finish(false)
            expect(done.isFinished, "a finished outcome reports it")
            let first = await done.wait()
            expect(first, "the first finish wins and wait returns it without suspending")
            let failed = CommandOutcome.failed()
            let failedResult = await failed.wait()
            expect(!failedResult, "a command that could not be sent is a failed outcome")

            let pending = CommandOutcome()
            let waiter = Task { @MainActor in await pending.wait() }
            // Cooperative scheduling on the main actor: yielding lets the waiter run until it
            // suspends. This ends as soon as the waiter is registered; it never sleeps.
            while !pending.hasWaiter { await Task.yield() }
            pending.finish(false)
            let late = await waiter.value
            expect(!late, "a waiter suspended before the answer receives it")
        }

        guard failures.isEmpty else {
            for failure in failures { FileHandle.standardError.write(Data(("file close resolution fixture failed: " + failure + "\n").utf8)) }
            exit(1)
        }
        print("File close resolution passed: Don't Save held across polls, Cancel and failed Save abort with nothing discarded, failed settle restores the model, holds counted, Don't Save applied only when the core closes the tab and released when it refuses.")
    }
}

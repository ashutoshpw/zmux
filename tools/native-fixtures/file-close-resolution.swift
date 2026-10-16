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

    static func panels(_ names: [String], clean: Set<String> = []) -> [ScriptedPanel] {
        let made = names.map { ScriptedPanel($0, dirty: !clean.contains($0)) }
        for panel in made { panel.onSuspend = { made.forEach { $0.poll() } } }
        return made
    }

    static func resolve(_ panels: [ScriptedPanel], _ answers: [String: FileCloseChoice]) async -> Bool {
        await FileCloseResolution.resolve(panels) { panel in answers[panel.name] ?? .cancel }
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

        guard failures.isEmpty else {
            for failure in failures { FileHandle.standardError.write(Data(("file close resolution fixture failed: " + failure + "\n").utf8)) }
            exit(1)
        }
        print("File close resolution passed: Don't Save held across polls, Cancel and failed Save abort with nothing discarded, failed settle restores the model, holds counted.")
    }
}

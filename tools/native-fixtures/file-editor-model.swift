import Foundation

/// Runs the production `FileEditorModel` (the type `FilePanelView` asks for every dirty, save,
/// reload and banner decision) through the situations that lose text when they go wrong. Everything
/// is synchronous: no threads, timers, files or sleeps, so every run is the same run.
@main
@MainActor
struct FileEditorModelProbe {
    static var failures: [String] = []

    static func expect(_ condition: Bool, _ message: String) {
        if !condition { failures.append(message) }
    }

    /// A model that has loaded `text` and is attached at `generation`.
    static func loaded(_ text: String, generation: UInt64 = 1) -> FileEditorModel {
        var model = FileEditorModel()
        model.adopt(text: text)
        model.attached(generation: generation)
        return model
    }

    static func dirtyTracking() {
        // Swift `String ==` calls canonically equivalent text equal. The model must not.
        let precomposed = "caf\u{e9}\n"
        let decomposed = "cafe\u{301}\n"
        expect(precomposed == decomposed, "control: Swift String equality treats the two forms as equal, so the byte comparison below is what detects the edit")
        expect(Array(precomposed.utf8) != Array(decomposed.utf8), "control: the two forms differ in bytes")
        var model = loaded(precomposed)
        expect(!model.isDirty, "a freshly loaded buffer is clean")
        expect(model.textEdited(decomposed) && model.isDirty, "an edit that only changes the normalization form changes the bytes Save writes, so it is dirty")
        expect(!model.textEdited(decomposed), "the same text again is not a change")
        expect(model.textEdited(precomposed) && !model.isDirty, "undoing back to the loaded bytes is clean again")

        model = loaded("one\n")
        expect(model.textEdited("one\ntwo\n") && model.isDirty, "typing makes the buffer dirty")
        expect(model.adopt(text: "one\ntwo\n") && !model.isDirty, "abandoning the text (Don't Save) makes the buffer clean")
        expect(model.textEdited("one\n") && model.isDirty, "after adopting, the adopted text is the baseline")

        expect(model.hasUnsavedChanges(editable: true), "a dirty editable buffer has unsaved changes")
        expect(!model.hasUnsavedChanges(editable: false), "a preview never has unsaved changes")
        expect(!loaded("x").hasUnsavedChanges(editable: true), "a clean buffer has none")
    }

    static func saving() {
        // The payload is the whole buffer as UTF-8: no byte dropped, added or re-normalized.
        let model = loaded("a")
        for text in ["", "x", "héllo\n日本\u{1F600}\n", "no trailing newline", "cafe\u{301}\n", "line\r\nline\r\n"] {
            expect(Array(model.savePayload(for: text)) == Array(text.utf8), "the save payload for \(text.debugDescription) must be exactly its UTF-8 bytes")
        }
        expect(model.savePayload(for: "abc").count == 3, "a save writes every byte of the buffer")

        // canSave: every condition is needed.
        var dirty = loaded("a")
        dirty.textEdited("ab")
        func can(_ m: FileEditorModel, editable: Bool = true, writable: Bool = true, attached: Bool = true, busy: Bool = false, stale: Bool = false) -> Bool {
            m.canSave(editable: editable, writable: writable, attached: attached, busy: busy, staleOwner: stale)
        }
        expect(can(dirty), "a dirty, attached, writable editable buffer can be saved")
        expect(!can(dirty, editable: false), "a preview cannot be saved")
        expect(!can(dirty, writable: false), "a read-only file cannot be saved")
        expect(!can(dirty, attached: false), "a detached view cannot be saved")
        expect(!can(dirty, busy: true), "a view with a request running cannot save")
        expect(!can(dirty, stale: true), "a stale owner cannot save")
        expect(!can(loaded("a")), "a clean buffer has nothing to save")

        // A confirmed save: baseline, generation and dirtiness follow what was written.
        var confirmed = loaded("a", generation: 3)
        confirmed.textEdited("ab")
        expect(confirmed.generation == 3, "the base generation is the one last loaded")
        expect(confirmed.saved(written: "ab", current: "ab", generation: 4) && !confirmed.isDirty, "a confirmed save of the current buffer makes it clean")
        expect(confirmed.generation == 4, "the next save is based on the generation the service returned")
        expect(confirmed.textEdited("abc") && confirmed.isDirty, "the written text is the new baseline")

        // Typing while the save is in flight stays unsaved.
        var racing = loaded("a")
        racing.textEdited("ab")
        expect(!racing.saved(written: "ab", current: "abc", generation: 2) && racing.isDirty, "text typed while the save was in flight is still unsaved")
        expect(racing.textEdited("ab") && !racing.isDirty, "and returns to clean only when it matches what was written")

        // A reply without a generation keeps the old base instead of resetting it.
        var keep = loaded("a", generation: 9)
        keep.textEdited("b")
        keep.saved(written: "b", current: "b", generation: nil)
        expect(keep.generation == 9, "a reply with no generation must not reset the base generation")

        // A save never silently overwrites a disk that moved on under kept edits.
        var pending = loaded("a")
        pending.textEdited("ab")
        _ = pending.externalStateReported(state: "changed", diskGeneration: 5, errorCode: nil, attached: true, busy: false, hasUnsavedChanges: true)
        expect(!pending.saveNeedsConflictPrompt(force: false), "a bare changed-on-disk report alone does not by itself defer saves (the service reports a conflict)")
        _ = pending.reloadReplied(automatic: true, hasUnsavedChanges: true, generation: 6)
        expect(pending.saveNeedsConflictPrompt(force: false), "kept edits over a newer disk version ask before saving")
        expect(!pending.saveNeedsConflictPrompt(force: true), "an explicit overwrite skips the prompt")
        pending.saved(written: "ab", current: "ab", generation: 7)
        expect(!pending.saveNeedsConflictPrompt(force: false), "a confirmed save ends the kept-edits state")
    }

    /// A save whose rename landed but whose directory sync failed (`sync_failed`, outcome
    /// unknown, committed generation reported). The text is on disk and the service moved its
    /// generation on, so the view must rebase on that generation and a repeat save must be
    /// possible and clean up after itself. Before the fix the view kept the old generation, so the
    /// repeat save was refused as stale and the banner never cleared.
    static func unsyncedSave() {
        func can(_ m: FileEditorModel) -> Bool { m.canSave(editable: true, writable: true, attached: true, busy: false, staleOwner: false) }
        var model = loaded("before\n", generation: 3)
        model.textEdited("after\n")
        expect(can(model), "control: the dirty buffer can be saved")
        expect(model.savedWithoutDurability(written: "after\n", current: "after\n", generation: 4) && !model.isDirty,
               "the text reached the disk, so the buffer is clean")
        expect(model.generation == 4, "the next save is based on the generation the failure reported, not the stale one")
        expect(model.durabilityUnconfirmed, "one more save is still owed")
        expect(can(model), "a clean buffer can be saved while the directory sync is owed, so Save Again works")
        expect(!model.saveNeedsConflictPrompt(force: false), "the rebased buffer does not ask about a conflict")
        expect(model.textEdited("after\nmore") && model.isDirty && model.durabilityUnconfirmed, "typing afterwards stays dirty and the debt remains")
        expect(model.textEdited("after\n") && !model.isDirty, "returning to the saved text is clean again")

        // Typing while the failed save was in flight stays dirty and is not lost.
        var racing = loaded("a", generation: 1)
        racing.textEdited("ab")
        expect(!racing.savedWithoutDurability(written: "ab", current: "abc", generation: 2) && racing.isDirty, "text typed during the failed save is still unsaved")

        // The repeat save, once confirmed, ends the debt and the notice.
        model.saved(written: "after\n", current: "after\n", generation: 5)
        expect(!model.durabilityUnconfirmed && model.generation == 5, "a confirmed repeat save clears the debt and moves the base on")
        expect(!can(model), "and a clean buffer with nothing owed cannot be saved again")

        // A reload settles it too: the buffer is replaced by what is on disk.
        var reloaded = loaded("x", generation: 1)
        reloaded.textEdited("y")
        reloaded.savedWithoutDurability(written: "y", current: "y", generation: 2)
        reloaded.attached(generation: 3)
        expect(!reloaded.durabilityUnconfirmed && !can(reloaded), "a reload ends the debt")

        // The notice: its buttons repeat the save or dismiss, and a save or load clears it.
        func press(_ primary: Bool) -> FileBannerAction {
            var m = loaded("a")
            m.setBanner(.savedUnsynced("sync_failed"))
            return m.bannerAction(primary: primary, hasUnsavedChanges: false)
        }
        expect(press(true) == .saveAgain, "the primary button of the unsynced notice repeats the save")
        expect(press(false) == .dismiss, "the secondary button dismisses it")
        expect(FileBanner.savedUnsynced("x").isObsoleteAfterSave, "a confirmed save clears the unsynced notice")
        expect(FileBanner.savedUnsynced("x").isObsoleteAfterLoad, "a load clears the unsynced notice")
        expect(!FileBanner.savedUnsynced("x").isExternal, "it is not about the disk's state")
        var sync = loaded("a")
        sync.setBanner(.savedUnsynced("sync_failed"))
        expect(sync.externalStateReported(state: "in_sync", diskGeneration: 4, errorCode: nil, attached: true, busy: false, hasUnsavedChanges: false) == .none,
               "back in sync does not hide the notice: the text is visible but not yet durable")
    }

    static func reloading() {
        expect(FileEditorModel.reloadNeedsConfirmation(hasUnsavedChanges: true), "a manual reload with unsaved text must ask")
        expect(!FileEditorModel.reloadNeedsConfirmation(hasUnsavedChanges: false), "a manual reload of a clean buffer need not ask")

        // Typing is blocked exactly while a reply can replace the buffer.
        var model = loaded("a")
        expect(model.isEditable(loaded: true, writable: true), "a loaded writable buffer is editable")
        expect(!model.isEditable(loaded: false, writable: true), "an unloaded buffer is not editable")
        expect(!model.isEditable(loaded: true, writable: false), "a read-only file is not editable")
        model.suspendEditing()
        expect(!model.isEditable(loaded: true, writable: true) && model.editingSuspended, "the buffer is read-only while a reload or attach is in flight")
        model.resumeEditing()
        expect(model.isEditable(loaded: true, writable: true), "and editable again after the reply was handled")

        // An automatic reload that finds typed text keeps it and remembers the disk moved on.
        var kept = loaded("a", generation: 2)
        _ = kept.externalStateReported(state: "changed", diskGeneration: 8, errorCode: nil, attached: true, busy: false, hasUnsavedChanges: false)
        kept.automaticReloadStarted()
        kept.textEdited("typed")
        let keptResult = kept.reloadReplied(automatic: true, hasUnsavedChanges: true, generation: 3)
        expect(keptResult == .keepText(.changedOnDisk(8)), "an automatic reload must not replace text typed after it was scheduled: \(keptResult)")
        expect(kept.generation == 3 && kept.isDirty, "the kept buffer is rebased on the new generation and stays dirty")

        // Everything else replaces the buffer.
        var clean = loaded("a")
        expect(clean.reloadReplied(automatic: true, hasUnsavedChanges: false, generation: 2) == .replaceBuffer, "an automatic reload of a clean buffer replaces it")
        var manual = loaded("a")
        manual.textEdited("b")
        expect(manual.reloadReplied(automatic: false, hasUnsavedChanges: true, generation: 2) == .replaceBuffer, "a manual reload was confirmed before it was sent, so its reply replaces the buffer")
        expect(manual.generation == 1, "a replacing reload leaves the generation to `attached(generation:)`")
        manual.attached(generation: 2)
        expect(manual.generation == 2 && !manual.saveNeedsConflictPrompt(force: false), "applying the reply rebases the generation and ends kept-edits state")
        manual.adopt(text: "disk")
        expect(!manual.isDirty, "adopting the reloaded text is clean")
    }

    static func externalChanges() {
        func report(_ m: inout FileEditorModel, _ state: String, _ generation: UInt64, unsaved: Bool, attached: Bool = true, busy: Bool = false, code: String? = nil) -> FileExternalReaction {
            m.externalStateReported(state: state, diskGeneration: generation, errorCode: code, attached: attached, busy: busy, hasUnsavedChanges: unsaved)
        }
        var model = loaded("a")
        expect(report(&model, "changed", 4, unsaved: false) == .reloadAutomatically, "a clean buffer reloads a changed file without asking")
        model.automaticReloadStarted()
        expect(report(&model, "changed", 4, unsaved: false) == .none, "one automatic attempt per disk generation")
        expect(report(&model, "changed", 5, unsaved: false) == .reloadAutomatically, "a newer disk generation gets a new attempt")
        expect(report(&model, "changed", 5, unsaved: true) == .show(.changedOnDisk(5)), "a dirty buffer is never reloaded automatically: it shows a banner")
        expect(report(&model, "deleted", 5, unsaved: true) == .show(.deletedOnDisk), "a deleted file shows a banner")
        expect(report(&model, "unreadable", 5, unsaved: true, code: "permission_denied") == .show(.unreadable("permission_denied")), "an unreadable file names the code")
        expect(report(&model, "unreadable", 5, unsaved: true) == .show(.unreadable("unknown")), "an unreadable file with no code says unknown")
        expect(report(&model, "unattached", 5, unsaved: true) == .none, "an unknown state does nothing")
        expect(report(&model, "changed", 9, unsaved: false, attached: false) == .none, "a detached view starts nothing")
        expect(report(&model, "changed", 9, unsaved: false, busy: true) == .none, "a busy view starts nothing")
        expect(model.latestDiskGeneration == 9, "the latest disk generation is recorded even when nothing starts")

        // Dismissals are remembered so the same disk state is not announced again.
        var dismiss = loaded("a")
        dismiss.textEdited("b")
        expect(report(&dismiss, "changed", 5, unsaved: true) == .show(.changedOnDisk(5)), "announced once")
        dismiss.setBanner(.changedOnDisk(5))
        expect(dismiss.bannerAction(primary: false, hasUnsavedChanges: true) == .dismiss, "Keep Mine dismisses")
        expect(report(&dismiss, "changed", 5, unsaved: true) == .none, "a dismissed disk generation is not announced again")
        expect(report(&dismiss, "changed", 6, unsaved: true) == .show(.changedOnDisk(6)), "a newer disk generation is announced")
        dismiss.setBanner(.deletedOnDisk)
        expect(dismiss.bannerAction(primary: true, hasUnsavedChanges: true) == .dismiss, "Dismiss on the deleted banner dismisses")
        expect(report(&dismiss, "deleted", 6, unsaved: true) == .none, "a dismissed deletion is not announced again")
        _ = report(&dismiss, "in_sync", 6, unsaved: true)
        expect(report(&dismiss, "deleted", 7, unsaved: true) == .show(.deletedOnDisk), "back in sync re-arms the deleted banner")

        // The banner clears when the file is back in sync, unless edits were kept over a newer disk.
        var sync = loaded("a")
        sync.setBanner(.changedOnDisk(5))
        expect(report(&sync, "in_sync", 5, unsaved: false) == .clearBanner, "back in sync clears an external banner")
        sync.setBanner(.message("Could not save"))
        expect(report(&sync, "in_sync", 5, unsaved: false) == .none, "back in sync keeps a banner that is not about the disk")
        sync.setBanner(.changedOnDisk(5))
        sync.textEdited("kept")
        _ = sync.reloadReplied(automatic: true, hasUnsavedChanges: true, generation: 6)
        expect(report(&sync, "in_sync", 5, unsaved: true) == .none, "kept edits over a newer disk keep their banner")

        // An error or recovery banner is never replaced by the disk-state banner on the next
        // snapshot, even though the disk stays changed, deleted or unreadable. Only an empty
        // banner, another disk banner or an informational notice yields.
        let kept: [FileBanner] = [
            .message("Could not save: permission denied (permission_denied)"),
            .saveUnknown(7), .saveUnknown(0),
            .ownerLost("This view is no longer attached to the file service (stale_view)."),
            .loadFailed("Reload failed: busy"),
            .savedUnsynced("sync_failed"),
        ]
        for banner in kept {
            expect(!banner.yieldsToExternal, "\(banner) is an error or recovery banner and does not yield to a disk banner")
            for (state, generation) in [("changed", UInt64(9)), ("deleted", 9), ("unreadable", 9)] {
                var ranked = loaded("a")
                ranked.textEdited("b")
                ranked.setBanner(banner)
                expect(report(&ranked, state, generation, unsaved: true, code: "io") == .none, "a \(state) report does not replace \(banner)")
                expect(ranked.banner == banner, "\(banner) is still showing after a \(state) report")
                // Repeated snapshots keep it, and dismissing it lets the disk state through.
                expect(report(&ranked, state, generation, unsaved: true, code: "io") == .none, "a second \(state) report still keeps \(banner)")
                ranked.setBanner(.none)
                expect(report(&ranked, state, generation, unsaved: true, code: "io") != .none, "once \(banner) is gone the \(state) state is announced")
            }
        }
        for banner in [FileBanner.none, .changedOnDisk(3), .deletedOnDisk, .unreadable("io"), .dirtyUnconfirmed, .saveResolved] {
            expect(banner.yieldsToExternal, "\(banner) yields to a disk banner")
        }
        var pendingSave = loaded("a")
        pendingSave.textEdited("b")
        pendingSave.setBanner(.saveUnknown(7))
        _ = report(&pendingSave, "changed", 9, unsaved: true)
        expect(pendingSave.saveOutcomeBanner(lastOperation: 7, state: "completed", code: nil) == .saveResolved,
               "a pending-save notice survives disk reports so its outcome can still resolve it")

        // A lost save acknowledgement resolves through a later snapshot.
        var unknown = loaded("a")
        unknown.setBanner(.saveUnknown(7))
        expect(unknown.saveOutcomeBanner(lastOperation: 7, state: "completed", code: nil) == .saveResolved, "a completed save resolves the notice")
        expect(unknown.saveOutcomeBanner(lastOperation: 7, state: "failed", code: "io") == .message("The save that did not confirm in time failed (io). Your text is unchanged."), "a failed save says so and that the text is unchanged")
        expect(unknown.saveOutcomeBanner(lastOperation: 7, state: "pending", code: nil) == nil, "a pending save changes nothing")
        expect(unknown.saveOutcomeBanner(lastOperation: 8, state: "completed", code: nil) == nil, "another operation's outcome is not this save's")
        unknown.setBanner(.saveUnknown(0))
        expect(unknown.saveOutcomeBanner(lastOperation: 0, state: "completed", code: nil) == nil, "operation 0 means the id is unknown and cannot be matched")
        unknown.setBanner(.none)
        expect(unknown.saveOutcomeBanner(lastOperation: 7, state: "completed", code: nil) == nil, "with no pending-save notice nothing is resolved")

        // The worker's own final word on a save the wait gave up on (a directory sync that failed
        // after the timeout) never turns into completed or failed. Waiting for it would be
        // permanent, so the notice becomes the one with the Reload and Dismiss buttons.
        var finalWord = loaded("a")
        finalWord.setBanner(.saveUnknown(7))
        expect(finalWord.saveOutcomeBanner(lastOperation: 7, state: "outcome_unknown", code: "sync_failed") == .saveUnknown(0), "a final unknown outcome stops waiting and offers Reload")
        expect(finalWord.saveOutcomeBanner(lastOperation: 7, state: "outcome_unknown", code: "timeout") == nil, "an unknown outcome that is a timeout is still being waited for: the worker will resolve it")
        expect(finalWord.saveOutcomeBanner(lastOperation: 8, state: "outcome_unknown", code: "sync_failed") == nil, "another operation's outcome is not this save's")
        finalWord.setBanner(.saveUnknown(0))
        expect(finalWord.saveOutcomeBanner(lastOperation: 7, state: "outcome_unknown", code: "sync_failed") == nil, "once resolved to the unmatched notice it is not resolved again")
        let resolvedPrimary = finalWord.bannerAction(primary: true, hasUnsavedChanges: true)
        let resolvedSecondary = finalWord.bannerAction(primary: false, hasUnsavedChanges: true)
        expect(resolvedPrimary == .reloadFromDisk && resolvedSecondary == .dismiss, "the resolved notice has working buttons")
    }

    static func banners() {
        var model = loaded("a")
        expect(model.setBanner(.deletedOnDisk) && !model.setBanner(.deletedOnDisk), "setting the same banner is not a change")
        expect(model.banner == .deletedOnDisk, "the banner is stored")

        for banner in [FileBanner.changedOnDisk(1), .deletedOnDisk, .unreadable("x")] {
            expect(banner.isExternal, "\(banner) reports the disk state")
        }
        for banner in [FileBanner.none, .ownerLost("x"), .saveUnknown(1), .saveResolved, .loadFailed("x"), .message("x"), .dirtyUnconfirmed] {
            expect(!banner.isExternal, "\(banner) is not about the disk")
        }
        let afterLoad: [FileBanner] = [.loadFailed("x"), .ownerLost("x"), .saveResolved, .changedOnDisk(1), .deletedOnDisk, .unreadable("x")]
        let keptAfterLoad: [FileBanner] = [.none, .saveUnknown(3), .message("x"), .dirtyUnconfirmed]
        expect(afterLoad.allSatisfy { $0.isObsoleteAfterLoad }, "a load obsoletes failure, lost-owner, resolved and disk notices")
        expect(keptAfterLoad.allSatisfy { !$0.isObsoleteAfterLoad }, "a load keeps an unresolved save notice, messages and the dirty notice")
        expect(FileBanner.saveResolved.isObsoleteAfterSave && FileBanner.changedOnDisk(1).isObsoleteAfterSave, "a confirmed save obsoletes the resolved and disk notices")
        expect(!FileBanner.ownerLost("x").isObsoleteAfterSave && !FileBanner.loadFailed("x").isObsoleteAfterSave && !FileBanner.message("x").isObsoleteAfterSave,
               "a save does not hide a lost-owner, load-failed or message notice")

        // Buttons: replacing the buffer is only ever requested through reloadFromDisk (which asks).
        func press(_ banner: FileBanner, primary: Bool, unsaved: Bool = true) -> FileBannerAction {
            var m = loaded("a")
            m.setBanner(banner)
            return m.bannerAction(primary: primary, hasUnsavedChanges: unsaved)
        }
        for banner in [FileBanner.changedOnDisk(2), .unreadable("x"), .saveResolved, .ownerLost("x"), .saveUnknown(0)] {
            expect(press(banner, primary: true) == .reloadFromDisk, "the primary button of \(banner) reloads through reloadFromDisk")
        }
        expect(press(.changedOnDisk(2), primary: false) == .dismiss, "Keep Mine dismisses")
        expect(press(.ownerLost("x"), primary: false) == .copyText, "the lost-owner banner offers Copy Text")
        expect(press(.deletedOnDisk, primary: true) == .dismiss, "Dismiss on a deleted file dismisses")
        expect(press(.saveUnknown(0), primary: false) == .dismiss, "Dismiss on an unknown save dismisses")
        expect(press(.saveUnknown(7), primary: true) == .nothing && press(.saveUnknown(7), primary: false) == .nothing, "a save that is still being waited for has no buttons")
        expect(press(.saveResolved, primary: false) == .dismiss, "Dismiss on a resolved save dismisses")
        expect(press(.loadFailed("x"), primary: true, unsaved: true) == .dismiss, "a load failure over unsaved text only dismisses: no retry may replace the text")
        expect(press(.loadFailed("x"), primary: true, unsaved: false) == .retry, "a load failure of a clean buffer retries")
        expect(press(.message("x"), primary: true) == .dismiss, "a message dismisses")
        expect(press(.dirtyUnconfirmed, primary: true) == .nothing && press(.none, primary: true) == .nothing, "banners without buttons do nothing")
        expect(press(.deletedOnDisk, primary: false) == .nothing, "the deleted banner has no secondary button")
    }

    static func main() async {
        dirtyTracking()
        saving()
        unsyncedSave()
        reloading()
        externalChanges()
        banners()
        guard failures.isEmpty else {
            for failure in failures { FileHandle.standardError.write(Data(("file editor model fixture failed: " + failure + "\n").utf8)) }
            exit(1)
        }
        print("File editor model passed: byte-exact dirty tracking, exact save payload and generation, typing during a save stays dirty, reload and attach gating, automatic reload keeps typed text, external-change banners and dismissals, banner buttons never reload without confirmation, a save whose directory sync failed rebases on the reported generation and can be repeated, a final unknown save outcome stops waiting.")
    }
}

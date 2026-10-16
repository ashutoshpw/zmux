import AppKit
import Darwin

private struct FilePanelFixture: Sendable {
    let directory: String
    var markdown: String { directory + "/notes.md" }
    var text: String { directory + "/plain.txt" }
    var image: String { directory + "/pixel.png" }
    var pdf: String { directory + "/page.pdf" }
    var binary: String { directory + "/blob.bin" }
    var folder: String { directory + "/folder" }
    var missing: String { directory + "/absent.txt" }
}

/// Native journey for the file panel: the real CLI (`zmux open`), the production File menu key
/// equivalents, the real bridge and a real directory. It checks observable behaviour only:
/// per-item open results, rendered Markdown, image and PDF decode, dirty marker without losing
/// the first responder, Cmd-S, external change (auto reload when clean, banner when dirty), the
/// conflict prompt's Cancel / Overwrite / Reload, dirty-close protection, reopen from disk,
/// dedupe, the discard confirmation behind a banner Reload, a panel error state recovered with
/// Retry, sixteen tabs opened by one command that all load within the load gate's limit, and no
/// descriptor held on the fixture directory after every tab is closed. Session
/// quit/restore (the `files.epoch` rotation) is not exercised: restore needs an empty desktop
/// and would destroy the app state this journey runs in. Conflict, close and discard sheets are
/// answered through the panel's own test seams so the smoke needs no human; every other path is
/// the production one.
@MainActor
enum FilePanelSmoke {
    private static let markdownSource = "# Zmux heading\n\nSome *emphasis* and a [link](https://example.org).\n\n- one\n- two\n"
    private static let textSource = "alpha\nbeta\n"
    // A 3x2 RGBA PNG and a one-page PDF, generated independently of the app under test.
    private static let pngBase64 = "iVBORw0KGgoAAAANSUhEUgAAAAMAAAACCAYAAACddGYaAAAAI0lEQVR4nGOwqTjxP6XixP+eihP/GWwqev6nVPT876no+Q8AwKkPHw7lAhQAAAAASUVORK5CYII="
    private static let pdfBase64 = "JVBERi0xLjQKMSAwIG9iago8PCAvVHlwZSAvQ2F0YWxvZyAvUGFnZXMgMiAwIFIgPj4KZW5kb2JqCjIgMCBvYmoKPDwgL1R5cGUgL1BhZ2VzIC9LaWRzIFszIDAgUl0gL0NvdW50IDEgPj4KZW5kb2JqCjMgMCBvYmoKPDwgL1R5cGUgL1BhZ2UgL1BhcmVudCAyIDAgUiAvTWVkaWFCb3ggWzAgMCAyMDAgMTAwXSAvQ29udGVudHMgNCAwIFIgL1Jlc291cmNlcyA8PCAvRm9udCA8PCAvRjEgNSAwIFIgPj4gPj4gPj4KZW5kb2JqCjQgMCBvYmoKPDwgL0xlbmd0aCAzOCA+PgpzdHJlYW0KQlQgL0YxIDE4IFRmIDIwIDUwIFRkIChabXV4IFBERikgVGogRVQKZW5kc3RyZWFtCmVuZG9iago1IDAgb2JqCjw8IC9UeXBlIC9Gb250IC9TdWJ0eXBlIC9UeXBlMSAvQmFzZUZvbnQgL0hlbHZldGljYSA+PgplbmRvYmoKeHJlZgowIDYKMDAwMDAwMDAwMCA2NTUzNSBmIAowMDAwMDAwMDA5IDAwMDAwIG4gCjAwMDAwMDAwNTggMDAwMDAgbiAKMDAwMDAwMDExNSAwMDAwMCBuIAowMDAwMDAwMjQxIDAwMDAwIG4gCjAwMDAwMDAzMjkgMDAwMDAgbiAKdHJhaWxlcgo8PCAvU2l6ZSA2IC9Sb290IDEgMCBSID4+CnN0YXJ0eHJlZgozOTkKJSVFT0YK"

    @MainActor
    private final class ConflictScript {
        private var choices: [FileConflictChoice]
        private(set) var seen: [String] = []
        init(_ choices: [FileConflictChoice]) { self.choices = choices }
        func next(_ kind: String) -> FileConflictChoice {
            seen.append(kind)
            return choices.isEmpty ? .cancel : choices.removeFirst()
        }
    }

    @MainActor
    private final class DiscardScript {
        private var answers: [Bool]
        private(set) var count = 0
        init(_ answers: [Bool]) { self.answers = answers }
        func next() -> Bool {
            count += 1
            return answers.isEmpty ? false : answers.removeFirst()
        }
    }

    @MainActor
    private final class CloseScript {
        private var choices: [FileCloseChoice]
        private(set) var count = 0
        init(_ choices: [FileCloseChoice]) { self.choices = choices }
        func next() -> FileCloseChoice {
            count += 1
            return choices.isEmpty ? .cancel : choices.removeFirst()
        }
    }

    static func validate(
        window: NSWindow,
        host: MainWindowHost,
        coordinator: ApplicationCoordinator,
        cli: @escaping @MainActor ([String], Bool) async throws -> Data,
        tabs: @MainActor () -> [TerminalTabControl]
    ) async throws {
        guard let before = coordinator.snapshot, let workspace = before.workspace,
              let originalPane = workspace.root.panes.first(where: { $0.id == workspace.focusedPane }),
              fileSurfaces(before).isEmpty else {
            throw failure("needs a focused pane and no file tab opened before the journey")
        }
        let originalSelection = originalPane.selectedSurface
        let savedResponder = window.firstResponder
        guard let png = Data(base64Encoded: pngBase64), let pdf = Data(base64Encoded: pdfBase64) else {
            throw failure("embedded image fixtures are not valid base64")
        }
        let fixture = try await makeFilePanelFixture(png: png, pdf: pdf, markdown: markdownSource, text: textSource)
        do {
            try await journey(fixture, window: window, host: host, coordinator: coordinator, cli: cli, tabs: tabs)
        } catch {
            do { try await cleanup(fixture, host: host, coordinator: coordinator, selection: originalSelection, responder: savedResponder, window: window, verifyDescriptors: false) }
            catch let secondary { throw BridgeFailure(message: "\(error); cleanup also failed: \(secondary)") }
            throw error
        }
        try await cleanup(fixture, host: host, coordinator: coordinator, selection: originalSelection, responder: savedResponder, window: window, verifyDescriptors: true)
        FileHandle.standardError.write(Data("Native file panel: zmux open mixed batch, rendered Markdown, image and PDF decode, edit/dirty marker with retained focus, Cmd-S, external reload and banner, conflict Cancel/Overwrite/Reload, dirty-close protection, cancelled multi-file close, back-to-back closes, keyboard focus after load, reopen from disk, dedupe, banner Reload asks before discarding, error state recovered with Retry, sixteen simultaneous tabs all loaded within the load gate's limit and no descriptor held after close passed\n".utf8))
    }

    private static func journey(
        _ f: FilePanelFixture, window: NSWindow, host: MainWindowHost, coordinator: ApplicationCoordinator,
        cli: @escaping @MainActor ([String], Bool) async throws -> Data, tabs: @MainActor () -> [TerminalTabControl]
    ) async throws {
        // 1. A mixed batch through the real CLI: four tabs open, three items fail explicitly.
        let opened = try openResult(try await cli(["open", f.markdown, f.text, f.image, f.pdf, f.binary, f.folder, f.missing], true))
        guard opened["ok"] as? Bool == false, let items = opened["items"] as? [[String: Any]], items.count == 7 else {
            throw failure("a mixed open did not report seven per-item results")
        }
        let expectedOK = [true, true, true, true, false, false, false]
        let expectedCodes: [Int: String] = [4: "binary_file", 5: "is_directory", 6: "not_found"]
        for (index, item) in items.enumerated() {
            guard item["ok"] as? Bool == expectedOK[index] else { throw failure("item \(index) had the wrong outcome: \(item)") }
            if let code = expectedCodes[index], (item["error"] as? [String: Any])?["code"] as? String != code {
                throw failure("item \(index) should fail with \(code): \(item)")
            }
        }
        var ids: [String: UInt64] = [:]
        try await wait("four file tabs with loaded panels") {
            guard let snapshot = coordinator.snapshot else { return false }
            let surfaces = fileSurfaces(snapshot)
            guard surfaces.count == 4 else { return false }
            for surface in surfaces { if let path = surface.file?.path { ids[path] = surface.id } }
            return ids.count == 4 && surfaces.allSatisfy { coordinator.files[$0.id]?.isLoaded == true }
        }
        guard let markdownID = ids[f.markdown], let textID = ids[f.text], let imageID = ids[f.image], let pdfID = ids[f.pdf],
              let markdownPanel = coordinator.files[markdownID], let text = coordinator.files[textID],
              let image = coordinator.files[imageID], let pdf = coordinator.files[pdfID] else {
            throw failure("opened tabs do not map to the fixture paths")
        }

        // 2. Each mode presents its own content, with no placeholder standing in for a failure.
        try await wait("Markdown rendered") { markdownPanel.presentationSummary == "markdown:rendered" }
        // A tab opened by `zmux open` was selected while it was still loading; once the content
        // replaced the loading state the keyboard must have followed it (typing, Cmd-V, arrows).
        try await wait("keyboard focus moved into the rendered Markdown view") {
            window.firstResponder === markdownPanel.markdownTextView && markdownPanel.preferredResponder === markdownPanel.markdownTextView
        }
        let rendered = markdownPanel.markdownTextView.string
        guard rendered.contains("Zmux heading"), rendered.contains("emphasis"), !rendered.contains("# Zmux"), !rendered.contains("*emphasis*") else {
            throw failure("rendered Markdown still shows its syntax: \(rendered)")
        }
        let linkAt = (rendered as NSString).range(of: "link")
        guard linkAt.location != NSNotFound,
              markdownPanel.markdownTextView.textStorage?.attribute(.link, at: linkAt.location, effectiveRange: nil) as? URL == URL(string: "https://example.org") else {
            throw failure("an https link was not made clickable in rendered Markdown")
        }
        markdownPanel.toggleMarkdownSource()
        guard markdownPanel.presentationSummary == "markdown:source", markdownPanel.editorTextView.string == markdownSource else {
            throw failure("the Markdown source toggle did not show the exact source")
        }
        markdownPanel.toggleMarkdownSource()
        try await wait("Markdown rendered again") { markdownPanel.presentationSummary == "markdown:rendered" }
        try await wait("image decoded") { image.presentationSummary == "image:3x2" }
        try await wait("PDF decoded") { pdf.presentationSummary == "pdf:1" }

        // 3. Opening an open file again focuses it instead of creating a second tab.
        let again = try openResult(try await cli(["open", "--no-focus", f.text], false))
        guard again["ok"] as? Bool == true, (again["items"] as? [[String: Any]])?.first?["deduplicated"] as? Bool == true,
              coordinator.snapshot.map({ fileSurfaces($0).count }) == 4 else {
            throw failure("opening an open file did not deduplicate")
        }

        // 4. Editing marks the tab dirty without rebuilding the pane or dropping focus.
        host.selectTab(textID)
        let editor = text.editorTextView
        try await wait("text editor focused") { window.isKeyWindow && window.firstResponder === editor }
        guard editor.string == textSource, editor.isEditable else { throw failure("the text editor does not show the file or is read-only") }
        typeText("X", into: editor)
        try await wait("dirty marker with retained focus") {
            text.isDirty && modelDirty(coordinator, textID) == true && window.firstResponder === editor
                && tabs().contains { $0.surface.id == textID && labels(in: $0).contains { $0.hasPrefix("• ") } }
        }
        let refused = String(decoding: try await cli(["close-surface", "--surface", String(textID)], true), as: UTF8.self)
        guard refused.contains("unsaved"), coordinator.files[textID] === text else {
            throw failure("closing a dirty file from the CLI was not refused: \(refused)")
        }

        // 5. Cmd-S writes the file through the file service.
        try command("s", code: 1, window: window)
        try await wait("saved and clean") { !text.isDirty && modelDirty(coordinator, textID) == false }
        guard try await read(f.text) == "X" + textSource else { throw failure("Cmd-S did not write the edited text") }

        // 6. A clean panel adopts an external write on its own.
        try await write("gamma\n", to: f.text)
        try await wait("clean panel reloaded the external write") { editor.string == "gamma\n" && !text.isDirty }

        // 7. A dirty panel keeps its edits, says so, and asks before overwriting.
        typeText("Y", into: editor)
        try await wait("dirty again") { text.isDirty && modelDirty(coordinator, textID) == true }
        try await write("delta\n", to: f.text)
        try await wait("changed-on-disk banner") { if case .changedOnDisk = text.banner { return true } else { return false } }
        guard editor.string == "Ygamma\n" else { throw failure("an external change overwrote unsaved edits") }
        let conflicts = ConflictScript([.cancel, .overwrite, .reload])
        text.conflictChooser = { conflict in conflicts.next(conflict.kind) }
        try command("s", code: 1, window: window)
        try await wait("first conflict prompt") { conflicts.seen == ["modified"] }
        _ = try await cli(["status"], false)
        guard text.isDirty, try await read(f.text) == "delta\n" else { throw failure("Cancel in the conflict prompt changed the buffer or the file") }
        try command("s", code: 1, window: window)
        try await wait("Overwrite saved") { !text.isDirty && conflicts.seen.count == 2 }
        guard try await read(f.text) == "Ygamma\n" else { throw failure("Overwrite did not write the buffer") }
        typeText("Z", into: editor)
        try await wait("dirty for the reload choice") { text.isDirty && modelDirty(coordinator, textID) == true }
        try await write("epsilon\n", to: f.text)
        try await wait("banner before the reload choice") { if case .changedOnDisk = text.banner { return true } else { return false } }
        try command("s", code: 1, window: window)
        try await wait("Reload adopted the disk version") { editor.string == "epsilon\n" && !text.isDirty && conflicts.seen.count == 3 }
        guard try await read(f.text) == "epsilon\n" else { throw failure("Reload changed the file on disk") }
        text.conflictChooser = nil

        // 8. A dirty tab cannot be closed without a decision; reopening reads the disk.
        typeText("Q", into: editor)
        try await wait("dirty before closing") { text.isDirty && modelDirty(coordinator, textID) == true }
        let closing = CloseScript([.cancel, .discard])
        coordinator.fileCloseChoiceOverride = { _ in closing.next() }
        host.closeTerminalTab(textID)
        try await wait("Cancel consulted and its request settled") { closing.count == 1 && !host.hasPendingFileCloses }
        _ = try await cli(["status"], false)
        guard coordinator.files[textID] === text, text.isDirty, coordinator.snapshot.flatMap({ fileSurface($0, textID) }) != nil else {
            throw failure("Cancel in the close sheet closed the tab or dropped the edits")
        }
        host.closeTerminalTab(textID)
        try await wait("Don't Save closed the tab") { coordinator.snapshot.flatMap({ fileSurface($0, textID) }) == nil }
        guard coordinator.orphanedFiles.isEmpty else { throw failure("a closed tab left an orphaned panel") }
        coordinator.fileCloseChoiceOverride = nil
        host.reopenClosed()
        var reopenedSurface: UInt64?
        try await wait("reopened tab reloaded from disk") {
            guard let snapshot = coordinator.snapshot,
                  let reopened = fileSurfaces(snapshot).first(where: { $0.file?.path == f.text }),
                  let view = coordinator.files[reopened.id] else { return false }
            reopenedSurface = reopened.id
            return view.isLoaded && view.editorTextView.string == "epsilon\n" && !view.isDirty
        }
        guard let reopenedID = reopenedSurface, let reopened = coordinator.files[reopenedID] else { throw failure("the reopened tab has no panel") }

        // 9. A preview whose file disappears says so and keeps the last good content.
        try await remove(f.image)
        try await wait("deleted banner on the image preview") { image.banner == .deletedOnDisk }
        guard image.presentationSummary == "image:3x2" else { throw failure("a deleted image replaced its last good preview with a placeholder") }

        // 10. A multi-file close that is cancelled part way changes nothing: Don't Save on the
        // first file must not clear its edits when the second file answers Cancel.
        host.selectTab(reopenedID)
        try await wait("reopened text editor focused") { window.isKeyWindow && window.firstResponder === reopened.editorTextView }
        typeText("R", into: reopened.editorTextView)
        host.selectTab(markdownID)
        try await wait("Markdown tab selected") { window.isKeyWindow && window.firstResponder === markdownPanel.markdownTextView }
        markdownPanel.toggleMarkdownSource()
        let sourceEditor = markdownPanel.editorTextView
        try await wait("Markdown source editor focused") { markdownPanel.presentationSummary == "markdown:source" && window.firstResponder === sourceEditor }
        typeText("M", into: sourceEditor)
        try await wait("two dirty panels") {
            markdownPanel.isDirty && reopened.isDirty && modelDirty(coordinator, markdownID) == true && modelDirty(coordinator, reopenedID) == true
        }
        let partial = CloseScript([.discard, .cancel])
        coordinator.fileCloseChoiceOverride = { _ in partial.next() }
        let aborted = await coordinator.resolveFilesBeforeClose([markdownID, reopenedID], window: window)
        guard aborted == nil, partial.count == 2 else { throw failure("a cancelled multi-file close was not refused") }
        _ = try await cli(["status"], false)
        guard markdownPanel.isDirty, markdownPanel.hasUnsavedChanges, sourceEditor.string == "M" + markdownSource,
              reopened.isDirty, reopened.editorTextView.string == "Repsilon\n" else {
            throw failure("Cancel after Don't Save dropped the earlier file's edits or its dirty tracking")
        }
        try await wait("both panels still guarded in the model") {
            modelDirty(coordinator, markdownID) == true && modelDirty(coordinator, reopenedID) == true
        }
        // Two Don't Save answers settled together: a snapshot poll that lands between the first
        // file's settle and the end of the close must not mark the first file dirty in the model
        // again, or the core would refuse the close of a panel that already reads as clean. While
        // the core has not answered the close, the panels still hold their text.
        let both = CloseScript([.discard, .discard])
        coordinator.fileCloseChoiceOverride = { _ in both.next() }
        let firstSettlement = await coordinator.resolveFilesBeforeClose([markdownID, reopenedID], window: window)
        coordinator.fileCloseChoiceOverride = nil
        guard let pending = firstSettlement, both.count == 2 else { throw failure("a two-file Don't Save close was refused") }
        guard markdownPanel.isDirty, reopened.isDirty, markdownPanel.hasUnsavedChanges, reopened.hasUnsavedChanges else {
            throw failure("Don't Save dropped the edits before the core closed the tab")
        }
        for _ in 0..<4 {
            _ = try await cli(["status"], false)
            guard modelDirty(coordinator, markdownID) == false, modelDirty(coordinator, reopenedID) == false,
                  markdownPanel.isDirty, reopened.isDirty else {
                throw failure("a snapshot poll re-marked a pending Don't Save dirty in the model, or the panel lost its edits")
            }
        }
        // The core refused (or never received) the close: both panels keep their edits, and the
        // model guards them again so a later close or quit still asks.
        pending.finish(closeSucceeded: false)
        guard markdownPanel.isDirty, reopened.isDirty, sourceEditor.string == "M" + markdownSource,
              reopened.editorTextView.string == "Repsilon\n", !markdownPanel.isDiscardPending, !reopened.isDiscardPending else {
            throw failure("a refused close discarded the edits or left the discard held")
        }
        try await wait("both panels guarded in the model after a refused close") {
            modelDirty(coordinator, markdownID) == true && modelDirty(coordinator, reopenedID) == true
        }
        // The same answers, now with the core closing the tab: only then are the edits dropped.
        let repeated = CloseScript([.discard, .discard])
        coordinator.fileCloseChoiceOverride = { _ in repeated.next() }
        let secondSettlement = await coordinator.resolveFilesBeforeClose([markdownID, reopenedID], window: window)
        coordinator.fileCloseChoiceOverride = nil
        guard let applied = secondSettlement, repeated.count == 2 else { throw failure("a repeated two-file Don't Save close was refused") }
        applied.finish(closeSucceeded: true)
        guard !markdownPanel.isDirty, !reopened.isDirty else { throw failure("Don't Save left a panel dirty after the core closed") }
        for _ in 0..<4 {
            _ = try await cli(["status"], false)
            guard modelDirty(coordinator, markdownID) == false, modelDirty(coordinator, reopenedID) == false,
                  !markdownPanel.isDirty, !reopened.isDirty else {
                throw failure("a snapshot poll re-marked a discarded panel dirty in the model")
            }
        }
        try await wait("both models stay clean after Don't Save") {
            modelDirty(coordinator, markdownID) == false && modelDirty(coordinator, reopenedID) == false
        }
        // The hold is lifted once the discard is applied: new edits are mirrored again.
        host.selectTab(reopenedID)
        try await wait("reopened text editor focused after Don't Save") { window.isKeyWindow && window.firstResponder === reopened.editorTextView }
        typeText("S", into: reopened.editorTextView)
        host.selectTab(markdownID)
        try await wait("Markdown source editor focused after Don't Save") { window.isKeyWindow && window.firstResponder === sourceEditor }
        typeText("T", into: sourceEditor)
        try await wait("edits after Don't Save are guarded again") {
            markdownPanel.isDirty && reopened.isDirty && modelDirty(coordinator, markdownID) == true && modelDirty(coordinator, reopenedID) == true
        }
        // Closing several tabs in a row answers every request, in order; none is dropped.
        let burst = CloseScript([.discard, .discard])
        coordinator.fileCloseChoiceOverride = { _ in burst.next() }
        host.closeTerminalTab(markdownID)
        host.closeTerminalTab(reopenedID)
        try await wait("both tabs closed by back-to-back close requests") {
            guard let snapshot = coordinator.snapshot else { return false }
            return fileSurface(snapshot, markdownID) == nil && fileSurface(snapshot, reopenedID) == nil
        }
        guard burst.count == 2, coordinator.orphanedFiles.isEmpty else {
            throw failure("back-to-back closes of dirty tabs were dropped or left an orphaned panel")
        }
        coordinator.fileCloseChoiceOverride = nil

        // 11. A banner Reload replaces the buffer, so with unsaved text it asks first, exactly like
        // File > Reload from Disk: Cancel changes nothing, Discard adopts the disk version.
        _ = try openResult(try await cli(["open", f.text], false))
        var secondID: UInt64?
        try await wait("text tab opened again and loaded") {
            guard let snapshot = coordinator.snapshot,
                  let surface = fileSurfaces(snapshot).first(where: { $0.file?.path == f.text }),
                  let view = coordinator.files[surface.id], view.isLoaded else { return false }
            secondID = surface.id
            return true
        }
        guard let id = secondID, let panel = coordinator.files[id] else { throw failure("the text tab did not open for the discard check") }
        host.selectTab(id)
        try await wait("text editor focused for the discard check") { window.isKeyWindow && window.firstResponder === panel.editorTextView }
        typeText("K", into: panel.editorTextView)
        try await wait("dirty before the banner Reload") { panel.isDirty && modelDirty(coordinator, id) == true }
        try await write("zeta\n", to: f.text)
        try await wait("changed-on-disk banner on the dirty panel") { if case .changedOnDisk = panel.banner { return true } else { return false } }
        let confirmations = DiscardScript([false, true])
        panel.discardConfirmation = { confirmations.next() }
        guard panel.clickBannerButton(primary: true) else { throw failure("the changed-on-disk banner has no Reload button") }
        try await wait("first discard confirmation") { confirmations.count == 1 }
        _ = try await cli(["status"], false)
        guard panel.isDirty, panel.editorTextView.string == "K" + "epsilon\n" else {
            throw failure("Cancel in the discard confirmation still replaced the unsaved text")
        }
        guard panel.clickBannerButton(primary: true) else { throw failure("the banner lost its Reload button after Cancel") }
        try await wait("Discard adopted the disk version") { confirmations.count == 2 && panel.editorTextView.string == "zeta\n" && !panel.isDirty }
        panel.discardConfirmation = nil

        // 12. A tab whose file vanished before it was reopened shows an explicit error state; the
        // file coming back and Retry load it, with no tab recreated.
        host.closeTerminalTab(id)
        try await wait("clean text tab closed") { coordinator.snapshot.flatMap({ fileSurface($0, id) }) == nil }
        try await remove(f.text)
        host.reopenClosed()
        var failedID: UInt64?
        try await wait("reopened tab shows an error state") {
            guard let snapshot = coordinator.snapshot,
                  let surface = fileSurfaces(snapshot).first(where: { $0.file?.path == f.text }),
                  let view = coordinator.files[surface.id], view.failureCode != nil else { return false }
            failedID = surface.id
            return true
        }
        guard let brokenID = failedID, let broken = coordinator.files[brokenID], broken.failureCode == "not_found",
              broken.presentationSummary == "error:not_found" else {
            throw failure("a vanished file did not produce the not_found error state")
        }
        try await write("restored\n", to: f.text)
        broken.retry()
        try await wait("Retry loaded the file") { broken.isLoaded && broken.failureCode == nil && broken.editorTextView.string == "restored\n" }

        // 13. Sixteen file tabs from one command all load: the load gate queues them instead of
        // overflowing the Rust file worker (which answers busy past eight queued jobs), none ends
        // in an error state, and no more than the gate's limit was ever in flight. The core admits
        // sixteen file tabs across the whole desktop, so the tabs the earlier steps kept (the
        // image, the PDF and the reopened error tab, all clean) are closed first: opened beside
        // them the last three of sixteen new files would be answered `file_capacity`.
        for keptID in [imageID, pdfID, brokenID] { host.closeTerminalTab(keptID) }
        try await wait("every earlier file tab closed") { coordinator.snapshot.map { fileSurfaces($0).isEmpty } == true && coordinator.files.isEmpty }
        let manyPaths = try await writeManyFiles(count: 16, in: f.directory)
        let manyOpened = try openResult(try await cli(["open"] + manyPaths, false))
        guard manyOpened["ok"] as? Bool == true, (manyOpened["items"] as? [[String: Any]])?.count == 16 else {
            throw failure("opening sixteen files at once did not report sixteen successes: \(manyOpened)")
        }
        try await wait("sixteen simultaneous file tabs loaded") {
            guard let snapshot = coordinator.snapshot else { return false }
            let panels = fileSurfaces(snapshot).filter { surface in surface.file.map { manyPaths.contains($0.path) } == true }
            return panels.count == 16 && panels.allSatisfy { coordinator.files[$0.id]?.isLoaded == true }
        }
        guard let manySnapshot = coordinator.snapshot else { throw failure("no snapshot after the sixteen-file open") }
        let manyPanels = fileSurfaces(manySnapshot).compactMap { surface in
            surface.file.flatMap { manyPaths.contains($0.path) ? coordinator.files[surface.id] : nil }
        }
        guard manyPanels.count == 16, manyPanels.allSatisfy({ $0.failureCode == nil }) else {
            throw failure("a tab opened with fifteen others ended in an error state instead of loading")
        }
        let gate = coordinator.fileLoadGate
        guard gate.peakInFlightCount >= 1, gate.peakInFlightCount <= FileLoadPolicy.defaultMaxInFlight,
              gate.inFlightCount == 0, gate.waitingCount == 0 else {
            throw failure("the load gate ran \(gate.peakInFlightCount) loads at once (limit \(FileLoadPolicy.defaultMaxInFlight)) or left work behind")
        }
    }

    private static func cleanup(_ f: FilePanelFixture, host: MainWindowHost, coordinator: ApplicationCoordinator, selection: UInt64,
                                responder: NSResponder?, window: NSWindow, verifyDescriptors: Bool) async throws {
        // A failed journey may leave a dirty tab; answer its sheet instead of waiting for a person.
        coordinator.fileCloseChoiceOverride = { _ in .discard }
        for panel in coordinator.files.values { panel.conflictChooser = nil }
        for surface in coordinator.snapshot.map({ fileSurfaces($0) }) ?? [] { host.closeTerminalTab(surface.id) }
        try await wait("fixture file tabs closed") { coordinator.snapshot.map { fileSurfaces($0).isEmpty } == true && coordinator.files.isEmpty }
        coordinator.fileCloseChoiceOverride = nil
        // Every fixture tab is closed: nothing in this process may still hold a descriptor on the
        // fixture directory. Detach is asynchronous, so poll briefly before declaring a leak.
        if verifyDescriptors {
            var held: [String] = []
            for _ in 0..<20 {
                held = try await openDescriptorPaths(under: f.directory)
                if held.isEmpty { break }
                try await Task.sleep(for: .milliseconds(250))
            }
            guard held.isEmpty else { throw failure("descriptors still open on the fixture after every tab was closed: \(held)") }
        }
        host.selectTab(selection)
        try await wait("original tab selected") {
            coordinator.snapshot?.workspace?.root.panes.contains { $0.selectedSurface == selection } == true
        }
        if let responder { window.makeFirstResponder(responder) }
        try await remove(f.directory)
    }

    // MARK: - Helpers

    private static func fileSurfaces(_ snapshot: DesktopSnapshot) -> [Surface] {
        snapshot.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.filter { $0.kind == "file" }
    }
    private static func fileSurface(_ snapshot: DesktopSnapshot, _ id: UInt64) -> Surface? {
        fileSurfaces(snapshot).first { $0.id == id }
    }
    private static func modelDirty(_ coordinator: ApplicationCoordinator, _ id: UInt64) -> Bool? {
        coordinator.snapshot.flatMap { fileSurface($0, id) }?.file?.dirty
    }
    private static func labels(in view: NSView) -> [String] {
        (view as? NSTextField).map { [$0.stringValue] } ?? view.subviews.flatMap { labels(in: $0) }
    }
    private static func typeText(_ text: String, into editor: NSTextView) {
        editor.setSelectedRange(NSRange(location: 0, length: 0))
        editor.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
    }
    private static func command(_ key: String, code: UInt16, window: NSWindow) throws {
        NSApp.mainMenu?.items.forEach { $0.submenu?.update() }
        guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.command],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
            characters: key, charactersIgnoringModifiers: key, isARepeat: false, keyCode: code),
              NSApp.mainMenu?.performKeyEquivalent(with: event) == true else {
            throw failure("Command-\(key.uppercased()) was not handled by an enabled File menu item")
        }
    }
    /// Paths under `directory` this process holds open, listed by `lsof` (always present on macOS).
    private static func openDescriptorPaths(under directory: String) async throws -> [String] {
        let pid = ProcessInfo.processInfo.processIdentifier
        return try await Task.detached {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/sbin/lsof")
            process.arguments = ["-nP", "-F", "n", "-p", String(pid)]
            let pipe = Pipe()
            process.standardOutput = pipe
            process.standardError = FileHandle.nullDevice
            try process.run()
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            guard process.terminationStatus == 0 else {
                throw BridgeFailure(message: "Native file panel: lsof exited with status \(process.terminationStatus)")
            }
            return String(decoding: data, as: UTF8.self).split(separator: "\n").filter { $0.hasPrefix("n" + directory) }.map(String.init)
        }.value
    }
    private static func openResult(_ data: Data) throws -> [String: Any] {
        let text = String(decoding: data, as: UTF8.self)
        guard let start = text.firstIndex(of: "{"), let end = text.lastIndex(of: "}"),
              let object = try JSONSerialization.jsonObject(with: Data(text[start...end].utf8)) as? [String: Any] else {
            throw failure("zmux open printed no JSON result: " + text)
        }
        return object
    }
    private static func wait(_ message: String, _ predicate: () -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(15)
        while !predicate() {
            guard ContinuousClock.now < deadline else { throw failure("timed out waiting for " + message) }
            try await Task.sleep(for: .milliseconds(25))
        }
    }
    private static func makeFilePanelFixture(png: Data, pdf: Data, markdown: String, text: String) async throws -> FilePanelFixture {
        try await Task.detached {
            let base = FileManager.default.temporaryDirectory.appendingPathComponent("zmux-file-panel-" + UUID().uuidString, isDirectory: true)
            try FileManager.default.createDirectory(at: base, withIntermediateDirectories: false)
            // The file service canonicalizes paths; compare against the same realpath.
            guard let resolved = realpath(base.path, nil) else { throw BridgeFailure(message: "Native file panel: fixture directory has no realpath") }
            defer { free(resolved) }
            let directory = String(cString: resolved)
            try Data(markdown.utf8).write(to: URL(fileURLWithPath: directory + "/notes.md"))
            try Data(text.utf8).write(to: URL(fileURLWithPath: directory + "/plain.txt"))
            try png.write(to: URL(fileURLWithPath: directory + "/pixel.png"))
            try pdf.write(to: URL(fileURLWithPath: directory + "/page.pdf"))
            try Data([0x00, 0x01, 0x02, 0xff, 0x00]).write(to: URL(fileURLWithPath: directory + "/blob.bin"))
            try FileManager.default.createDirectory(atPath: directory + "/folder", withIntermediateDirectories: false)
            return FilePanelFixture(directory: directory)
        }.value
    }
    private static func writeManyFiles(count: Int, in directory: String) async throws -> [String] {
        try await Task.detached {
            try (0..<count).map { (index: Int) -> String in
                let path = directory + "/many-" + String(format: "%02d", index) + ".txt"
                try Data(("many " + String(index) + "\n").utf8).write(to: URL(fileURLWithPath: path))
                return path
            }
        }.value
    }
    private static func write(_ text: String, to path: String) async throws {
        try await Task.detached { try Data(text.utf8).write(to: URL(fileURLWithPath: path), options: .atomic) }.value
    }
    private static func read(_ path: String) async throws -> String {
        try await Task.detached { String(decoding: try Data(contentsOf: URL(fileURLWithPath: path)), as: UTF8.self) }.value
    }
    private static func remove(_ path: String) async throws {
        try await Task.detached { try FileManager.default.removeItem(atPath: path) }.value
    }
    private static func failure(_ text: String) -> BridgeFailure { BridgeFailure(message: "Native file panel: " + text) }
}

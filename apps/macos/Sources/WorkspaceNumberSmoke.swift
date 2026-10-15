import AppKit

/// Synthetic key events go through the production NSWindow key-equivalent route,
/// with real terminal/WebKit responders and the shared Rust selection owner.
@MainActor
enum WorkspaceNumberSmoke {
    private static func wait(_ message: String, _ predicate: () -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(8)
        while !predicate() {
            guard ContinuousClock.now < deadline else { throw BridgeFailure(message: message) }
            try await Task.sleep(for: .milliseconds(25))
        }
    }
    private static func addressEditor(in browser: BrowserView) -> NSTextView? {
        var pending: [NSView] = [browser]
        while let view = pending.popLast() {
            if let field = view as? NSTextField, field.isEditable,
               let editor = field.currentEditor() as? NSTextView,
               browser.window?.firstResponder === editor { return editor }
            pending.append(contentsOf: view.subviews)
        }
        return nil
    }
    static func validate(
        window: NSWindow,
        cli: @escaping @MainActor ([String], Bool) async throws -> Data,
        snapshot: @MainActor () -> DesktopSnapshot?,
        terminals: @MainActor () -> [UInt64: TerminalView],
        browser: @MainActor (UInt64) -> BrowserView?
    ) async throws {
        let status = try await cli(["status"], false)
        guard let current = try JSONSerialization.jsonObject(with: status) as? [String: Any],
              let revision = current["model_revision"] as? UInt64 else { throw BridgeFailure(message: "Numbered fixture could not observe its executor revision") }
        try await wait("Numbered fixture did not observe prior cleanup") { (snapshot()?.model.revision ?? 0) >= revision }
        guard let initial = snapshot(), let owner = initial.model.windows.first(where: { $0.id == initial.model.focusedWindow }),
              owner.workspaceIds.count >= 2, owner.workspaceIds.count < 8,
              let original = owner.selectedWorkspace, window.isKeyWindow else {
            throw BridgeFailure(message: "Numbered fixture needs two ordinary rows and a missing numbered slot")
        }
        let order = owner.workspaceIds
        let remembered = Dictionary(uniqueKeysWithValues: initial.model.workspaces.map { workspace in
            (workspace.id, [workspace.focusedPane] + workspace.root.panes.flatMap { [$0.id, $0.selectedSurface] })
        })
        let views = terminals().mapValues { ObjectIdentifier($0) }
        let browsers = initial.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.filter { $0.kind == "browser" }.compactMap { surface -> (UInt64, ObjectIdentifier, UUID, UInt64)? in
            guard let view = browser(surface.id) else { return nil }
            return (surface.id, ObjectIdentifier(view), view.hostID, view.documentGeneration)
        }
        guard !browsers.isEmpty else { throw BridgeFailure(message: "Numbered fixture needs the real selected WebKit host") }
        var processes: [UInt64: UInt32] = [:]
        @MainActor func retained() throws {
            guard let value = snapshot(), value.model.focusedWindow == owner.id,
                  window.isKeyWindow, NSApp.keyWindow === window,
                  Dictionary(uniqueKeysWithValues: value.model.workspaces.map { workspace in
                      (workspace.id, [workspace.focusedPane] + workspace.root.panes.flatMap { [$0.id, $0.selectedSurface] })
                  }) == remembered,
                  terminals().mapValues({ ObjectIdentifier($0) }) == views,
                  browsers.allSatisfy({ id, identity, host, generation in
                      guard let view = browser(id) else { return false }
                      return ObjectIdentifier(view) == identity && view.hostID == host && view.documentGeneration == generation
                  }) else { throw BridgeFailure(message: "Numbered selection replaced native content, changed remembered pane/tab or crossed windows") }
            for frame in value.frames {
                guard let pid = frame.processId else { throw BridgeFailure(message: "Numbered selection lost a live PTY") }
                if let original = processes[frame.sessionId], original != pid { throw BridgeFailure(message: "Numbered selection restarted a PTY") }
                processes[frame.sessionId] = pid
            }
        }
        @MainActor func press(_ digit: Int) throws {
            let codes: [Int: UInt16] = [1:18, 2:19, 8:28, 9:25]
            guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.command], timestamp: 0,
                windowNumber: window.windowNumber, context: nil, characters: String(digit), charactersIgnoringModifiers: String(digit),
                isARepeat: false, keyCode: codes[digit] ?? 0), window.performKeyEquivalent(with: event) else {
                throw BridgeFailure(message: "Native window did not consume Command-\(digit)")
            }
        }
        @MainActor func selected(_ id: UInt64, addressFocus: Bool = false) async throws {
            try await wait("Numbered selection did not reach its remembered native content responder") {
                guard let value = snapshot(), value.model.focusedWorkspace == id,
                      let workspace = value.workspace,
                      let pane = workspace.root.panes.first(where: { $0.id == workspace.focusedPane }) else { return false }
                if let session = pane.sessionId { return window.firstResponder === terminals()[session] }
                guard let host = browser(pane.selectedSurface) else { return false }
                if addressFocus { return addressEditor(in: host) != nil }
                guard let responder = window.firstResponder as? NSView else { return false }
                return responder === host.webView || responder.isDescendant(of: host.webView)
            }
            try retained()
        }
        @MainActor func missing() async throws {
            guard let before = snapshot(), let responder = window.firstResponder else { throw BridgeFailure(message: "Missing-number fixture lost its responder") }
            try press(8)
            try await wait("Consumed missing number did not reach the executor") { snapshot()?.lastRequestId != before.lastRequestId }
            guard snapshot()?.model.revision == before.model.revision, snapshot()?.model.focusedWorkspace == before.model.focusedWorkspace,
                  window.firstResponder === responder else { throw BridgeFailure(message: "Unavailable workspace number mutated selection or revision") }
            try retained()
        }
        guard let originalPane = initial.workspace?.root.panes.first(where: { $0.id == initial.workspace?.focusedPane }),
              let originalBrowser = browser(originalPane.selectedSurface),
              window.makeFirstResponder(originalBrowser.webView) else { throw BridgeFailure(message: "Numbered fixture could not focus its real loaded document") }
        // This strong regression fails with unconditional address focus on layout
        // switches. Native red/green execution requires the hosted macOS gate.
        try await selected(original)
        try press(9); try await selected(order.last!)
        try await missing() // Terminal-focused missing slot must not reach shell input.
        try press(1); try await selected(order[0])
        try await missing() // Browser-focused missing slot must also be consumed.
        guard let addressEvent = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.command], timestamp: 0,
            windowNumber: window.windowNumber, context: nil, characters: "l", charactersIgnoringModifiers: "l", isARepeat: false, keyCode: 37),
              NSApp.mainMenu?.performKeyEquivalent(with: addressEvent) == true else { throw BridgeFailure(message: "Command-L did not reach the real browser address action") }
        try await selected(original, addressFocus: true)
        guard let editor = addressEditor(in: originalBrowser), editor.string.utf16.count >= 4 else { throw BridgeFailure(message: "Loaded address field is unavailable") }
        let addressRange = NSRange(location: 1, length: 3)
        editor.setSelectedRange(addressRange)
        try press(9); try await selected(order.last!)
        try press(1); try await selected(original, addressFocus: true)
        guard addressEditor(in: originalBrowser)?.selectedRange() == addressRange else { throw BridgeFailure(message: "Workspace return lost the explicit address selection") }
        try await missing()
        guard window.makeFirstResponder(originalBrowser.webView) else { throw BridgeFailure(message: "Could not return to the real browser document") }
        let reversed = Array(order.reversed())
        let scope = ["--window", String(owner.id)]
        _ = try await cli(["reorder-workspaces", "--order", reversed.map(String.init).joined(separator: ",")] + scope, false)
        try await wait("Numbered fixture did not observe the reordered sidebar") { snapshot()?.model.windows.first(where: { $0.id == owner.id })?.workspaceIds == reversed }
        try press(1); try await selected(reversed[0])
        try press(9); try await selected(reversed.last!)
        _ = try await cli(["reorder-workspaces", "--order", order.map(String.init).joined(separator: ",")] + scope, false)
        try await wait("Numbered fixture did not restore sidebar order") { snapshot()?.model.windows.first(where: { $0.id == owner.id })?.workspaceIds == order }
        guard let menu = NSApp.mainMenu?.items.compactMap(\.submenu).first(where: { menu in
            menu.items.contains { $0.title == "Workspace 2" && $0.keyEquivalent == "2" }
        }), let item = menu.items.first(where: { $0.title == "Workspace 2" && $0.keyEquivalent == "2" }) else {
            throw BridgeFailure(message: "Native numbered workspace menu is unavailable")
        }
        menu.performActionForItem(at: menu.index(of: item))
        try await selected(order[1])
        guard let originalIndex = order.firstIndex(of: original) else { throw BridgeFailure(message: "Numbered fixture lost its original workspace") }
        try press(originalIndex + 1); try await selected(original)
        // A genuinely new blank tab keeps its initial address preference.
        let createdData = try await cli(["browser", "open", "about:blank"], false)
        guard let created = try JSONSerialization.jsonObject(with: createdData) as? [String: Any],
              let surfaces = created["created_surfaces"] as? [[String: Any]],
              let blankID = surfaces.first?["surface_id"] as? UInt64 else { throw BridgeFailure(message: "Blank focus fixture omitted its surface") }
        do {
            try await wait("New blank browser did not focus its actual address editor") {
                guard let value = snapshot(), value.workspace?.root.panes.contains(where: { $0.selectedSurface == blankID }) == true,
                      let blank = browser(blankID) else { return false }
                return addressEditor(in: blank) != nil
            }
        } catch {
            do { _ = try await cli(["close-surface", "--surface", String(blankID)], false) }
            catch { FileHandle.standardError.write(Data(("Blank focus fixture cleanup: " + error.localizedDescription + "\n").utf8)) }
            throw error
        }
        _ = try await cli(["close-surface", "--surface", String(blankID)], false)
        _ = try await cli(["select-surface", "--surface", String(originalPane.selectedSurface)], false)
        try await wait("Blank focus fixture did not remove its native host") { browser(blankID) == nil }
        try await selected(original)
        FileHandle.standardError.write(Data("Native numbered workspaces: real key-equivalent/menu, canonical reorder, 9=last, missing-slot consumption, document/address intent and blank initial focus passed\n".utf8))
    }
}

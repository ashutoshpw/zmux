import AppKit

/// Native content retention and actual menu/key-equivalent routing. Physical
/// AppKit drag delivery still requires the native manual drag acceptance pass.
@MainActor
enum SurfaceOrderSmoke {
    private static func wait(_ message: String, _ predicate: () -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(8)
        while !predicate() {
            guard ContinuousClock.now < deadline else { throw BridgeFailure(message: message) }
            try await Task.sleep(for: .milliseconds(25))
        }
    }
    private static func selection(_ value: DesktopSnapshot?) -> [UInt64] {
        guard let model = value?.model else { return [] }
        var selected: [UInt64] = [model.focusedWindow ?? 0, model.focusedWorkspace ?? 0]
        for window in model.windows {
            selected.append(window.id)
            selected.append(window.selectedWorkspace ?? 0)
        }
        for workspace in model.workspaces {
            selected.append(workspace.id)
            selected.append(workspace.focusedPane)
            for pane in workspace.root.panes {
                selected.append(pane.id)
                selected.append(pane.selectedSurface)
            }
        }
        return selected
    }
    static func validate(
        window: NSWindow,
        cli: @escaping @MainActor ([String], Bool) async throws -> Data,
        snapshot: @MainActor () -> DesktopSnapshot?,
        tabs: @MainActor () -> [TerminalTabControl],
        terminals: @MainActor () -> [UInt64: TerminalView],
        browser: @MainActor (UInt64) -> BrowserView?
    ) async throws {
        let status = try await cli(["status"], false)
        guard let object = try JSONSerialization.jsonObject(with: status) as? [String: Any],
              let revision = object["model_revision"] as? UInt64 else { throw BridgeFailure(message: "Surface order fixture has no executor revision") }
        try await wait("Surface order fixture did not settle") { (snapshot()?.model.revision ?? 0) >= revision }
        guard let initial = snapshot(), let workspace = initial.workspace,
              let pane = workspace.root.panes.first(where: { $0.id == workspace.focusedPane }), pane.surfaces.count > 1,
              let responder = window.firstResponder, window.isKeyWindow else { throw BridgeFailure(message: "Surface order fixture needs two live tabs and native focus") }
        let original = pane.surfaces.map(\.id)
        let selected = pane.selectedSurface
        let remembered = selection(initial)
        let nativeTerminals = terminals().mapValues { ObjectIdentifier($0) }
        let nativeBrowsers = initial.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.filter { $0.kind == "browser" }.compactMap { surface -> (UInt64, ObjectIdentifier, UUID, UInt64)? in
            guard let view = browser(surface.id) else { return nil }
            return (surface.id, ObjectIdentifier(view), view.hostID, view.documentGeneration)
        }
        guard !nativeBrowsers.isEmpty else { throw BridgeFailure(message: "Surface order fixture needs a live browser host") }
        @MainActor func settled(_ expected: [UInt64], keepResponder: Bool = true) async throws {
            try await wait("Surface order did not reach the native tab strip") {
                snapshot()?.model.workspaces.first(where: { $0.id == workspace.id })?.root.panes.first(where: { $0.id == pane.id })?.surfaces.map(\.id) == expected
                    && tabs().filter { $0.paneID == pane.id }.map { $0.surface.id } == expected
            }
            guard selection(snapshot()) == remembered, window.isKeyWindow, NSApp.keyWindow === window,
                  (!keepResponder || window.firstResponder === responder), terminals().mapValues({ ObjectIdentifier($0) }) == nativeTerminals,
                  nativeBrowsers.allSatisfy({ id, identity, host, generation in
                      guard let view = browser(id) else { return false }
                      return ObjectIdentifier(view) == identity && view.hostID == host && view.documentGeneration == generation
                  }) else { throw BridgeFailure(message: "Surface reorder changed selection, native focus or live terminal/browser identity") }
        }
        let subject = original.first(where: { $0 != selected })!
        let target = ["--surface", String(subject)]
        _ = try await cli(["reorder-surface"] + target + ["--index", String(Int64.max)], false)
        var moved = original.filter { $0 != subject }; moved.append(subject)
        try await settled(moved)
        for placement in ["--before", "--after"] {
            _ = try await cli(["reorder-surface"] + target + [placement, String(subject)], false)
            try await settled(moved)
        }
        _ = try await cli(["reorder-surface", String(subject), String(original.firstIndex(of: subject)!)], false)
        try await settled(original)
        // Put the selected tab at the left boundary without changing selection.
        _ = try await cli(["reorder-surface", String(selected), "--index", String(Int64.min)], false)
        let atLeft = [selected] + original.filter { $0 != selected }
        try await settled(atLeft)
        guard let menu = NSApp.mainMenu?.items.compactMap(\.submenu).first(where: { $0.items.contains { $0.title == "Move Tab Right" } }),
              let left = menu.items.first(where: { $0.title == "Move Tab Left" }),
              let right = menu.items.first(where: { $0.title == "Move Tab Right" }),
              left.keyEquivalent == "{", right.keyEquivalent == "}",
              left.keyEquivalentModifierMask == [.command, .option], right.keyEquivalentModifierMask == [.command, .option] else {
            throw BridgeFailure(message: "Native surface reorder shortcuts are unavailable")
        }
        menu.performActionForItem(at: menu.index(of: left))
        // Flush the command before checking the clamped no-op.
        _ = try await cli(["status"], false)
        try await settled(atLeft, keepResponder: false)
        guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.command, .option, .shift], timestamp: 0,
            windowNumber: window.windowNumber, context: nil, characters: "}", charactersIgnoringModifiers: "}", isARepeat: false, keyCode: 30),
              window.performKeyEquivalent(with: event) else { throw BridgeFailure(message: "Native window did not consume Command-Shift-Option-]") }
        var atSecond = atLeft; atSecond.swapAt(0, 1)
        try await settled(atSecond, keepResponder: false)
        guard let tab = tabs().first(where: { $0.surface.id == selected }), let context = tab.menu,
              let moveLeft = context.items.first(where: { $0.title == "Move Left" }) else { throw BridgeFailure(message: "Actual tab context reorder menu is unavailable") }
        context.performActionForItem(at: context.index(of: moveLeft))
        try await settled(atLeft, keepResponder: false)
        _ = try await cli(["reorder-surface", String(selected), "--index", String(original.firstIndex(of: selected)!)], false)
        try await settled(original, keepResponder: false)
        FileHandle.standardError.write(Data("Native surface reorder: canonical/legacy CLI, self anchors, bounds, real tab menu/key-equivalent and native content retention passed; physical drag acceptance remains separate\n".utf8))
    }
}

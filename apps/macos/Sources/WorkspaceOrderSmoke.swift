import AppKit

/// The bundled CLI and actual workspace-row menus must reconcile the same order
/// while the key window, content responder and live native views stay in place.
@MainActor
enum WorkspaceOrderSmoke {
    private static func wait(_ message: String, _ predicate: () -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(8)
        while !predicate() {
            guard ContinuousClock.now < deadline else { throw BridgeFailure(message: message) }
            try await Task.sleep(for: .milliseconds(25))
        }
    }
    private static func selection(_ snapshot: DesktopSnapshot?) -> [UInt64] {
        guard let model = snapshot?.model else { return [] }
        var result = [model.focusedWindow ?? 0, model.focusedWorkspace ?? 0]
        for window in model.windows { result += [window.id, window.selectedWorkspace ?? 0] }
        for workspace in model.workspaces {
            result += [workspace.id, workspace.focusedPane]
            for pane in workspace.root.panes { result += [pane.id, pane.selectedSurface] }
        }
        return result
    }
    static func validate(
        window: NSWindow,
        cli: @escaping @MainActor ([String], Bool) async throws -> Data,
        snapshot: @MainActor () -> DesktopSnapshot?,
        rows: @MainActor () -> [WorkspaceRowView],
        terminals: @MainActor () -> [UInt64: TerminalView],
        browser: @MainActor (UInt64) -> BrowserView?
    ) async throws {
        // Settle earlier fixture cleanup before capturing the live identities.
        let status = try await cli(["status"], false)
        guard let current = try JSONSerialization.jsonObject(with: status) as? [String: Any],
              let revision = current["model_revision"] as? UInt64 else { throw BridgeFailure(message: "Workspace order fixture could not observe its executor revision") }
        try await wait("Workspace order fixture did not observe prior cleanup") { (snapshot()?.model.revision ?? 0) >= revision }
        guard let initial = snapshot(), let owner = initial.model.windows.first(where: { $0.id == initial.model.focusedWindow }),
              owner.workspaceIds.count >= 2, let subject = owner.workspaceIds.first,
              let responder = window.firstResponder, window.isKeyWindow else {
            throw BridgeFailure(message: "Workspace order fixture needs two workspaces and a real content responder")
        }
        let original = owner.workspaceIds
        let selected = selection(initial)
        let terminalViews = terminals().mapValues { ObjectIdentifier($0) }
        let processes = Dictionary(uniqueKeysWithValues: initial.frames.map { ($0.sessionId, $0.processId) })
        let browserIDs = initial.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.filter { $0.kind == "browser" }.map(\.id)
        let browserViews = browserIDs.compactMap { id -> (UInt64, ObjectIdentifier, UUID, UInt64)? in
            guard let view = browser(id) else { return nil }
            return (id, ObjectIdentifier(view), view.hostID, view.documentGeneration)
        }
        guard browserViews.count == browserIDs.count, !browserViews.isEmpty else { throw BridgeFailure(message: "Workspace order fixture lost its real browser host") }
        let scope = ["--window", String(owner.id)]
        @MainActor func unchanged() throws {
            guard selection(snapshot()) == selected, window.isKeyWindow, NSApp.keyWindow === window,
                  window.firstResponder === responder, terminals().mapValues({ ObjectIdentifier($0) }) == terminalViews,
                  Dictionary(uniqueKeysWithValues: (snapshot()?.frames ?? []).map { ($0.sessionId, $0.processId) }) == processes,
                  browserViews.allSatisfy({ id, identity, host, generation in
                      guard let view = browser(id) else { return false }
                      return ObjectIdentifier(view) == identity && view.hostID == host && view.documentGeneration == generation
                  }) else { throw BridgeFailure(message: "Reorder changed selection, key/responder, a live process or native content identity") }
        }
        @MainActor func settled(_ expected: [UInt64]) async throws {
            try await wait("Workspace order did not reach the actual sidebar rows") {
                snapshot()?.model.windows.first(where: { $0.id == owner.id })?.workspaceIds == expected && rows().map(\.workspaceID) == expected
            }
            try unchanged()
        }
        let preview = try await cli(["reorder-workspace", String(subject), "--after", String(subject), "--dry-run", "--expect-revision", String(initial.model.revision)] + scope, false)
        guard let receipt = try JSONSerialization.jsonObject(with: preview) as? [String: Any],
              receipt["to_index"] as? Int == 1, receipt["dry_run"] as? Bool == true,
              snapshot()?.model.revision == initial.model.revision else { throw BridgeFailure(message: "Workspace preview mutated the model or lost after-self behavior") }
        try unchanged()
        _ = try await cli(["reorder-workspace", String(subject), "--after", String(subject)] + scope, false)
        var advanced = original; advanced.removeFirst(); advanced.insert(subject, at: 1)
        try await settled(advanced)
        guard let menu = rows().first(where: { $0.workspaceID == subject })?.menu?.items.first(where: { $0.title == "Reorder Workspace" })?.submenu,
              let item = menu.items.first(where: { $0.title == "Move to Top" }), item.isEnabled else {
            throw BridgeFailure(message: "Workspace row does not expose its real enabled reorder menu")
        }
        menu.performActionForItem(at: menu.index(of: item))
        try await settled(original)
        let reversed = Array(original.reversed())
        _ = try await cli(["reorder-workspaces", "--order", reversed.map(String.init).joined(separator: ",")] + scope, false)
        try await settled(reversed)
        _ = try await cli(["reorder-workspaces", "--order", original.map(String.init).joined(separator: ",")] + scope, false)
        try await settled(original)
        FileHandle.standardError.write(Data("Native workspace reorder: CLI preview/apply/bulk, real row menu, canonical sidebar order and live focus/content retention passed\n".utf8))
    }
}

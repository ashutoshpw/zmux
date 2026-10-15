import AppKit

/// A fresh-profile packaged journey. The close-phase receipt is deliberately
/// only preparation evidence; the external runner inspects disk after app exit.
@MainActor
enum WindowLifecycleSmoke {
    struct Prepared: Codable {
        let sourceSHA: String
        let namespace: UUID
        let window: UInt64
        let workspace: UInt64
        let surface: UInt64
        let oldPID: UInt32
        let noFocus: Bool
        let independentResize: Bool
        let nativeInput: Bool
        let nativeClose: Bool
        let emptyWindow: Bool
        let cliClose: Bool
    }

    static func run(coordinator: ApplicationCoordinator, report: URL, phase: String) async throws {
        try await wait(coordinator) { snapshot in
            snapshot.persistence?.startupPending != true && snapshot.persistence?.restoring != true &&
            snapshot.model.windows.count == 1 && coordinator.mainWindowHosts.count == 1 &&
            coordinator.mainWindowHost.nativeReady && snapshot.frames.first?.processId != nil && snapshot.frames.first?.snapshot != nil
        }
        let first = coordinator.mainWindowHost
        if phase == "reopen" {
            let saved = try JSONDecoder().decode(Prepared.self, from: Data(contentsOf: report))
            guard let snapshot = coordinator.snapshot,
                  snapshot.model.identityNamespace == saved.namespace,
                  snapshot.model.focusedWindow == saved.window,
                  snapshot.model.focusedWorkspace == saved.workspace,
                  let terminal = terminal(coordinator, window: saved.window),
                  terminal.surface == saved.surface, terminal.frame.processId != saved.oldPID,
                  terminal.view.window === first.window, first.window.isVisible else {
                throw BridgeFailure(message: "Last-close profile did not restore its exact live native owner")
            }
            try await typeMarker("ZMUX_WINDOW_REOPEN", into: first, coordinator: coordinator)
            let evidence: [String: Any] = [
                "success": true, "source_sha": saved.sourceSHA,
                "restored_namespace": saved.namespace.uuidString.lowercased(),
                "restored_window": saved.window, "restored_workspace": saved.workspace,
                "restored_surface": saved.surface, "fresh_process": true,
                "native_input": true
            ]
            try JSONSerialization.data(withJSONObject: evidence, options: [.prettyPrinted, .sortedKeys])
                .write(to: report.appendingPathExtension("reopened.json"), options: .atomic)
            return
        }
        guard phase == "close", let initial = coordinator.snapshot,
              let firstID = first.identity?.windowId,
              let original = terminal(coordinator, window: firstID), let pid = original.frame.processId else {
            throw BridgeFailure(message: "Window lifecycle fixture needs one initial terminal")
        }
        try await wait(coordinator) { _ in first.window.isKeyWindow && first.window.firstResponder === original.view }
        let responder = first.window.firstResponder
        let new = try await command(first, ["new-window", "--no-focus"])
        let secondID = try receipt(new, kind: "open")
        try await wait(coordinator) { snapshot in
            coordinator.mainWindowHosts.count == 2 && terminal(coordinator, window: secondID) != nil &&
            snapshot.model.focusedWindow == firstID && snapshot.observedKeyWindow == firstID
        }
        guard first.window.isKeyWindow, first.window.firstResponder === responder,
              terminal(coordinator, window: firstID)?.frame.processId == pid,
              let second = host(coordinator, secondID), second.window.isVisible,
              terminal(coordinator, window: secondID)?.view.window === second.window else {
            throw BridgeFailure(message: "No-focus creation changed the source responder/process or failed to mount both windows")
        }
        first.window.setContentSize(NSSize(width: 930, height: 620))
        second.window.setContentSize(NSSize(width: 750, height: 510))
        first.window.contentView?.layoutSubtreeIfNeeded()
        second.window.contentView?.layoutSubtreeIfNeeded()
        try await wait(coordinator) { _ in
            guard let a = terminal(coordinator, window: firstID), let b = terminal(coordinator, window: secondID),
                  let av = a.frame.snapshot?.viewport, let bv = b.frame.snapshot?.viewport else { return false }
            return a.view.requestedSize == (av.size.rows, av.size.cols) &&
                b.view.requestedSize == (bv.size.rows, bv.size.cols) && av.size.cols != bv.size.cols
        }
        let focus = try await command(first, ["focus-window", "--window", "window:\(secondID)"])
        guard try receipt(focus, kind: "focus") == secondID else { throw BridgeFailure(message: "Focus receipt targeted another window") }
        try await typeMarker("ZMUX_WINDOW_SECOND", into: second, coordinator: coordinator)
        guard terminal(coordinator, window: firstID)?.frame.snapshot?.viewport.contents.contains("ZMUX_WINDOW_SECOND") != true else {
            throw BridgeFailure(message: "Native key input crossed windows")
        }
        // The native delegate must use the same acknowledged close route as CLI.
        second.window.performClose(nil)
        try await wait(coordinator) { snapshot in
            snapshot.model.windows.count == 1 && host(coordinator, secondID) == nil &&
            !snapshot.windowHosts.contains(where: { $0.windowId == secondID })
        }
        guard terminal(coordinator, window: firstID)?.frame.processId == pid else {
            throw BridgeFailure(message: "Closing the sibling window replaced the original shell")
        }
        let emptyReceipt = try await command(first, ["new-window"])
        let emptyID = try receipt(emptyReceipt, kind: "open")
        try await wait(coordinator) { _ in terminal(coordinator, window: emptyID) != nil }
        guard let empty = host(coordinator, emptyID), let workspace = empty.snapshot?.workspace?.id else {
            throw BridgeFailure(message: "Focused window did not materialize its workspace")
        }
        _ = try await command(first, ["close-workspace", "--workspace", String(workspace)])
        try await wait(coordinator) { snapshot in
            snapshot.model.focusedWindow == emptyID && snapshot.model.focusedWorkspace == nil &&
            empty.snapshot?.workspace == nil && snapshot.frames.count == 1
        }
        let current = try await command(first, ["current-window", "--window", "window:\(emptyID)"])
        guard (current["window_id"] as? NSNumber)?.uint64Value == emptyID else {
            throw BridgeFailure(message: "Empty window root was not addressable")
        }
        // Exercise the native window-scoped action rather than ambient CLI focus.
        empty.newWorkspace()
        try await wait(coordinator) { _ in terminal(coordinator, window: emptyID) != nil }
        let closed = try await command(first, ["close-window", "--window", "window:\(emptyID)"])
        guard try receipt(closed, kind: "close") == emptyID else { throw BridgeFailure(message: "Close receipt targeted another window") }
        try await wait(coordinator) { snapshot in snapshot.model.windows.count == 1 && host(coordinator, emptyID) == nil }
        guard let final = coordinator.snapshot, let retained = terminal(coordinator, window: firstID),
              let finalWorkspace = final.model.windows.first?.selectedWorkspace else {
            throw BridgeFailure(message: "Last window lost its saveable terminal layout")
        }
        let prepared = Prepared(sourceSHA: Bundle.main.object(forInfoDictionaryKey: "ZmuxSourceSHA") as? String ?? "local",
            namespace: initial.model.identityNamespace, window: firstID, workspace: finalWorkspace,
            surface: retained.surface, oldPID: pid, noFocus: true, independentResize: true,
            nativeInput: true, nativeClose: true, emptyWindow: true, cliClose: true)
        try JSONEncoder().encode(prepared).write(to: report, options: .atomic)
        // Auxiliary native windows do not redefine the last-main-window policy.
        let auxiliary = NSWindow(contentRect: NSRect(x: 20, y: 20, width: 200, height: 100),
            styleMask: [.titled], backing: .buffered, defer: false)
        auxiliary.title = "Lifecycle fixture auxiliary window"
        auxiliary.orderFront(nil)
        first.window.performClose(nil)
        // Keep the auxiliary alive until the normal application termination/save.
        try await wait(coordinator) { _ in _ = auxiliary.isVisible; return false }
    }

    private static func host(_ coordinator: ApplicationCoordinator, _ id: UInt64) -> MainWindowHost? {
        coordinator.mainWindowHosts.values.first { $0.identity?.windowId == id }
    }
    private static func terminal(_ coordinator: ApplicationCoordinator, window id: UInt64) -> (surface: UInt64, view: TerminalView, frame: Frame)? {
        guard let snapshot = coordinator.snapshot,
              let window = snapshot.model.windows.first(where: { $0.id == id }),
              let workspace = snapshot.model.workspaces.first(where: { $0.id == window.selectedWorkspace }),
              let pane = workspace.root.panes.first(where: { $0.id == workspace.focusedPane }),
              let session = pane.sessionId, let view = coordinator.terminals[session],
              let frame = snapshot.frames.first(where: { $0.sessionId == session }), frame.snapshot != nil else { return nil }
        return (pane.selectedSurface, view, frame)
    }
    private static func command(_ host: MainWindowHost, _ arguments: [String]) async throws -> [String: Any] {
        let data = try await host.nativeCLI(arguments, timeout: 30000, externalCaller: true)
        guard let value = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw BridgeFailure(message: "Window CLI response was not an object")
        }
        return value
    }
    private static func receipt(_ value: [String: Any], kind: String) throws -> UInt64 {
        guard value["state"] as? String == "completed", value["kind"] as? String == kind,
              let operation = value["operation_id"] as? String, UUID(uuidString: operation) != nil,
              let epoch = value["epoch"] as? String, UUID(uuidString: epoch) != nil,
              let id = value["window_id"] as? NSNumber else {
            throw BridgeFailure(message: "Window CLI omitted an exact completed native receipt")
        }
        return id.uint64Value
    }
    private static func typeMarker(_ marker: String, into host: MainWindowHost, coordinator: ApplicationCoordinator) async throws {
        guard let id = host.identity?.windowId else { throw BridgeFailure(message: "Native input has no window identity") }
        host.window.makeKeyAndOrderFront(nil)
        NSApp.activate()
        try await wait(coordinator) { snapshot in
            guard let terminal = terminal(coordinator, window: id) else { return false }
            return host.window.isKeyWindow && host.window.firstResponder === terminal.view && snapshot.observedKeyWindow == id
        }
        for (characters, code) in [("printf '\(marker)\\n'", UInt16(0)), ("\r", UInt16(36))] {
            guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: host.window.windowNumber,
                context: nil, characters: characters, charactersIgnoringModifiers: characters,
                isARepeat: false, keyCode: code) else { throw BridgeFailure(message: "Could not create native key event") }
            host.window.sendEvent(event)
        }
        try await wait(coordinator) { _ in terminal(coordinator, window: id)?.frame.snapshot?.viewport.contents.lines.contains(marker) == true }
    }
    private static func wait(_ coordinator: ApplicationCoordinator, _ predicate: (DesktopSnapshot) -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(20)
        while ContinuousClock.now < deadline {
            if let snapshot = coordinator.snapshot, predicate(snapshot) { return }
            try await Task.sleep(for: .milliseconds(25))
        }
        throw BridgeFailure(message: "Packaged window lifecycle condition timed out")
    }
}

private extension String {
    var lines: [String] { components(separatedBy: .newlines) }
}

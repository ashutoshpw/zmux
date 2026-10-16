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
        let workspaceMove: Bool
        let ratioKey: String
        let ratio: Double
        let todoIDs: [UUID]
    }

    private static var traceLines = 0
    static func trace(_ message: String) {
        guard CommandLine.arguments.contains("--window-lifecycle-smoke"), traceLines < 256 else { return }
        traceLines += 1
        let text = "[window-lifecycle pid=\(ProcessInfo.processInfo.processIdentifier) t=\(ProcessInfo.processInfo.systemUptime)] " + String(message.prefix(256)) + "\n"
        let data = Data(text.utf8)
        FileHandle.standardError.write(data)
        guard let index = CommandLine.arguments.firstIndex(of: "--window-lifecycle-smoke"), CommandLine.arguments.count > index + 1 else { return }
        let url = URL(fileURLWithPath: CommandLine.arguments[index + 1]).appendingPathExtension("trace.log")
        do {
            if !FileManager.default.fileExists(atPath: url.path) {
                guard FileManager.default.createFile(atPath: url.path, contents: nil, attributes: [.posixPermissions: 0o600]) else { throw BridgeFailure(message: "Cannot create fixture trace") }
            }
            let file = try FileHandle(forWritingTo: url)
            defer { do { try file.close() } catch { FileHandle.standardError.write(Data("Window trace close failed: \(error)\n".utf8)) } }
            let offset = try file.seekToEnd()
            if offset < 65_536 { try file.write(contentsOf: data.prefix(65_536 - Int(offset))) }
        } catch { FileHandle.standardError.write(Data("Window trace write failed: \(error)\n".utf8)) }
    }

    static func run(coordinator: ApplicationCoordinator, report: URL, phase: String) async throws {
        trace("fixture run phase=" + phase)
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
            guard let split = splits(first.window.contentView).first(where: { $0.geometryKey == saved.ratioKey }),
                  abs(Double(split.fraction) - saved.ratio) < 0.01 else {
                throw BridgeFailure(message: "Workspace split ratio did not persist across app restart")
            }
            guard let metadata = snapshot.workspaceMetadata?.first(where: { $0.workspaceId == saved.workspace }),
                  metadata.todos.map(\.id) == saved.todoIDs, metadata.todos.count == 2,
                  metadata.todos[0].text == "Saved pending checklist item", metadata.todos[0].state == .pending, metadata.todos[0].origin == .user,
                  metadata.todos[1].text == "Saved completed checklist item", metadata.todos[1].state == .completed, metadata.todos[1].origin == .agent,
                  metadata.statuses.isEmpty, metadata.progress == nil, metadata.logs.isEmpty, metadata.pullRequest == nil else {
                throw BridgeFailure(message: "Fresh native process lost saved todo identity/order/state/origin or restored transient channels")
            }
            first.window.contentView?.layoutSubtreeIfNeeded()
            guard let row = rows(first.window.contentView).first,
                  row.accessibilityLabel()?.contains("2 todo items") == true,
                  row.subviews.compactMap({ $0 as? NSTextField }).contains(where: { !$0.isHidden && $0.stringValue.contains("1/2 todos completed") }) else {
                throw BridgeFailure(message: "Saved todo state did not render in the fresh native workspace row")
            }
            let evidence: [String: Any] = [
                "success": true, "source_sha": saved.sourceSHA,
                "restored_namespace": saved.namespace.uuidString.lowercased(),
                "restored_window": saved.window, "restored_workspace": saved.workspace,
                "restored_surface": saved.surface, "fresh_process": true,
                "native_input": true, "workspace_split_ratio_restored": true,
                "workspace_todos_restored": true, "transient_metadata_not_restored": true,
                "workspace_todo_row_rendered": true
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
        let movedRatio = try await validateMoves(coordinator, first: first, second: second, source: firstID, destination: secondID)
        // The native delegate must use the same acknowledged close route as CLI.
        second.window.performClose(nil)
        try await wait(coordinator) { snapshot in
            snapshot.model.windows.count == 1 && host(coordinator, secondID) == nil &&
            !snapshot.windowHosts.contains(where: { $0.windowId == secondID })
        }
        guard terminal(coordinator, window: firstID)?.frame.processId == pid else {
            throw BridgeFailure(message: "Closing the sibling window replaced the original shell")
        }
        try await typeMarker("ZMUX_MOVED_AFTER_HOST_CLOSE", into: first, coordinator: coordinator)
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
        let target = ["--workspace", String(finalWorkspace)]
        _ = try await command(first, ["todo", "add", "Saved pending checklist item"] + target)
        let added = try await command(first, ["todo", "add", "Saved completed checklist item", "--state", "completed", "--origin", "agent"] + target)
        guard let items = added["todos"] as? [[String: Any]], items.count == 2 else {
            throw BridgeFailure(message: "Close preparation did not create the exact saved checklist")
        }
        let todoIDs = items.compactMap { item in (item["id"] as? String).flatMap(UUID.init(uuidString:)) }
        guard todoIDs.count == 2, Set(todoIDs).count == 2,
              let owner = final.workspaceIdentities.first(where: { $0.workspaceId == finalWorkspace })?.uuid else {
            throw BridgeFailure(message: "Saved checklist omitted stable item/workspace identities")
        }
        _ = try await command(first, ["set-status", "temporary", "Do not restore this status"] + target)
        _ = try await command(first, ["set-progress", "0.5"] + target)
        _ = try await command(first, ["log", "Do not restore this log"] + target)
        try await WorkspacePullRequestSmoke.set(workspace: owner, number: 17, url: "https://github.com/acme/project/pull/17")
        var metadataDiagnostic = "metadata absent"
        do {
            try await wait(coordinator) { state in
                guard let metadata = state.workspaceMetadata?.first(where: { $0.workspaceId == finalWorkspace }) else {
                    metadataDiagnostic = "metadata absent"
                    return false
                }
                let todosMatch = metadata.todos.map(\.id) == todoIDs
                let moveMatches = metadata.statuses.contains { $0.key == "move" && $0.value == "retained" }
                let temporaryMatches = metadata.statuses.contains { $0.key == "temporary" && $0.value == "Do not restore this status" }
                let progressMatches = metadata.progress?.value == 0.5
                let pullRequestMatches = metadata.pullRequest?.number == 17
                metadataDiagnostic = "todosMatch=\(todosMatch) statusCount=\(metadata.statuses.count) moveMatches=\(moveMatches) temporaryMatches=\(temporaryMatches) progressMatches=\(progressMatches) logCount=\(metadata.logs.count) pullRequestMatches=\(pullRequestMatches)"
                return todosMatch && metadata.statuses.count == 2 && moveMatches && temporaryMatches && progressMatches && metadata.logs.count == 1 && pullRequestMatches
            }
        } catch {
            trace("close preparation metadata: " + metadataDiagnostic)
            throw error
        }
        let prepared = Prepared(sourceSHA: Bundle.main.object(forInfoDictionaryKey: "ZmuxSourceSHA") as? String ?? "local",
            namespace: initial.model.identityNamespace, window: firstID, workspace: finalWorkspace,
            surface: retained.surface, oldPID: pid, noFocus: true, independentResize: true,
            nativeInput: true, nativeClose: true, emptyWindow: true, cliClose: true,
            workspaceMove: true, ratioKey: movedRatio.0, ratio: movedRatio.1, todoIDs: todoIDs)
        try JSONEncoder().encode(prepared).write(to: report, options: .atomic)
        trace("preparation receipt written; requesting last-main-window close")
        // Auxiliary native windows do not redefine the last-main-window policy.
        let auxiliary = NSWindow(contentRect: NSRect(x: 20, y: 20, width: 200, height: 100),
            styleMask: [.titled], backing: .buffered, defer: false)
        auxiliary.title = "Lifecycle fixture auxiliary window"
        auxiliary.orderFront(nil)
        first.window.performClose(nil)
        // Keep the auxiliary alive until the normal application termination/save.
        try await wait(coordinator) { _ in _ = auxiliary.isVisible; return false }
    }

    private static func validateMoves(_ coordinator: ApplicationCoordinator, first: MainWindowHost, second: MainWindowHost, source: UInt64, destination: UInt64) async throws -> (String, Double) {
        guard let snapshot = coordinator.snapshot,
              let workspace = snapshot.model.workspaces.first(where: { $0.id == snapshot.model.windows.first(where: { $0.id == source })?.selectedWorkspace }),
              let original = terminal(coordinator, window: source) else { throw BridgeFailure(message: "Move fixture has no source workspace") }
        _ = try await command(first, ["split-pane", "--pane", String(workspace.focusedPane), "horizontal"])
        try await wait(coordinator) { _ in first.snapshot?.workspace?.root.panes.count == 2 }
        guard let sibling = first.snapshot?.workspace?.root.panes.first(where: { $0.id != workspace.focusedPane }) else { throw BridgeFailure(message: "Move fixture split did not materialize") }
        let created = try await command(first, ["browser", "open", "--pane", String(sibling.id), "about:blank"])
        guard let surfaces = created["created_surfaces"] as? [[String: Any]], let browserID = surfaces.first?["surface_id"] as? NSNumber else { throw BridgeFailure(message: "Move fixture browser omitted its surface") }
        try await wait(coordinator) { state in
            coordinator.browsers[browserID.uint64Value]?.webView.isLoading == false &&
            state.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.contains { $0.id == browserID.uint64Value && $0.title == "New tab · zmux" }
        }
        guard let browser = coordinator.browsers[browserID.uint64Value] else { throw BridgeFailure(message: "Move fixture browser missing") }
        _ = try await command(first, ["focus-pane", "--pane", String(workspace.focusedPane)])
        try await wait(coordinator) { state in state.observedKeyWindow == source && original.view.window === first.window && browser.window === first.window }
        guard let split = splits(first.window.contentView).first else { throw BridgeFailure(message: "Move fixture has no native split") }
        split.setFraction(0.63)
        let ratioKey = split.geometryKey
        let hostID = browser.hostID
        let generation = browser.documentGeneration
        let dataStore = browser.webView.configuration.websiteDataStore
        let framePID = original.frame.processId
        _ = try await command(first, ["set-status", "move", "retained", "--workspace", String(workspace.id)])
        // The original WebKit document starts a delayed operation before moving.
        // Its observed title is an independent dispatch barrier through Rust.
        let pending = Task { try await first.nativeCLI(["browser", "eval", "document.title='ZMUX_MOVE_PENDING'; new Promise(resolve=>setTimeout(()=>{globalThis.zmuxMoveMarker='same document';resolve('MOVE_OPERATION_SURVIVED')},5000))", "--surface", String(browserID.uint64Value)], timeout: 30000, externalCaller: true) }
        try await wait(coordinator) { state in state.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.contains { $0.id == browserID.uint64Value && $0.title == "ZMUX_MOVE_PENDING" } }
        let destinationSelection = coordinator.snapshot?.model.windows.first(where: { $0.id == destination })?.selectedWorkspace
        let moved = try await command(first, ["move-workspace-to-window", "--workspace", String(workspace.id), "--window", "window:\(destination)", "--no-focus"])
        guard moved["state"] as? String == "topology_committed", moved["native_focus"] is NSNull else { throw BridgeFailure(message: "No-focus move did not distinguish topology commitment") }
        try await wait(coordinator) { state in
            state.model.windows.first(where: { $0.id == source })?.selectedWorkspace == nil &&
            state.model.windows.first(where: { $0.id == destination })?.selectedWorkspace == destinationSelection &&
            state.observedKeyWindow == source && first.snapshot?.workspace == nil
        }
        guard first.window.isKeyWindow, original.view.window == nil, browser.window == nil,
              coordinator.terminals[original.frame.sessionId] === original.view,
              coordinator.browsers[browserID.uint64Value] === browser else {
            throw BridgeFailure(message: "No-focus move changed key window or recreated a live view")
        }
        // Notification activation must resolve the workspace's current owner.
        _ = try await command(first, ["notify", "--workspace", String(workspace.id), "--title", "ZMUX_MOVED_TARGET", "--desktop", "false"])
        let listed = try await command(first, ["list-notifications"])
        guard let rows = listed["notifications"] as? [[String: Any]], let row = rows.first(where: { $0["title"] as? String == "ZMUX_MOVED_TARGET" }), let notification = row["id"] as? NSNumber else {
            throw BridgeFailure(message: "Move fixture notification did not retain workspace attribution")
        }
        _ = try await command(first, ["open-notification", "--id", notification.stringValue])
        try await wait(coordinator) { state in state.observedKeyWindow == destination && original.view.window === second.window && browser.window === second.window }
        guard let movedSplit = splits(second.window.contentView).first(where: { $0.geometryKey == ratioKey }), abs(movedSplit.fraction - 0.63) < 0.01,
              browser.hostID == hostID, browser.documentGeneration == generation,
              browser.webView.configuration.websiteDataStore === dataStore,
              coordinator.snapshot?.frames.first(where: { $0.sessionId == original.frame.sessionId })?.processId == framePID else {
            throw BridgeFailure(message: "Reparenting lost terminal/browser identity, document, storage or workspace geometry")
        }
        let pendingData = try await pending.value
        let pendingValue = try JSONSerialization.jsonObject(with: pendingData) as? [String: Any]
        guard pendingValue?["value"] as? String == "MOVE_OPERATION_SURVIVED" else { throw BridgeFailure(message: "Pending browser operation did not survive workspace placement") }
        // Native and CLI enter the same runtime action. Move back into the empty
        // source, then close the previous host in the caller to expose stale binds.
        guard let workspaceUUID = coordinator.snapshot?.workspaceIdentities.first(where: { $0.workspaceId == workspace.id })?.uuid,
              let destinationUUID = first.windowUUID else { throw BridgeFailure(message: "Native move omitted stable routing identities") }
        coordinator.enqueue(.workspaceMove(workspaceUUID, destinationUUID, true))
        try await wait(coordinator) { state in state.observedKeyWindow == source && original.view.window === first.window && browser.window === first.window }
        try await typeMarker("ZMUX_WORKSPACE_MOVED_LIVE", into: first, coordinator: coordinator)
        let metadata = try await command(first, ["list-status", "--workspace", String(workspace.id)])
        guard let statuses = metadata["statuses"] as? [[String: Any]], statuses.contains(where: { $0["key"] as? String == "move" && $0["value"] as? String == "retained" }) else {
            throw BridgeFailure(message: "Workspace metadata did not follow stable ownership")
        }
        return (ratioKey, 0.63)
    }

    private static func rows(_ view: NSView?) -> [WorkspaceRowView] {
        guard let view else { return [] }
        return ((view as? WorkspaceRowView).map { [$0] } ?? []) + view.subviews.flatMap { rows($0) }
    }
    private static func splits(_ view: NSView?) -> [PaneSplitView] {
        guard let view else { return [] }
        return ((view as? PaneSplitView).map { [$0] } ?? []) + view.subviews.flatMap { splits($0) }
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
        trace("CLI begin " + (arguments.first ?? "empty"))
        let data = try await host.nativeCLI(arguments, timeout: 30000, externalCaller: true)
        trace("CLI completed " + (arguments.first ?? "empty"))
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
    /// Public socket reads distinguish a stale native polling loop from the
    /// current executor state. Each CLI invocation has its own one-second budget
    /// and runs through nativeCLI's detached subprocess, never blocking AppKit.
    private static func captureTimeoutStatus(_ coordinator: ApplicationCoordinator) async {
        guard let socket = ProcessInfo.processInfo.environment["ZMUX_SOCKET"], socket.hasPrefix("/"),
              let index = CommandLine.arguments.firstIndex(of: "--window-lifecycle-smoke"), CommandLine.arguments.count > index + 1 else {
            trace("timeout public status unavailable: no exact fixture socket/report")
            return
        }
        var evidence: [String: Any] = [:]
        for (name, arguments) in [("status", ["status"]), ("session", ["session-status"]), ("profiles", ["browser", "profiles", "list"])] {
            do {
                let data = try await coordinator.mainWindowHost.nativeCLI(["--socket", socket] + arguments, timeout: 1000, externalCaller: true)
                guard data.count <= 32_768, let value = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                    throw BridgeFailure(message: "Timeout status response is oversized or not a JSON object")
                }
                evidence[name] = value
                if name == "status" {
                    trace("timeout public status publication=\(value["revision"] ?? "missing") modelRevision=\(value["model_revision"] ?? "missing") windows=\(value["window_count"] ?? "missing") pendingLaunches=\(value["pending_launches"] ?? "missing") closed=\(value["closed"] ?? "missing")")
                }
            } catch {
                let message = String(error.localizedDescription.prefix(512))
                evidence[name] = ["error": message]
                trace("timeout public " + name + " failed: " + message)
            }
        }
        do {
            let data = try JSONSerialization.data(withJSONObject: evidence, options: [.sortedKeys])
            guard data.count <= 65_536 else { throw BridgeFailure(message: "Timeout diagnostics exceeded 64 KiB") }
            let report = URL(fileURLWithPath: CommandLine.arguments[index + 1]).appendingPathExtension("timeout-diagnostics.json")
            try data.write(to: report, options: .atomic)
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: report.path)
        } catch { trace("timeout diagnostics write failed: " + error.localizedDescription) }
    }
    private static func wait(_ coordinator: ApplicationCoordinator, line: UInt = #line, _ predicate: (DesktopSnapshot) -> Bool) async throws {
        trace("wait begin line=\(line)")
        let deadline = ContinuousClock.now + .seconds(20)
        while ContinuousClock.now < deadline {
            if let snapshot = coordinator.snapshot, predicate(snapshot) { trace("wait complete line=\(line)"); return }
            try await Task.sleep(for: .milliseconds(25))
        }
        let state = coordinator.snapshot.map { "publication=\($0.revision) modelRevision=\($0.model.revision) windows=\($0.model.windows.count) focused=\(String(describing: $0.model.focusedWindow)) observed=\(String(describing: $0.observedKeyWindow)) hosts=\(coordinator.mainWindowHosts.count) requests=\($0.windowRequests.count)" } ?? "no snapshot"
        trace("wait timeout line=\(line) " + state)
        if let snapshot = coordinator.snapshot {
            trace("timeout persistence startup=\(String(describing: snapshot.persistence?.startupPending)) restoring=\(String(describing: snapshot.persistence?.restoring)) autosave=\(String(describing: snapshot.persistence?.autosaveEnabled)) saved=\(String(describing: snapshot.persistence?.lastSavedRevision))")
            trace("timeout persistence error=" + (snapshot.persistence?.error ?? "none"))
            trace("timeout profiles loading=\(snapshot.browserProfiles.loading) enabled=\(snapshot.browserProfiles.enabled) count=\(snapshot.browserProfiles.profiles.count) error=" + (snapshot.browserProfiles.error ?? "none"))
            trace("timeout lastError=" + (snapshot.lastError ?? "none") + " cleanupError=" + (snapshot.cleanupError ?? "none"))
        }
        await captureTimeoutStatus(coordinator)
        throw BridgeFailure(message: "Packaged window lifecycle condition at line \(line) timed out: " + state)
    }
}

private extension String {
    var lines: [String] { components(separatedBy: .newlines) }
}

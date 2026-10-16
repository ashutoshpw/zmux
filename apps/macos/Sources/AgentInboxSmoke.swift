import AppKit

/// Actual menu/presentation boundary, real CLI journal and independently observed PTYs.
/// No external Accessibility permission or notification reply injection is involved.
@MainActor
enum AgentInboxSmoke {
    private struct Child: Codable, Sendable {
        let pid: UInt32
        let parentPID: Int32
        let nonce: String
        let workspace: UUID
        let surface: UUID
        let inputBytes: UInt64
        let sequence: UInt64
    }
    private struct Evidence: Encodable, Sendable {
        let scope = "native_agent_inbox_menu_and_durable_messages"
        let sourceSHA: String
        let appPID: Int32
        let nonce: String
        var state = "starting"
        var stage = "prerequisites"
        var sourceWindow: UInt64?
        var otherWindow: UInt64?
        var activationPID: Int32?
        var messageIDs: [UUID] = []
        var checks: [String] = []
        var children: [Child] = []
        var failure: String?
    }
    private struct Message: Decodable {
        struct Address: Decodable { let surface_id: UUID }
        struct Sender: Decodable { let surface_id: UUID? }
        let id: UUID
        let body: String
        let state: String
        let failure: String?
        let sender: Sender
        let recipient: Address
    }
    private struct Sent: Decodable { let durability: String; let message: Message }
    private struct Inbox: Decodable { let messages: [Message] }

    static func run(coordinator: ApplicationCoordinator, report: URL) async throws {
        guard let rootPath = ProcessInfo.processInfo.environment["ZMUX_AGENT_INBOX_ROOT"],
              let nonce = ProcessInfo.processInfo.environment["ZMUX_AGENT_INBOX_NONCE"], UUID(uuidString: nonce) != nil else {
            throw failure("Private fixture directory and nonce unavailable")
        }
        let root = URL(fileURLWithPath: rootPath, isDirectory: true)
        var evidence = Evidence(sourceSHA: Bundle.main.object(forInfoDictionaryKey: "ZmuxSourceSHA") as? String ?? "",
                                appPID: ProcessInfo.processInfo.processIdentifier, nonce: nonce)
        var children: [Child] = []
        var livePIDs: Set<UInt32> = []
        do {
            try await wait(coordinator, "initial real terminal") { state in
                state.persistence?.startupPending != true && state.persistence?.restoring != true &&
                state.model.windows.count == 1 && coordinator.mainWindowHosts.count == 1 &&
                coordinator.mainWindowHosts.values.allSatisfy(\.nativeReady) &&
                state.frames.contains { $0.processId != nil && $0.snapshot?.viewport.contents.contains("AGENT_INBOX_READY_" + nonce) == true }
            }
            guard let source = coordinator.mainWindowHosts.values.first, let sourceID = source.identity?.windowId,
                  let sourcePID = terminalPID(coordinator, window: sourceID) else { throw failure("Source terminal owner missing") }
            let sourceChild = try await child(root, pid: sourcePID, nonce: nonce)
            children.append(sourceChild)
            livePIDs.insert(sourcePID)
            let new = try await command(source, ["new-window", "--no-focus"])
            guard new["state"] as? String == "completed", let otherID = (new["window_id"] as? NSNumber)?.uint64Value,
                  otherID != sourceID else { throw failure("Real new-window receipt missing") }
            try await wait(coordinator, "second mounted real terminal") { _ in
                coordinator.mainWindowHosts.count == 2 && terminalPID(coordinator, window: otherID) != nil &&
                coordinator.mainWindowHosts.values.allSatisfy(\.nativeReady)
            }
            guard let other = coordinator.mainWindowHosts.values.first(where: { $0.identity?.windowId == otherID }),
                  let otherPID = terminalPID(coordinator, window: otherID), otherPID != sourcePID else {
                throw failure("Second window substituted the first PTY")
            }
            let otherChild = try await child(root, pid: otherPID, nonce: nonce)
            children.append(otherChild)
            livePIDs.insert(otherPID)
            evidence.sourceWindow = sourceID; evidence.otherWindow = otherID
            let body = "  Native inbox α界 e\u{301}🙂\t\nsecond line  \n"
            let primary = try await send(source, from: otherChild, to: sourceChild.surface, body: body)
            let sibling = try await send(source, from: sourceChild, to: otherChild.surface, body: "Other window retained message")

            // A third, temporary terminal gives the installation list an unavailable recipient.
            let temporary = try await command(source, ["new-workspace", "Closed inbox recipient"])
            guard let temporarySurface = temporary["focused_surface_uuid"] as? String,
                  let closedUUID = UUID(uuidString: temporarySurface),
                  let numericSurface = (temporary["focused_surface"] as? NSNumber)?.uint64Value else {
                throw failure("Temporary terminal identity missing")
            }
            try await wait(coordinator, "temporary recipient PTY") { state in
                state.model.workspaces.flatMap { $0.root.panes }.flatMap(\.surfaces).contains { surface in
                    surface.id == numericSurface && state.frames.contains { frame in
                        frame.sessionId == surface.sessionId && frame.processId != nil &&
                        frame.snapshot?.viewport.contents.contains("AGENT_INBOX_READY_" + nonce) == true
                    }
                }
            }
            guard let state = coordinator.snapshot,
                  let terminal = state.model.workspaces.flatMap({ $0.root.panes }).flatMap(\.surfaces).first(where: { $0.id == numericSurface }),
                  let temporaryPID = state.frames.first(where: { $0.sessionId == terminal.sessionId })?.processId else {
                throw failure("Temporary recipient did not publish a real process")
            }
            children.append(try await child(root, pid: temporaryPID, nonce: nonce))
            let closed = try await send(source, from: otherChild, to: closedUUID, body: "Closed recipient retained message")
            _ = try await command(source, ["close-surface", temporarySurface])
            try await wait(coordinator, "closed recipient removal") { state in
                !state.model.workspaces.flatMap { $0.root.panes }.flatMap(\.surfaces).contains { $0.id == numericSurface }
            }
            let listed = try JSONDecoder().decode(Inbox.self, from: await cli(source, ["agent", "inbox"]))
            guard listed.messages.count == 3,
                  listed.messages.contains(where: { $0.id == primary.id && $0.state == "queued" && $0.body == body &&
                      $0.sender.surface_id == otherChild.surface && $0.recipient.surface_id == sourceChild.surface }),
                  listed.messages.contains(where: { $0.id == sibling.id && $0.state == "queued" && $0.recipient.surface_id == otherChild.surface }),
                  listed.messages.contains(where: { $0.id == closed.id && $0.state == "failed" && $0.failure == "recipient_closed" && $0.recipient.surface_id == closedUUID }) else {
                throw failure("Bundled CLI did not retain exact queued/global/closed-message prerequisites")
            }
            evidence.messageIDs = [primary.id, sibling.id, closed.id]
            evidence.checks.append("two_real_window_ptys_and_exact_cli_queued_global_closed_records")
            evidence.children = try await counters(root, children: children, nonce: nonce, livePIDs: livePIDs)
            evidence.checks.append("independent_pty_input_counters_zero")

            // Capture the source's actual public menu, then install/focus the sibling.
            // Invoking a retained NSMenu action must retain its explicit source target.
            source.installMenus()
            let sourceMenu = NSApp.mainMenu
            evidence.activationPID = try await activateOwnedApplication()
            guard evidence.activationPID == evidence.appPID else { throw failure("Activation substituted another process") }
            other.installMenus(); other.window.makeKeyAndOrderFront(nil); NSApp.activate()
            try await wait(coordinator, "other window really key") { state in
                NSApp.keyWindow === other.window && other.window.isKeyWindow &&
                state.observedKeyWindow == otherID && state.model.focusedWindow == otherID
            }
            evidence.checks.append("other_window_key_before_explicit_source_menu")
            evidence.stage = "entrypoint"
            try await save(evidence, to: report)
            guard let (menu, index) = entrypoint(sourceMenu), let action = menu.item(at: index)?.action,
                  menu.item(at: index)?.target === source, source.responds(to: action) else {
                throw failure("Agent Inbox entrypoint unavailable")
            }
            menu.performActionForItem(at: index)
            try await wait(coordinator, "mounted inbox owned by invoking source") { _ in
                NSApp.windows.contains { $0.parent === source.window && $0.isVisible && $0.title == "Agent Inbox" }
            }
            guard let inbox = NSApp.windows.first(where: { $0.parent === source.window && $0.isVisible && $0.title == "Agent Inbox" }),
                  let content = inbox.contentView else { throw failure("Inbox attached to another window") }
            try await wait(coordinator, "all installation message rows") { _ in
                let ids = Set(descendants(content).compactMap { $0.accessibilityIdentifier() })
                return evidence.messageIDs.allSatisfy { ids.contains($0.uuidString.lowercased()) }
            }
            guard let row = descendants(content).compactMap({ $0 as? NSButton }).first(where: {
                $0.accessibilityIdentifier() == primary.id.uuidString.lowercased()
            }) else { throw failure("Immutable primary message row unavailable") }
            row.performClick(nil)
            try await wait(coordinator, "selected message body and reply responder") { _ in
                let views = descendants(content)
                return views.contains { ($0 as? NSTextView)?.string == body || ($0 as? NSTextField)?.stringValue == body } &&
                    views.compactMap { $0 as? NSTextView }.contains { $0.isEditable && $0.accessibilityLabel() == "Reply" && inbox.firstResponder === $0 }
            }
            let rendered = descendants(content)
            guard rendered.contains(where: { ($0 as? NSTextView)?.string == body || ($0 as? NSTextField)?.stringValue == body }),
                  let reply = rendered.compactMap({ $0 as? NSTextView }).first(where: { $0.isEditable && $0.accessibilityLabel() == "Reply" }),
                  inbox.firstResponder === reply else { throw failure("Inbox body or actual reply first responder differs") }
            evidence.checks.append("actual_source_owned_inbox_exact_body_and_reply_responder")
            evidence.children = try await counters(root, children: children, nonce: nonce, livePIDs: livePIDs)
            try await coordinator.shutdown()
            evidence.stage = "complete"; evidence.state = "complete"
            try await save(evidence, to: report)
        } catch {
            evidence.state = "failed"; evidence.failure = error.localizedDescription
            do { evidence.children = try await counters(root, children: children, nonce: nonce, livePIDs: livePIDs) }
            catch { evidence.failure = (evidence.failure ?? "") + "; counter verification: " + error.localizedDescription }
            try await save(evidence, to: report)
            throw error
        }
    }

    private static func terminalPID(_ coordinator: ApplicationCoordinator, window: UInt64) -> UInt32? {
        guard let state = coordinator.snapshot, let owner = state.model.windows.first(where: { $0.id == window }),
              let workspace = state.model.workspaces.first(where: { $0.id == owner.selectedWorkspace }),
              let pane = workspace.root.panes.first(where: { $0.id == workspace.focusedPane }),
              let session = pane.sessionId, let view = coordinator.terminals[session],
              let host = coordinator.mainWindowHosts.values.first(where: { $0.identity?.windowId == window }),
              view.window === host.window,
              let frame = state.frames.first(where: { $0.sessionId == session }),
              frame.snapshot?.viewport.contents.contains("AGENT_INBOX_READY_") == true else { return nil }
        return frame.processId
    }
    private static func cli(_ host: MainWindowHost, _ arguments: [String], caller: Child? = nil) async throws -> Data {
        if caller == nil { return try await host.nativeCLI(arguments, timeout: 5000, externalCaller: true) }
        let executable = Bundle.main.resourceURL!.appendingPathComponent("bin/zmux")
        return try await Task.detached {
            let process = Process()
            process.executableURL = executable; process.arguments = ["--timeout", "5000"] + arguments
            var environment = ProcessInfo.processInfo.environment
            for key in ["ZMUX_WINDOW_ID", "ZMUX_WORKSPACE_ID", "ZMUX_SURFACE_ID", "ZMUX_TAB_ID"] { environment.removeValue(forKey: key) }
            environment["ZMUX_WORKSPACE_ID"] = caller?.workspace.uuidString
            environment["ZMUX_SURFACE_ID"] = caller?.surface.uuidString
            process.environment = environment
            let pipe = Pipe(); process.standardOutput = pipe; process.standardError = pipe
            try process.run()
            let data = pipe.fileHandleForReading.readDataToEndOfFile(); process.waitUntilExit()
            guard process.terminationStatus == 0, data.count <= 131_072 else { throw failure("Exact-attribution CLI failed: " + String(decoding: data.prefix(2048), as: UTF8.self)) }
            return data
        }.value
    }
    private static func command(_ host: MainWindowHost, _ arguments: [String]) async throws -> [String: Any] {
        guard let value = try JSONSerialization.jsonObject(with: await cli(host, arguments)) as? [String: Any] else { throw failure("CLI response is not an object") }
        return value
    }
    private static func send(_ host: MainWindowHost, from sender: Child, to recipient: UUID, body: String) async throws -> Message {
        let sent = try JSONDecoder().decode(Sent.self, from: await cli(host, ["agent", "message", recipient.uuidString, "--", body], caller: sender))
        guard sent.durability == "saved", sent.message.state == "queued", sent.message.body == body,
              sent.message.sender.surface_id == sender.surface, sent.message.recipient.surface_id == recipient else { throw failure("CLI send lost exact attribution/body/durability") }
        return sent.message
    }
    private static func child(_ root: URL, pid: UInt32, nonce: String) async throws -> Child {
        let deadline = ContinuousClock.now + .seconds(5)
        while ContinuousClock.now < deadline {
            if let value = try await Task.detached(operation: { () throws -> Child? in
                let path = root.appendingPathComponent("child-\(pid).json")
                guard FileManager.default.fileExists(atPath: path.path) else { return nil }
                let data = try Data(contentsOf: path)
                guard data.count <= 2048 else { throw failure("Child counter exceeds bound") }
                return try JSONDecoder().decode(Child.self, from: data)
            }).value {
                guard value.pid == pid, value.parentPID == ProcessInfo.processInfo.processIdentifier, value.nonce == nonce else { throw failure("Independent child identity differs") }
                return value
            }
            try await Task.sleep(for: .milliseconds(32))
        }
        throw failure("Independent child counter unavailable")
    }
    private static func counters(_ root: URL, children: [Child], nonce: String, livePIDs: Set<UInt32>) async throws -> [Child] {
        var values: [Child] = []
        for original in children {
            var value = try await child(root, pid: original.pid, nonce: nonce)
            if livePIDs.contains(original.pid) {
                // Two independent select cycles after observing the action boundary:
                // no terminal bytes are used to wake or synchronize the child.
                let barrier = value.sequence
                let deadline = ContinuousClock.now + .seconds(3)
                while value.sequence < barrier + 2 && ContinuousClock.now < deadline {
                    try await Task.sleep(for: .milliseconds(32))
                    value = try await child(root, pid: original.pid, nonce: nonce)
                }
                guard value.sequence >= barrier + 2 else { throw failure("Independent PTY child did not progress after UI action") }
            }
            guard value.workspace == original.workspace, value.surface == original.surface, value.inputBytes == 0 else { throw failure("Inbox action wrote PTY input or substituted a terminal") }
            values.append(value)
        }
        return values
    }
    private static func entrypoint(_ menu: NSMenu?) -> (NSMenu, Int)? {
        guard let menu else { return nil }
        for (index, item) in menu.items.enumerated() {
            if item.title == "Agent Inbox" || item.title == "Agent Inbox…" { return (menu, index) }
            if let found = entrypoint(item.submenu) { return found }
        }
        return nil
    }
    private static func descendants(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap { descendants($0) } }
    private static func wait(_ coordinator: ApplicationCoordinator, _ description: String, until predicate: (DesktopSnapshot) -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(8)
        while ContinuousClock.now < deadline {
            if let state = coordinator.snapshot, predicate(state) { return }
            try await Task.sleep(for: .milliseconds(32))
        }
        throw failure("Timed out awaiting " + description)
    }
    private static func activateOwnedApplication() async throws -> Int32 {
        let current = NSRunningApplication.current
        let bundle = Bundle.main.bundleURL.standardizedFileURL
        guard current.bundleURL?.standardizedFileURL == bundle, let identifier = current.bundleIdentifier,
              NSWorkspace.shared.runningApplications.filter({ $0.bundleIdentifier == identifier }).map(\.processIdentifier) == [current.processIdentifier] else {
            throw failure("LaunchServices did not identify the exact unique fixture app")
        }
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true; configuration.createsNewApplicationInstance = false
        configuration.allowsRunningApplicationSubstitution = false
        configuration.arguments = ["--cli", "--help"]
        return try await withCheckedThrowingContinuation { continuation in
            NSWorkspace.shared.openApplication(at: bundle, configuration: configuration) { @Sendable application, error in
                if let error { continuation.resume(throwing: error) }
                else if let application { continuation.resume(returning: application.processIdentifier) }
                else { continuation.resume(throwing: failure("Owned activation returned no process")) }
            }
        }
    }
    private static func save(_ evidence: Evidence, to report: URL) async throws {
        try await Task.detached {
            let data = try JSONEncoder().encode(evidence)
            guard data.count <= 65_536 else { throw failure("Native report exceeds bound") }
            try data.write(to: report, options: .atomic)
        }.value
    }
    nonisolated private static func failure(_ text: String) -> BridgeFailure { BridgeFailure(message: "Native agent inbox: " + text) }
}

import AppKit

/// Direct native event-handler proof through the real coordinator, bridge and
/// explicitly selected worker. This is not physical keyboard or full IME proof.
@MainActor
enum TerminalEngineSmoke {
    static func run(coordinator: ApplicationCoordinator, report: URL) async throws {
        guard let rootPath = ProcessInfo.processInfo.environment["ZMUX_ENGINE_SMOKE_ROOT"] else {
            throw BridgeFailure(message: "Terminal engine smoke requires its owned fixture root")
        }
        let root = URL(fileURLWithPath: rootPath, isDirectory: true)
        try await wait(coordinator) { snapshot in
            snapshot.frames.contains { $0.snapshot?.viewport.contents.contains("NATIVE_ENGINE_READY") == true }
        }
        guard let snapshot = coordinator.snapshot,
              let frame = snapshot.frames.first(where: { $0.snapshot?.viewport.contents.contains("NATIVE_ENGINE_READY") == true }),
              let pane = snapshot.model.workspaces.flatMap({ $0.root.panes }).first(where: { $0.sessionId == frame.sessionId }),
              frame.snapshot?.viewport.applicationCursor == true,
              frame.snapshot?.viewport.bracketedPaste == true else {
            throw BridgeFailure(message: "Explicit native terminal did not publish the child's actual modes")
        }
        let view = coordinator.terminal(session: frame.sessionId, pane: pane)
        guard view.onKey != nil, view.onPaste != nil, view.onInput != nil else {
            throw BridgeFailure(message: "Native presenter did not install the production terminal input routes")
        }
        for repeated in [false, true] {
            guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0,
                                               windowNumber: view.window?.windowNumber ?? 0, context: nil,
                                               characters: "\u{f700}", charactersIgnoringModifiers: "\u{f700}",
                                               isARepeat: repeated, keyCode: 126) else {
                throw BridgeFailure(message: "Cannot construct the owned native special-key event")
            }
            view.keyDown(with: event)
        }
        guard let release = NSEvent.keyEvent(with: .keyUp, location: .zero, modifierFlags: [], timestamp: 0,
                                             windowNumber: view.window?.windowNumber ?? 0, context: nil,
                                             characters: "\u{f700}", charactersIgnoringModifiers: "\u{f700}",
                                             isARepeat: false, keyCode: 126) else {
            throw BridgeFailure(message: "Cannot construct the owned native release event")
        }
        view.keyUp(with: release)
        let pasteboard = NSPasteboard(name: .init("app.w3dev.zmux.engine-smoke.\(UUID().uuidString)"))
        defer { pasteboard.clearContents() }
        // Exactly16KiB, forcing a larger JSON envelope while preserving one paste.
        let text = String(repeating: "N", count: 16380) + "\r\n\u{1b}Q"
        guard pasteboard.setString(text, forType: .string), view.paste(from: pasteboard) else {
            throw BridgeFailure(message: "Native semantic clipboard admission failed")
        }
        view.insertText("END", replacementRange: NSRange(location: NSNotFound, length: 0))
        try await wait(coordinator) { snapshot in
            snapshot.frames.first(where: { $0.sessionId == frame.sessionId })?.snapshot?.viewport.contents.contains("NATIVE_ENGINE_DONE") == true
        }
        let childData = try Data(contentsOf: root.appendingPathComponent("child-observation.json"))
        guard let child = try JSONSerialization.jsonObject(with: childData) as? [String: Any],
              child["success"] as? Bool == true,
              (child["pid"] as? NSNumber)?.uint32Value == frame.processId else {
            throw BridgeFailure(message: "Native fixture did not retain its exact Session child observation")
        }
        let receipt: [String: Any] = [
            "success": true, "source_sha": Bundle.main.object(forInfoDictionaryKey: "ZmuxSourceSHA") as? String ?? "",
            "engine": "explicit_ghostty", "native_direct_key_handlers": true,
            "native_clipboard_worker_route": true, "composed_text_once": true,
            "physical_keyboard_delivery": false, "full_keyboard_ime_fidelity": false,
            "child_observation": child
        ]
        try JSONSerialization.data(withJSONObject: receipt, options: [.prettyPrinted, .sortedKeys]).write(to: report, options: .atomic)
    }

    private static func wait(_ coordinator: ApplicationCoordinator, predicate: (DesktopSnapshot) -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(8)
        while ContinuousClock.now < deadline {
            if let snapshot = coordinator.snapshot {
                if predicate(snapshot) { return }
                if let error = snapshot.lastError { throw BridgeFailure(message: error) }
                if let failure = snapshot.frames.compactMap({ $0.failure ?? $0.snapshot?.failure }).first {
                    throw BridgeFailure(message: failure)
                }
            }
            try await Task.sleep(for: .milliseconds(10))
        }
        throw BridgeFailure(message: "Native terminal engine fixture did not reach its bounded child observation")
    }
}

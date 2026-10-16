import AppKit
import Darwin

private struct SelectionWordReference: Codable, Sendable {
    let text: String
    let clickedUTF16: Int
    let location: Int
    let length: Int
    let selected: String
}

private struct SelectionNativeCase: Codable, Sendable {
    let name: String
    let expected: String
    let observed: String?
}

private struct SelectionNativeReport: Codable, Sendable {
    let schemaVersion: Int
    let sourceSHA: String
    let appPID: Int32
    let childPID: UInt32
    let sessionID: UInt64
    let surfaceID: UInt64
    let nonce: String
    let applicationActiveBefore: Bool
    let windowVisibleBefore: Bool
    let keyWindowBefore: Bool
    let applicationActive: Bool
    let windowVisible: Bool
    let keyWindow: Bool
    let terminalFirstResponder: Bool
    let activationRequestProcessID: Int32?
    let activationFailure: String?
    var state: String
    var publicWordReferences: [SelectionWordReference]
    var cases: [SelectionNativeCase]
}

/// Real mounted TerminalView events, system word policy and menu Copy. No fake
/// view or injected selection is used; physical mouse/VoiceOver remain separate.
@MainActor
enum TerminalSelectionSmoke {
    static func run(coordinator: ApplicationCoordinator, report: URL) async throws {
        guard let nonce = ProcessInfo.processInfo.environment["ZMUX_SELECTION_NONCE"],
              UUID(uuidString: nonce) != nil,
              ProcessInfo.processInfo.environment["ZMUX_TERMINAL_ENGINE"] == nil else {
            throw failure("Selection fixture requires its private nonce and default engine")
        }
        let deadline = ContinuousClock.now + .seconds(20)
        var captured: (DesktopSnapshot, Frame, Pane)?
        while ContinuousClock.now < deadline {
            if let value = coordinator.snapshot,
               let frame = value.frames.first(where: { $0.snapshot?.viewport.contents.contains("NATIVE_SELECTION_READY_" + nonce) == true }),
               let pane = value.workspace?.root.panes.first(where: { $0.sessionId == frame.sessionId }),
               frame.snapshot?.state == "running", frame.processId != nil {
                captured = (value, frame, pane)
                break
            }
            try await Task.sleep(for: .milliseconds(32))
        }
        guard let (_, frame, pane) = captured, let pid = frame.processId,
              let viewport = frame.snapshot?.viewport else { throw failure("Actual PTY did not publish its rows") }
        let view = coordinator.terminal(session: frame.sessionId, pane: pane)
        guard let window = view.window, view.onInput != nil, view.bounds.width > 100,
              viewport.lines.count > 4, viewport.size.cols > 40 else {
            throw failure("Actual native terminal is not mounted with the production routes")
        }
        let applicationActiveBefore = NSApp.isActive
        let windowVisibleBefore = window.isVisible
        let keyWindowBefore = window.isKeyWindow
        var activationRequestProcessID: Int32?
        var activationFailure: String?
        do {
            activationRequestProcessID = try await activateOwnedApplication()
            if activationRequestProcessID != getpid() {
                activationFailure = "LaunchServices activation returned a different process"
            }
        } catch {
            activationFailure = String(describing: error)
        }
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
        _ = window.makeFirstResponder(view)
        // Window activation is asynchronous. Yield the main actor after the
        // existing native request; never infer focus from the request itself.
        let focusDeadline = ContinuousClock.now + .seconds(5)
        while ContinuousClock.now < focusDeadline,
              !(NSApp.isActive && window.isVisible && window.isKeyWindow && NSApp.keyWindow === window && window.firstResponder === view) {
            try await Task.sleep(for: .milliseconds(32))
        }
        var result = SelectionNativeReport(schemaVersion: 1,
            sourceSHA: Bundle.main.object(forInfoDictionaryKey: "ZmuxSourceSHA") as? String ?? "",
            appPID: ProcessInfo.processInfo.processIdentifier, childPID: pid,
            sessionID: frame.sessionId, surfaceID: pane.selectedSurface, nonce: nonce,
            applicationActiveBefore: applicationActiveBefore, windowVisibleBefore: windowVisibleBefore,
            keyWindowBefore: keyWindowBefore, applicationActive: NSApp.isActive, windowVisible: window.isVisible,
            keyWindow: window.isKeyWindow && NSApp.keyWindow === window,
            terminalFirstResponder: window.firstResponder === view,
            activationRequestProcessID: activationRequestProcessID, activationFailure: activationFailure,
            state: "running", publicWordReferences: [], cases: [])
        // Public system behavior is recorded separately from the independently
        // specified regression literal below. Punctuation is not guessed.
        let reference = NSTextView(frame: .zero)
        for (text, clicked) in [("alpha.beta", 5), ("one  two", 3), ("value_name", 5),
                                ("/tmp/file", 4), ("--flag=value", 6), ("A界 e\u{301}🙂 Z", 1)] {
            reference.string = text
            let range = reference.selectionRange(forProposedRange: NSRange(location: clicked, length: 0), granularity: .selectByWord)
            let string = text as NSString
            guard range.location != NSNotFound, NSMaxRange(range) <= string.length else { throw failure("Public AppKit word range is invalid") }
            result.publicWordReferences.append(SelectionWordReference(text: text, clickedUTF16: clicked,
                location: range.location, length: range.length, selected: string.substring(with: range)))
        }
        guard result.activationFailure == nil, result.activationRequestProcessID == getpid(),
              result.applicationActive, result.windowVisible, result.keyWindow, result.terminalFirstResponder else {
            result.state = "prerequisite_failed"
            try await save(result, to: report)
            throw failure("Exact-self activation or actual focus was unavailable before selection: \(result.activationFailure ?? "no activation error")")
        }
        try await save(result, to: report)

        let clipboard = NSPasteboard.general
        let previous = clipboard.pasteboardItems?.map { original -> NSPasteboardItem in
            let copy = NSPasteboardItem()
            for type in original.types { if let data = original.data(forType: type) { copy.setData(data, forType: type) } }
            return copy
        } ?? []
        defer { clipboard.clearContents(); if !previous.isEmpty { clipboard.writeObjects(previous) } }

        // The first frozen case is the before/after regression: baseline leaves
        // a zero-length selection because mouseDown ignores clickCount.
        clipboard.clearContents()
        clipboard.setString("ZMUX_SELECTION_SENTINEL", forType: .string)
        try click(view, window: window, row: 0, col: 2, count: 2, size: viewport.size)
        try copy(window: window)
        let observed = clipboard.string(forType: .string)
        result.cases.append(SelectionNativeCase(name: "double_click_word", expected: "alpha", observed: observed))
        if observed != "alpha" {
            result.state = "failed"
            try await save(result, to: report)
            throw failure("double_click_word expected alpha; actual clipboard \(observed.debugDescription)")
        }
        result.state = "completed_cases"
        try await save(result, to: report)
        try await coordinator.shutdown()
        result.state = "complete"
        try await save(result, to: report)
    }

    private static func activateOwnedApplication() async throws -> Int32 {
        let current = NSRunningApplication.current
        let bundle = Bundle.main.bundleURL.standardizedFileURL
        guard current.processIdentifier == getpid(), current.bundleURL?.standardizedFileURL == bundle,
              let identifier = current.bundleIdentifier,
              NSWorkspace.shared.runningApplications.filter({ $0.bundleIdentifier == identifier }).map(\.processIdentifier) == [getpid()] else {
            throw failure("LaunchServices did not identify this exact unique running fixture")
        }
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true
        configuration.createsNewApplicationInstance = false
        configuration.allowsRunningApplicationSubstitution = false
        // If LaunchServices unexpectedly starts another instance, offline CLI
        // help exits before AppKit/Rust/storage and cannot run this fixture.
        configuration.arguments = ["--cli", "--help"]
        // This callback runs on LaunchServices' concurrent queue. Transfer only
        // the PID, never NSRunningApplication across the MainActor boundary.
        return try await withCheckedThrowingContinuation { continuation in
            NSWorkspace.shared.openApplication(at: bundle, configuration: configuration) { @Sendable running, error in
                if let error { continuation.resume(throwing: error) }
                else if let running { continuation.resume(returning: running.processIdentifier) }
                else { continuation.resume(throwing: BridgeFailure(message: "Native selection: LaunchServices returned no running application")) }
            }
        }
    }

    private static func click(_ view: TerminalView, window: NSWindow, row: Int, col: Int,
                              count: Int, size: GridSize) throws {
        // Interior cell centers in the actual mounted bounds, including the
        // existing terminal inset. These cases stay far from rounding edges.
        let point = view.convert(NSPoint(x: 10 + (view.bounds.width - 20) / CGFloat(size.cols) * (CGFloat(col) + 0.5),
                                         y: 10 + (view.bounds.height - 20) / CGFloat(size.rows) * (CGFloat(row) + 0.5)), to: nil)
        for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
            guard let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                context: nil, eventNumber: 0, clickCount: count, pressure: type == .leftMouseDown ? 1 : 0) else {
                throw failure("Cannot construct actual native selection event")
            }
            window.sendEvent(event)
        }
    }

    private static func copy(window: NSWindow) throws {
        guard NSApp.isActive, window.isVisible, window.isKeyWindow, NSApp.keyWindow === window, window.firstResponder is TerminalView else {
            throw failure("Actual key window or terminal first responder changed before menu Copy")
        }
        NSApp.mainMenu?.items.forEach { $0.submenu?.update() }
        guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.command],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
            characters: "c", charactersIgnoringModifiers: "c", isARepeat: false, keyCode: 8),
              NSApp.mainMenu?.performKeyEquivalent(with: event) == true else { throw failure("Real Command-C menu path was unavailable") }
    }

    private static func save(_ value: SelectionNativeReport, to url: URL) async throws {
        try await Task.detached {
            let data = try JSONEncoder().encode(value)
            guard data.count <= 131_072 else { throw BridgeFailure(message: "Selection report exceeds its byte bound") }
            try data.write(to: url, options: .atomic)
        }.value
    }

    private static func failure(_ message: String) -> BridgeFailure { BridgeFailure(message: "Native selection: " + message) }
}

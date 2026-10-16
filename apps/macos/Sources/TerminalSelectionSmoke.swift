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

private struct SelectionCellGeometry: Codable, Sendable {
    let originX: Double
    let originY: Double
    let width: Double
    let height: Double
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
    let viewportRows: Int
    let viewportColumns: Int
    let requestedRows: Int
    let requestedColumns: Int
    let terminalWidth: Double
    let terminalHeight: Double
    let keyWindow: Bool
    let terminalFirstResponder: Bool
    let activationRequestProcessID: Int32?
    let activationFailure: String?
    var cellGeometry: SelectionCellGeometry?
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
                let view = coordinator.terminal(session: frame.sessionId, pane: pane)
                if let size = frame.snapshot?.viewport.size,
                   size.rows >= 8, size.cols >= 64,
                   view.requestedSize == (size.rows, size.cols) {
                    captured = (value, frame, pane)
                    break
                }
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
            viewportRows: viewport.size.rows, viewportColumns: viewport.size.cols,
            requestedRows: view.requestedSize.0, requestedColumns: view.requestedSize.1,
            terminalWidth: Double(view.bounds.width), terminalHeight: Double(view.bounds.height),
            keyWindow: window.isKeyWindow && NSApp.keyWindow === window,
            terminalFirstResponder: window.firstResponder === view,
            activationRequestProcessID: activationRequestProcessID, activationFailure: activationFailure,
            cellGeometry: nil, state: "running", publicWordReferences: [], cases: [])
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
        let geometry = try gridGeometry(view, viewport: viewport)
        result.cellGeometry = SelectionCellGeometry(originX: Double(geometry.minX), originY: Double(geometry.minY),
            width: Double(geometry.width), height: Double(geometry.height))
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
        try await check("triple_click_visible_row", expected: "alpha beta", window: window, result: &result, report: report) {
            try click(view, window: window, row: 0, col: 2, count: 3, viewport: viewport)
        }
        for (name, start, end, count, expected) in [
            ("character_forward", 0, 5, 1, "alpha"),
            ("character_reverse", 5, 0, 1, "alpha"),
            ("word_drag_forward", 2, 8, 2, "alpha beta"),
            ("word_drag_reverse", 8, 2, 2, "alpha beta")
        ] {
            try await check(name, expected: expected, window: window, result: &result, report: report) {
                try drag(view, window: window, from: (0, start), to: (0, end), count: count, viewport: viewport)
            }
        }
        try await check("row_drag", expected: "alpha beta\nalpha.beta  value_name /tmp/file", window: window, result: &result, report: report) {
            try drag(view, window: window, from: (0, 2), to: (1, 2), count: 3, viewport: viewport)
        }
        try await check("style_boundary_word", expected: "abcdef", window: window, result: &result, report: report) {
            try click(view, window: window, row: 3, col: 2, count: 2, viewport: viewport)
        }
        for (name, start, end, expected) in [
            ("wide_cell_continuation", 2, 3, "界"),
            ("combining_cell", 4, 5, "e\u{301}"),
            ("emoji_cell_continuation", 6, 7, "🙂")
        ] {
            try await check(name, expected: expected, window: window, result: &result, report: report) {
                try drag(view, window: window, from: (2, start), to: (2, end), count: 1, viewport: viewport)
            }
        }
        try await check("interior_selected_spaces", expected: "  ", window: window, result: &result, report: report) {
            try drag(view, window: window, from: (1, 10), to: (1, 12), count: 1, viewport: viewport)
        }
        try await check("explicit_right_edge_spaces", expected: "  ", window: window, result: &result, report: report) {
            try drag(view, window: window, from: (1, viewport.size.cols - 2), to: (1, viewport.size.cols), count: 1, viewport: viewport)
        }
        for (name, col, expected) in [("dot_word", 5, "alpha.beta"), ("space_word", 10, "  "),
                                       ("underscore_word", 17, "value_name"), ("slash_word", 27, "/")] {
            try await check(name, expected: expected, window: window, result: &result, report: report) {
                try click(view, window: window, row: 1, col: col, count: 2, viewport: viewport)
            }
        }
        let allRows = ["alpha beta", "alpha.beta  value_name /tmp/file", "A界 e\u{301}🙂 Z", "abcdef", "NATIVE_SELECTION_READY_" + nonce]
            + Array(repeating: "", count: viewport.size.rows - 5)
        try await check("select_all_menu", expected: allRows.joined(separator: "\n"), window: window, result: &result, report: report) {
            try shortcut("a", code: 0, window: window)
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
        // Preserve the frozen baseline alpha event exactly.
        let point = view.convert(NSPoint(x: 10 + (view.bounds.width - 20) / CGFloat(size.cols) * (CGFloat(col) + 0.5),
                                         y: 10 + (view.bounds.height - 20) / CGFloat(size.rows) * (CGFloat(row) + 0.5)), to: nil)
        try click(window: window, point: point, count: count)
    }

    private static func click(_ view: TerminalView, window: NSWindow, row: Int, col: Int,
                              count: Int, viewport: Viewport) throws {
        try click(window: window, point: point(view, row: row, col: col, viewport: viewport), count: count)
    }

    private static func click(window: NSWindow, point: NSPoint, count: Int) throws {
        for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
            guard let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                context: nil, eventNumber: 0, clickCount: count, pressure: type == .leftMouseDown ? 1 : 0) else {
                throw failure("Cannot construct actual native selection event")
            }
            window.sendEvent(event)
        }
    }

    private static func drag(_ view: TerminalView, window: NSWindow, from: (Int, Int), to: (Int, Int),
                             count: Int, viewport: Viewport) throws {
        for (type, cell) in [(NSEvent.EventType.leftMouseDown, from), (.leftMouseDragged, to), (.leftMouseUp, to)] {
            guard let event = NSEvent.mouseEvent(with: type, location: try point(view, row: cell.0, col: cell.1, viewport: viewport),
                modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                context: nil, eventNumber: 0, clickCount: count, pressure: type == .leftMouseUp ? 0 : 1) else {
                throw failure("Cannot construct actual native drag event")
            }
            window.sendEvent(event)
        }
    }

    private static func gridGeometry(_ view: TerminalView, viewport: Viewport) throws -> NSRect {
        guard let window = view.window else { throw failure("Actual terminal window disappeared") }
        // Existing NSTextInputClient API returns the actual cursor cell rect.
        // Derive grid origin from its authoritative cursor indices, without
        // duplicating font metrics or adding a test-only production geometry API.
        let screen = view.firstRect(forCharacterRange: NSRange(location: 0, length: 0), actualRange: nil)
        let cursor = view.convert(window.convertFromScreen(screen), from: nil)
        guard cursor.width > 0, cursor.height > 0 else { throw failure("Actual cursor cell rect is empty") }
        return NSRect(x: cursor.minX - CGFloat(viewport.cursorCol) * cursor.width,
                      y: cursor.minY - CGFloat(viewport.cursorRow) * cursor.height,
                      width: cursor.width, height: cursor.height)
    }

    private static func point(_ view: TerminalView, row: Int, col: Int, viewport: Viewport) throws -> NSPoint {
        let grid = try gridGeometry(view, viewport: viewport)
        let local = NSPoint(x: grid.minX + (CGFloat(col) + 0.5) * grid.width,
                            y: grid.minY + (CGFloat(row) + 0.5) * grid.height)
        guard view.bounds.contains(local) else { throw failure("Actual requested cell point is outside terminal bounds") }
        return view.convert(local, to: nil)
    }

    private static func check(_ name: String, expected: String, window: NSWindow,
                              result: inout SelectionNativeReport, report: URL,
                              execute: @MainActor () throws -> Void) async throws {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString("ZMUX_SELECTION_SENTINEL", forType: .string)
        try execute()
        try copy(window: window)
        let observed = NSPasteboard.general.string(forType: .string)
        result.cases.append(SelectionNativeCase(name: name, expected: expected, observed: observed))
        if observed?.utf8.elementsEqual(expected.utf8) != true {
            result.state = "failed"
            try await save(result, to: report)
            throw failure("\(name) clipboard differs from its specified literal")
        }
    }

    private static func copy(window: NSWindow) throws {
        try shortcut("c", code: 8, window: window)
    }

    private static func shortcut(_ key: String, code: UInt16, window: NSWindow) throws {
        guard NSApp.isActive, window.isVisible, window.isKeyWindow, NSApp.keyWindow === window, window.firstResponder is TerminalView else {
            throw failure("Actual key window or terminal first responder changed before menu Copy")
        }
        NSApp.mainMenu?.items.forEach { $0.submenu?.update() }
        guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.command],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
            characters: key, charactersIgnoringModifiers: key, isARepeat: false, keyCode: code),
              NSApp.mainMenu?.performKeyEquivalent(with: event) == true else { throw failure("Real Command-\(key.uppercased()) menu path was unavailable") }
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

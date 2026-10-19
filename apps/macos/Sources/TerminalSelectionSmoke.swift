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

private struct SelectionWrapPhase: Codable, Sendable {
    let pid: Int32
    let parentPID: Int32
    let nonce: String
    let rows: Int
    let columns: Int
    let phase: String?
    enum CodingKeys: String, CodingKey {
        case pid, parentPID = "parent_pid", nonce, rows, columns, phase
    }
}

private struct SelectionPointerCapture: Codable, Sendable {
    let pid: Int32
    let parentPID: Int32
    let nonce: String
    let rows: Int
    let columns: Int
    let phase: String
    let capturedHex: String
    let sentinelHex: String
    let trailingHex: String
    enum CodingKeys: String, CodingKey {
        case pid, parentPID = "parent_pid", nonce, rows, columns, phase
        case capturedHex = "captured_hex", sentinelHex = "sentinel_hex", trailingHex = "trailing_hex"
    }
}

private struct SelectionCopyRows: Codable, Sendable {
    let phase: String
    let scrollbackOffset: Int
    let wrapsToNext: [Bool]
    let writtenEnds: [Int]
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
    var wrapPhase: SelectionWrapPhase?
    var additionalPhases: [SelectionWrapPhase]
    var parkedOffset: Int?
    var copyRows: [SelectionCopyRows]
    var pointerCapture: SelectionPointerCapture?
    var hoverPointerCapture: SelectionPointerCapture?
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
            cellGeometry: nil, wrapPhase: nil, additionalPhases: [], parkedOffset: nil, copyRows: [], pointerCapture: nil, hoverPointerCapture: nil, state: "running", publicWordReferences: [], cases: [])
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
        result.state = "completed_visible_cases"
        try await save(result, to: report)
        let wrapped = try await beginCopyPhase(key: "w", code: 13, marker: "WRAP_READY_",
            coordinator: coordinator, view: view, window: window, result: &result, report: report)
        try await check("wrapped_boundary_written_space", expected: String(repeating: "X", count: wrapped.size.cols - 1) + " EDGE",
                        window: window, result: &result, report: report) {
            try drag(view, window: window, from: (0, 0), to: (1, 4), count: 1, viewport: wrapped)
        }
        for (name, key, code, marker, prefixAdjustment, expectedSuffix) in [
            ("wrapped_wide_cell", "v", UInt16(9), "WIDE_READY_", 2, "界Z"),
            ("wide_margin_engine_hard_boundary", "m", UInt16(46), "MARGIN_READY_", 1, "\n界Z"),
            ("wrapped_combining_emoji", "u", UInt16(32), "UNICODE_READY_", 1, "e\u{301}🙂Z")
        ] {
            let phase = try await beginCopyPhase(key: key, code: code, marker: marker,
                coordinator: coordinator, view: view, window: window, result: &result, report: report)
            // With a full wide cell at the old edge, Z starts row1. The
            // other two payloads place Z after a wide cell on row1.
            let end = key == "v" ? 1 : 3
            try await check(name, expected: String(repeating: "X", count: phase.size.cols - prefixAdjustment) + expectedSuffix,
                            window: window, result: &result, report: report) {
                try drag(view, window: window, from: (0, 0), to: (1, end), count: 1, viewport: phase)
            }
        }
        let hard = try await beginCopyPhase(key: "h", code: 4, marker: "HARD_READY_",
            coordinator: coordinator, view: view, window: window, result: &result, report: report)
        try await check("hard_newline_and_blank_row", expected: "HARD_A\n\nHARD_B", window: window, result: &result, report: report) {
            try drag(view, window: window, from: (0, 0), to: (2, 6), count: 1, viewport: hard)
        }
        _ = try await beginCopyPhase(key: "p", code: 35, marker: "PARK_READY_",
            coordinator: coordinator, view: view, window: window, result: &result, report: report)
        try sendKey("\u{f729}", code: 115, modifiers: [.command], window: window)
        let historyDeadline = ContinuousClock.now + .seconds(5)
        var parked: Viewport?
        while ContinuousClock.now < historyDeadline {
            if let frame = coordinator.snapshot?.frames.first(where: { $0.sessionId == result.sessionID }),
               frame.processId == result.childPID, let viewport = frame.snapshot?.viewport,
               viewport.scrollbackOffset > 0, viewport.contents.hasPrefix("X"),
               viewport.contents.contains("EDGE"), view.requestedSize == (viewport.size.rows, viewport.size.cols),
               (view.accessibilityValue() as? String) == viewport.contents {
                parked = viewport
                break
            }
            try await Task.sleep(for: .milliseconds(32))
        }
        guard let parked else {
            result.state = "prerequisite_failed"
            try await save(result, to: report)
            throw failure("Actual native history navigation did not expose the wrapped rows")
        }
        result.parkedOffset = parked.scrollbackOffset
        try observeRows(parked, phase: "parked", result: &result)
        try await check("wrapped_parked_history", expected: String(repeating: "X", count: parked.size.cols - 1) + " EDGE",
                        window: window, result: &result, report: report) {
            try drag(view, window: window, from: (0, 0), to: (1, 4), count: 1, viewport: parked)
        }
        let word = try await beginCopyPhase(key: "b", code: 11, marker: "WORD_READY_",
            coordinator: coordinator, view: view, window: window, result: &result, report: report)
        let wholeWord = "word" + String(repeating: "x", count: word.size.cols + 4)
        for (name, row, column) in [("wrapped_word_first_row", 0, 2), ("wrapped_word_second_row", 1, 2)] {
            try await check(name, expected: wholeWord, window: window, result: &result, report: report) {
                try click(view, window: window, row: row, col: column, count: 2, viewport: word)
            }
        }
        for (name, start, end) in [
            ("wrapped_word_drag_forward", (0, 2), (1, 10)),
            ("wrapped_word_drag_reverse", (1, 10), (0, 2))
        ] {
            try await check(name, expected: wholeWord + " end", window: window, result: &result, report: report) {
                try drag(view, window: window, from: start, to: end, count: 2, viewport: word)
            }
        }
        for (row, expected) in [(2, "hardleft"), (3, "hardright")] {
            try await check("word_hard_boundary_\(row)", expected: expected, window: window, result: &result, report: report) {
                try click(view, window: window, row: row, col: 2, count: 2, viewport: word)
            }
        }
        let unicodeWord = try await beginCopyPhase(key: "c", code: 8, marker: "WORD_UNICODE_READY_",
            coordinator: coordinator, view: view, window: window, result: &result, report: report)
        for (row, column) in [(0, unicodeWord.size.cols - 1), (1, 1)] {
            try await check("wrapped_combining_word_\(row)",
                            expected: String(repeating: "X", count: unicodeWord.size.cols - 1) + "e\u{301}tail",
                            window: window, result: &result, report: report) {
                try click(view, window: window, row: row, col: column, count: 2, viewport: unicodeWord)
            }
        }
        let pointer = try await beginCopyPhase(key: "t", code: 17, marker: "POINTER_READY_",
            coordinator: coordinator, view: view, window: window, result: &result, report: report)
        // These are actual mounted AppKit handlers and the same bounded native
        // queue as keyboard input. Expected bytes below are public SGR literals,
        // independently captured by the raw PTY child, never returned by an encoder.
        for (type, cell, modifiers) in [
            (NSEvent.EventType.leftMouseDown, (0, 2), NSEvent.ModifierFlags()),
            (.leftMouseDragged, (0, 2), []),
            (.leftMouseDragged, (1, 4), []),
            (.leftMouseDragged, (1, 4), [.option]),
            (.leftMouseDragged, (1, 4), [.option]),
            (.leftMouseDragged, (1, 4), [.shift]),
            (.leftMouseUp, (1, 4), [.shift]),
            (.rightMouseDown, (2, 1), []),
            (.otherMouseDown, (2, 3), []),
            (.rightMouseDragged, (2, 2), [.option]),
            (.otherMouseDragged, (2, 4), [.control]),
            (.rightMouseUp, (2, 2), []),
            (.otherMouseUp, (2, 4), []),
            (.leftMouseDown, (3, 1), []),
            (.leftMouseDragged, (-3, -3), []),
            (.leftMouseDragged, (3, 2), []),
            (.leftMouseUp, (3, 2), []),
            (.mouseMoved, (0, 1), [])
        ] {
            try pointerEvent(type, cell: cell, modifiers: modifiers, view: view, window: window, viewport: pointer,
                             allowOutside: type == .leftMouseDragged)
        }
        // A real responder change must end application ownership without a stuck
        // button. A later mouse-up cannot create a second release.
        try pointerEvent(.leftMouseDown, cell: (3, 4), modifiers: [], view: view, window: window, viewport: pointer)
        guard window.makeFirstResponder(nil), window.makeFirstResponder(view) else { throw failure("Actual pointer cancellation responder change was refused") }
        try pointerEvent(.leftMouseUp, cell: (3, 4), modifiers: [], view: view, window: window, viewport: pointer)
        try await check("pointer_shift_selection", expected: "pointer", window: window, result: &result, report: report) {
            for (type, cell, modifiers) in [
                (NSEvent.EventType.leftMouseDown, (0, 0), NSEvent.ModifierFlags.shift),
                (.leftMouseDragged, (0, 7), []),
                (.leftMouseUp, (0, 7), [])
            ] {
                try pointerEvent(type, cell: cell, modifiers: modifiers, view: view, window: window, viewport: pointer)
            }
        }
        let expectedPointer = "\u{1b}[<0;3;1M\u{1b}[<32;5;2M\u{1b}[<40;5;2M\u{1b}[<36;5;2M\u{1b}[<4;5;2m"
            + "\u{1b}[<2;2;3M\u{1b}[<1;4;3M\u{1b}[<42;3;3M\u{1b}[<49;5;3M\u{1b}[<2;3;3m\u{1b}[<1;5;3m"
            + "\u{1b}[<0;2;4M\u{1b}[<32;1;1M\u{1b}[<32;3;4M\u{1b}[<0;3;4m\u{1b}[<0;5;4M\u{1b}[<0;5;4m"
        let pointerCapture = try await capturePointer(phase: "t", filename: "pointer-child.json", expected: expectedPointer,
            name: "application_pointer_capture", viewport: pointer, coordinator: coordinator, window: window, result: &result, report: report)
        result.pointerCapture = pointerCapture
        let hover = try await beginCopyPhase(key: "y", code: 16, marker: "HOVER_READY_",
            coordinator: coordinator, view: view, window: window, result: &result, report: report)
        guard hover.pointerTracking == .some(.anyMotion), window.acceptsMouseMovedEvents else { throw failure("Actual AnyMotion mode did not request native motion delivery") }
        for (cell, modifiers) in [((0, 1), NSEvent.ModifierFlags()), ((0, 1), []),
                                  ((0, 1), [.option]), ((1, 2), []), ((1, 2), [.shift])] {
            try pointerEvent(.mouseMoved, cell: cell, modifiers: modifiers, view: view, window: window, viewport: hover)
        }
        let hoverCapture = try await capturePointer(phase: "y", filename: "pointer-hover-child.json",
            expected: "\u{1b}[<35;2;1M\u{1b}[<43;2;1M\u{1b}[<35;3;2M", name: "application_hover_capture", viewport: hover,
            coordinator: coordinator, window: window, result: &result, report: report)
        result.hoverPointerCapture = hoverCapture
        let links = try await beginCopyPhase(key: "l", code: 37, marker: "TERMINAL_LINK_READY_",
            coordinator: coordinator, view: view, window: window, result: &result, report: report)
        guard let plainURL = ProcessInfo.processInfo.environment["ZMUX_TERMINAL_LINK_PLAIN_URL"],
              let osc8URL = ProcessInfo.processInfo.environment["ZMUX_TERMINAL_LINK_OSC8_URL"],
              links.size.rows >= 2, links.size.cols >= 64,
              links.contents.contains(plainURL), links.contents.contains("OSC8 LINK") else {
            throw failure("Local HTTP and OSC 8 fixture links were not visible in the production viewport")
        }
        let linkURLs = Set([plainURL, osc8URL])
        let openedBeforeClick = coordinator.snapshot?.model.workspaces.flatMap({ $0.root.panes })
            .flatMap(\.surfaces).filter { $0.kind == "browser" && $0.url.map(linkURLs.contains) == true } ?? []
        let noAutomaticOpen = SelectionNativeCase(name: "printed_links_do_not_auto_open", expected: "no matching browser surface",
                                                  observed: openedBeforeClick.isEmpty ? "no matching browser surface" : "matching browser surface already exists")
        result.cases.append(noAutomaticOpen)
        guard openedBeforeClick.isEmpty else {
            result.state = "failed"
            try await save(result, to: report)
            throw failure("Printing fixture URLs opened a browser before Command-click")
        }
        let plainStart = 6
        let plainEnd = plainStart + (plainURL as NSString).length
        guard plainEnd < links.size.cols else { throw failure("Plain fixture URL does not fit on its specified row") }
        let releaseSelectionOffset = 4
        let releaseSelectionStart = plainStart + releaseSelectionOffset
        let releaseSelection = (plainURL as NSString).substring(from: releaseSelectionOffset)
        try await check("command_link_drag_selection", expected: plainURL, window: window, result: &result, report: report) {
            try pointerEvent(.leftMouseDown, cell: (0, plainStart), modifiers: [.command], view: view, window: window, viewport: links)
            try pointerEvent(.leftMouseDragged, cell: (0, plainEnd), modifiers: [.command], view: view, window: window, viewport: links)
            try pointerEvent(.leftMouseUp, cell: (0, plainEnd), modifiers: [.command], view: view, window: window, viewport: links)
        }
        // No dragged event is delivered. The suffix differs from the previous selection so a
        // swallowed down or stale selection cannot satisfy this clipboard assertion.
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString("ZMUX_SELECTION_SENTINEL", forType: .string)
        try pointerEvent(.leftMouseDown, cell: (0, releaseSelectionStart), modifiers: [.command],
                         view: view, window: window, viewport: links)
        try pointerEvent(.leftMouseUp, cell: (0, plainEnd), modifiers: [.command],
                         view: view, window: window, viewport: links)
        var releaseSelectionCopyError: String?
        do { try copy(window: window) }
        catch { releaseSelectionCopyError = String(describing: error) }
        let releaseSelectionObserved = NSPasteboard.general.string(forType: .string)
        let releaseSelectionMatches = releaseSelectionCopyError == nil
            && releaseSelectionObserved?.utf8.elementsEqual(releaseSelection.utf8) == true
        let releaseSelectionReport = releaseSelectionCopyError.map { "copy failed: \($0)" } ?? releaseSelectionObserved
        result.cases.append(SelectionNativeCase(
            name: "command_link_far_mouse_up_selection",
            expected: releaseSelection, observed: releaseSelectionReport))

        let noOpenDeadline = ContinuousClock.now + .seconds(2)
        var matchingLinkBrowsers: Int?
        while ContinuousClock.now < noOpenDeadline {
            if let snapshot = coordinator.snapshot {
                matchingLinkBrowsers = snapshot.model.workspaces.flatMap({ $0.root.panes })
                    .flatMap(\.surfaces).filter { $0.kind == "browser" && $0.url.map(linkURLs.contains) == true }.count
                if let matchingLinkBrowsers, matchingLinkBrowsers > 0 { break }
            }
            try await Task.sleep(for: .milliseconds(32))
        }
        let noOpenObserved = matchingLinkBrowsers.map { "\($0) matching browser surfaces" } ?? "snapshot unavailable"
        result.cases.append(SelectionNativeCase(name: "command_link_far_mouse_up_no_open",
            expected: "0 matching browser surfaces", observed: noOpenObserved))
        try await save(result, to: report)
        guard releaseSelectionMatches, matchingLinkBrowsers == 0 else {
            result.state = "failed"
            try await save(result, to: report)
            throw failure("Far Command mouse-up selection was \(releaseSelectionReport ?? "empty"); \(noOpenObserved)")
        }

        let linkCases = [
            (name: "command_click_plain_http", url: plainURL, visibleText: plainURL,
             title: "ZMUX_TERMINAL_LINK_PLAIN_" + nonce, row: 0,
             column: plainStart + (plainEnd - plainStart) / 2),
            (name: "command_click_osc8", url: osc8URL, visibleText: "OSC8 LINK",
             title: "ZMUX_TERMINAL_LINK_OSC8_" + nonce, row: 1, column: 5)
        ]
        for link in linkCases {
            guard let current = coordinator.snapshot,
                  let terminalPane = current.model.workspaces.flatMap({ $0.root.panes }).first(where: { $0.id == pane.id }),
                  terminalPane.surfaces.contains(where: { $0.id == pane.selectedSurface && $0.sessionId == frame.sessionId }) else {
                throw failure("Clicked terminal no longer belongs to its original pane")
            }
            if terminalPane.selectedSurface != pane.selectedSurface {
                guard coordinator.enqueue(.surface("select_surface", pane.selectedSurface, nil, nil, nil)) else {
                    throw failure("Could not restore the originating terminal tab for the next link click")
                }
                let selectionDeadline = ContinuousClock.now + .seconds(5)
                while ContinuousClock.now < selectionDeadline,
                      coordinator.snapshot?.model.workspaces.flatMap({ $0.root.panes }).first(where: { $0.id == pane.id })?.selectedSurface != pane.selectedSurface {
                    try await Task.sleep(for: .milliseconds(32))
                }
            }
            guard let mounted = view.window, mounted === window, window.isKeyWindow,
                  window.makeFirstResponder(view), window.firstResponder === view else {
                throw failure("Originating terminal did not return to its mounted native view")
            }
            let viewport = coordinator.snapshot?.frames.first(where: { $0.sessionId == frame.sessionId })?.snapshot?.viewport
            guard let viewport, viewport.contents.contains(link.visibleText), viewport.size.rows > link.row,
                  viewport.size.cols > link.column else { throw failure("Expected terminal link was absent before Command-click") }
            try pointerEvent(.leftMouseDown, cell: (link.row, link.column), modifiers: [.command], view: view, window: window, viewport: viewport)
            try pointerEvent(.leftMouseUp, cell: (link.row, link.column), modifiers: [.command], view: view, window: window, viewport: viewport)

            let browserDeadline = ContinuousClock.now + .seconds(10)
            var observed: (paneID: UInt64, surface: Surface)?
            while ContinuousClock.now < browserDeadline {
                if let snapshot = coordinator.snapshot {
                    observed = snapshot.model.workspaces.flatMap({ $0.root.panes }).compactMap { candidatePane in
                        candidatePane.surfaces.first(where: { $0.kind == "browser" && $0.url == link.url })
                            .map { (paneID: candidatePane.id, surface: $0) }
                    }.first
                    if observed?.surface.title == link.title { break }
                }
                try await Task.sleep(for: .milliseconds(32))
            }
            let expected = "\(link.url)|\(link.title)|pane=\(pane.id)"
            let actual = observed.map { "\($0.surface.url ?? "<missing-url>")|\($0.surface.title)|pane=\($0.paneID)" }
                ?? "no browser surface after Command-click"
            result.cases.append(SelectionNativeCase(name: link.name, expected: expected, observed: actual))
            if observed?.surface.title != link.title || observed?.paneID != pane.id {
                result.state = "failed"
                try await save(result, to: report)
                throw failure("\(link.name) did not load the exact URL in the clicked terminal's pane; observed \(actual)")
            }
        }
        result.state = "completed_cases"
        try await save(result, to: report)
        try await coordinator.shutdown()
        result.state = "complete"
        try await save(result, to: report)
    }

    private static func capturePointer(phase: String, filename: String, expected: String, name: String, viewport: Viewport,
                                       coordinator: ApplicationCoordinator, window: NSWindow,
                                       result: inout SelectionNativeReport, report: URL) async throws -> SelectionPointerCapture {
        try sendKey("x", code: 7, modifiers: [.control], window: window)
        guard let root = ProcessInfo.processInfo.environment["ZMUX_SELECTION_ROOT"] else { throw failure("Pointer capture root unavailable") }
        let captureURL = URL(fileURLWithPath: root).appendingPathComponent(filename)
        let pointerDeadline = ContinuousClock.now + .seconds(10)
        while ContinuousClock.now < pointerDeadline {
            if coordinator.snapshot?.frames.first(where: { $0.sessionId == result.sessionID })?.snapshot?.viewport.contents.contains("POINTER_CAPTURED_" + result.nonce) == true { break }
            try await Task.sleep(for: .milliseconds(32))
        }
        let capture = try await Task.detached {
            let data = try Data(contentsOf: captureURL)
            guard data.count <= 8192 else { throw BridgeFailure(message: "Pointer capture exceeds byte bound") }
            return try JSONDecoder().decode(SelectionPointerCapture.self, from: data)
        }.value
        guard UInt32(exactly: capture.pid) == result.childPID, capture.parentPID == getpid(), capture.nonce == result.nonce,
              capture.rows == viewport.size.rows, capture.columns == viewport.size.cols, capture.phase == phase,
              capture.sentinelHex == "18", capture.trailingHex.isEmpty else { throw failure("Pointer capture child identity, grid or native key sentinel differs") }
        let expectedPointer = expected.utf8.map { String(format: "%02x", $0) }.joined()
        result.cases.append(SelectionNativeCase(name: name, expected: expectedPointer, observed: capture.capturedHex))
        if capture.capturedHex != expectedPointer {
            result.state = "failed"
            try await save(result, to: report)
            throw failure("Actual PTY did not receive the exact native pointer reports; observed \(capture.capturedHex)")
        }
        return capture
    }

    private static func beginCopyPhase(key: String, code: UInt16, marker: String,
                                       coordinator: ApplicationCoordinator, view: TerminalView, window: NSWindow,
                                       result: inout SelectionNativeReport, report: URL) async throws -> Viewport {
        do {
            return try await captureCopyPhase(key: key, code: code, marker: marker,
                coordinator: coordinator, view: view, window: window, result: &result)
        } catch {
            result.state = "prerequisite_failed"
            try await save(result, to: report)
            throw error
        }
    }

    private static func captureCopyPhase(key: String, code: UInt16, marker: String,
                                         coordinator: ApplicationCoordinator, view: TerminalView, window: NSWindow,
                                         result: inout SelectionNativeReport) async throws -> Viewport {
        guard NSApp.isActive, window.isKeyWindow, NSApp.keyWindow === window, window.firstResponder === view,
              let root = ProcessInfo.processInfo.environment["ZMUX_SELECTION_ROOT"] else {
            throw failure("Actual focus or private child phase root unavailable before wrapped-copy fixture")
        }
        // This is an actual native text key routed through the production input
        // lane. The finite PTY child observes it before emitting its next phase.
        try sendKey(key, code: code, modifiers: [], window: window)
        let deadline = ContinuousClock.now + .seconds(10)
        var captured: Viewport?
        while ContinuousClock.now < deadline {
            if let frame = coordinator.snapshot?.frames.first(where: { $0.sessionId == result.sessionID }),
               frame.processId == result.childPID, frame.snapshot?.state == "running",
               let viewport = frame.snapshot?.viewport,
               viewport.contents.contains(marker + result.nonce),
               view.requestedSize == (viewport.size.rows, viewport.size.cols),
               (view.accessibilityValue() as? String) == viewport.contents {
                captured = viewport
                break
            }
            try await Task.sleep(for: .milliseconds(32))
        }
        guard let viewport = captured else { throw failure("Actual wrapped PTY phase did not reach the mounted viewport") }
        let registration = key == "w" ? "wrap-child.json" : "copy-child-" + key + ".json"
        let url = URL(fileURLWithPath: root).appendingPathComponent(registration)
        let phase = try await Task.detached {
            let data = try Data(contentsOf: url)
            guard data.count <= 4096 else { throw BridgeFailure(message: "Wrapped child phase exceeds byte bound") }
            return try JSONDecoder().decode(SelectionWrapPhase.self, from: data)
        }.value
        if key == "w" { result.wrapPhase = phase } else { result.additionalPhases.append(phase) }
        guard phase.phase == key, UInt32(exactly: phase.pid) == result.childPID, phase.parentPID == getpid(), phase.nonce == result.nonce,
              phase.rows == viewport.size.rows, phase.columns == viewport.size.cols,
              viewport.size.rows >= 8, viewport.size.cols >= 64, viewport.size.cols <= 240,
              NSApp.isActive, window.isVisible, window.isKeyWindow, NSApp.keyWindow === window, window.firstResponder === view else {
            throw failure("Wrapped-copy prerequisite differs: phase=\(phase.phase ?? "<missing>")/\(key) child=\(phase.pid)/\(result.childPID) parent=\(phase.parentPID)/\(getpid()) nonceMatches=\(phase.nonce == result.nonce) kernel=\(phase.rows)x\(phase.columns) viewport=\(viewport.size.rows)x\(viewport.size.cols) active=\(NSApp.isActive) visible=\(window.isVisible) key=\(window.isKeyWindow) exactKey=\(NSApp.keyWindow === window) terminalResponder=\(window.firstResponder === view)")
        }
        try observeRows(viewport, phase: key, result: &result)
        return viewport
    }

    private static func observeRows(_ viewport: Viewport, phase: String, result: inout SelectionNativeReport) throws {
        guard let metadata = viewport.rowCopy else { throw failure("Default fixture engine did not publish copy metadata") }
        result.copyRows.append(SelectionCopyRows(phase: phase, scrollbackOffset: viewport.scrollbackOffset,
            wrapsToNext: metadata.map(\.wrapsToNext), writtenEnds: metadata.map(\.writtenEnd)))
    }

    private static func sendKey(_ key: String, code: UInt16, modifiers: NSEvent.ModifierFlags, window: NSWindow) throws {
        for type in [NSEvent.EventType.keyDown, .keyUp] {
            guard let event = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: modifiers,
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
                characters: key, charactersIgnoringModifiers: key, isARepeat: false, keyCode: code) else {
                throw failure("Cannot construct actual native phase/navigation key")
            }
            window.sendEvent(event)
        }
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

    private static func pointerEvent(_ type: NSEvent.EventType, cell: (Int, Int), modifiers: NSEvent.ModifierFlags,
                                     view: TerminalView, window: NSWindow, viewport: Viewport, allowOutside: Bool = false) throws {
        let grid = try gridGeometry(view, viewport: viewport)
        let location: NSPoint
        if allowOutside {
            location = view.convert(NSPoint(x: grid.minX + (CGFloat(cell.1) + 0.5) * grid.width,
                y: grid.minY + (CGFloat(cell.0) + 0.5) * grid.height), to: nil)
        } else { location = try point(view, row: cell.0, col: cell.1, viewport: viewport) }
        guard let original = NSEvent.mouseEvent(with: type, location: location,
            modifierFlags: modifiers, timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, eventNumber: 0, clickCount: 1, pressure: type == .leftMouseUp ? 0 : 1) else {
            throw failure("Cannot construct actual native pointer event")
        }
        var event = original
        if [NSEvent.EventType.otherMouseDown, .otherMouseDragged, .otherMouseUp].contains(type) {
            // AppKit's convenience initializer defaults the button number to
            // zero, even for otherMouseDown. Set the public Core Graphics field
            // and prove the windowed event survived the public round trip.
            guard let cg = original.cgEvent else { throw failure("Public middle-button fixture has no Core Graphics event") }
            cg.setIntegerValueField(.mouseEventButtonNumber, value: 2)
            let converted = NSEvent(cgEvent: cg)
            let candidate = original.buttonNumber == 2 ? original : converted
            guard let middle = candidate, middle.type == type, middle.buttonNumber == 2,
                  middle.windowNumber == window.windowNumber,
                  abs(middle.locationInWindow.x - location.x) < 0.01,
                  abs(middle.locationInWindow.y - location.y) < 0.01 else {
                throw failure("Public middle-button fixture did not retain type, button, window and coordinates: originalButton=\(original.buttonNumber), convertedButton=\(converted?.buttonNumber ?? -1), convertedWindow=\(converted?.windowNumber ?? -1), expectedWindow=\(window.windowNumber), convertedPoint=\(String(describing: converted?.locationInWindow)), expectedPoint=\(location)")
            }
            event = middle
        }
        window.sendEvent(event)
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
        try await save(result, to: report)
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

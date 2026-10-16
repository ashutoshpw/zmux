import AppKit

/// Native presentation of fixture-generated saved history through normal restore.
/// This does not authenticate a supervisor or establish the core no-launch contract.
@MainActor
enum CompletedTerminalHistorySmoke {
    private struct Copy: Codable, Sendable {
        let name: String
        let expected: String
        let observed: String?
    }
    private struct Position: Codable, Sendable {
        let stage: String
        let location: Int
        let length: Int
        let scrollY: Double
    }
    private struct Evidence: Encodable, Sendable {
        let scope = "fixture_generated_saved_history_native_presentation"
        let sourceSHA: String
        let appPID: Int32
        var state = "starting"
        var workspace: UInt64?
        var surface: UInt64?
        var sourceWindow: UInt64?
        var destinationWindow: UInt64?
        var activationPID: Int32?
        var checks: [String] = []
        var copies: [Copy] = []
        var positions: [Position] = []
        var failure: String?
    }

    static func run(coordinator: ApplicationCoordinator, report: URL) async throws {
        var evidence = Evidence(sourceSHA: Bundle.main.object(forInfoDictionaryKey: "ZmuxSourceSHA") as? String ?? "",
                                appPID: ProcessInfo.processInfo.processIdentifier)
        let clipboard = NSPasteboard.general
        let original: [NSPasteboardItem] = clipboard.pasteboardItems?.map { item -> NSPasteboardItem in
            let saved = NSPasteboardItem()
            for type in item.types { if let data = item.data(forType: type) { saved.setData(data, forType: type) } }
            return saved
        } ?? []
        defer {
            clipboard.clearContents()
            if !original.isEmpty { clipboard.writeObjects(original) }
        }
        do {
            // The runner writes this independent plaintext oracle before app startup.
            let expected = try await Task.detached {
                let data = try Data(contentsOf: report.appendingPathExtension("expected.txt"))
                guard data.count <= 16_384, let value = String(data: data, encoding: .utf8),
                      value.components(separatedBy: "\n").count >= 40 else {
                    throw BridgeFailure(message: "Completed-history oracle must be bounded multiline UTF-8")
                }
                return value
            }.value
            try await wait(coordinator, "restored two-window completed fixture") { state in
                state.persistence?.startupPending != true && state.persistence?.restoring != true &&
                state.model.windows.count == 2 && coordinator.mainWindowHosts.count == 2 &&
                coordinator.mainWindowHosts.values.allSatisfy(\.nativeReady) &&
                state.model.workspaces.count == 1 &&
                state.model.workspaces[0].root.panes.count == 1 &&
                state.model.workspaces[0].root.panes[0].selected.localZellijCompletion != nil
            }
            guard let state = coordinator.snapshot, let workspace = state.model.workspaces.first,
                  let pane = workspace.root.panes.first, pane.surfaces.count == 1,
                  pane.selected.sessionId == nil,
                  let history = pane.selected.localZellijCompletion,
                  history.viewport?.contents.utf8.elementsEqual(expected.utf8) == true,
                  let sourceID = state.model.windows.first(where: { $0.workspaceIds.contains(workspace.id) })?.id,
                  let destinationID = state.model.windows.first(where: { $0.id != sourceID && $0.workspaceIds.isEmpty })?.id,
                  let source = host(coordinator, sourceID), let destination = host(coordinator, destinationID),
                  let workspaceUUID = state.workspaceIdentities.first(where: { $0.workspaceId == workspace.id })?.uuid,
                  let destinationUUID = destination.windowUUID,
                  let view = coordinator.completedHistories[pane.selectedSurface],
                  let output = view.outputResponder as? NSTextView,
                  let scroll = output.enclosingScrollView, view.window === source.window else {
                throw failure("Restored production model/view did not match the independent saved-output fixture")
            }
            evidence.workspace = workspace.id; evidence.surface = pane.selectedSurface
            evidence.sourceWindow = sourceID; evidence.destinationWindow = destinationID
            evidence.checks.append("actual_restore_model_and_mounted_saved_output")
            for window in [source.window, destination.window] { window.setContentSize(NSSize(width: 760, height: 380)) }
            source.window.contentView?.layoutSubtreeIfNeeded()
            destination.window.contentView?.layoutSubtreeIfNeeded()
            evidence.activationPID = try await activateOwnedApplication()
            guard evidence.activationPID == evidence.appPID else { throw failure("Activation substituted another process") }
            source.installMenus(); source.window.makeKeyAndOrderFront(nil); NSApp.activate()
            guard source.window.makeFirstResponder(output) else { throw failure("Saved output refused first responder") }
            try await wait(coordinator, "actual saved-output focus") { _ in focused(source.window, output) }
            guard output.string.utf8.elementsEqual(expected.utf8), output.isSelectable, !output.isEditable else {
                throw failure("Saved-output presentation is not exact selectable read-only text")
            }
            let renderedLabels = labels(in: view)
            guard renderedLabels.contains("Process exited with status 7"), renderedLabels.contains("Saved output · truncated") else {
                throw failure("Restored exit and truncation status did not render")
            }
            try menu("a", code: 0, window: source.window, output: output)
            guard output.selectedRange() == NSRange(location: 0, length: (expected as NSString).length) else {
                throw failure("Actual Select All menu did not select exact saved output")
            }
            try copied("select_all", expected: expected, window: source.window, output: output, evidence: &evidence)
            evidence.checks.append("actual_select_all_and_exact_utf8_copy")

            let selected = "row 020 · α界 e\u{301}🙂  "
            let range = (expected as NSString).range(of: selected)
            guard range.location != NSNotFound else { throw failure("Independent fixture lacks its Unicode selection literal") }
            output.setSelectedRange(range)
            try key("!", code: 18, window: source.window)
            clipboard.clearContents(); clipboard.setString("SHOULD_NOT_REPLACE_SAVED_OUTPUT", forType: .string)
            try menu("v", code: 9, window: source.window, output: output)
            guard output.string.utf8.elementsEqual(expected.utf8), output.selectedRange() == range else {
                throw failure("Native typing or menu Paste edited saved output")
            }
            evidence.checks.append("native_typing_and_menu_paste_leave_output_read_only")
            output.scrollRangeToVisible(range)
            scroll.layoutSubtreeIfNeeded()
            await Task.yield()
            let origin = scroll.contentView.bounds.origin
            guard origin.y > 20 else { throw failure("Actual saved-output viewport did not scroll") }
            record("before_move", output: output, scroll: scroll, evidence: &evidence)
            try copied("unicode_selection", expected: selected, window: source.window, output: output, evidence: &evidence)
            try await save(evidence, to: report)

            // CLI enters the real serialized model mutation and coordinator reconciliation.
            let moved = try await source.nativeCLI(["move-workspace-to-window", "--workspace", workspaceUUID.uuidString,
                "--window", destinationUUID.uuidString, "--focus", "true"], timeout: 8_000, externalCaller: true)
            guard let receipt = try JSONSerialization.jsonObject(with: moved) as? [String: Any],
                  receipt["state"] as? String == "topology_committed" else {
                throw failure("Real move omitted its topology commitment")
            }
            try await wait(coordinator, "canonical move and mounted destination") { state in
                state.model.windows.first(where: { $0.id == sourceID })?.workspaceIds.isEmpty == true &&
                state.model.windows.first(where: { $0.id == destinationID })?.selectedWorkspace == workspace.id &&
                coordinator.completedHistories[pane.selectedSurface]?.window === destination.window
            }
            destination.installMenus()
            try await wait(coordinator, "destination saved-output responder") { _ in focused(destination.window, output) }
            try retained(view, output: output, scroll: scroll, range: range, origin: origin, expected: expected,
                         surface: pane.selectedSurface, coordinator: coordinator)
            record("after_move", output: output, scroll: scroll, evidence: &evidence)
            evidence.checks.append("canonical_cross_window_move_retains_selection_scroll_and_focus")
            try copied("after_move", expected: selected, window: destination.window, output: output, evidence: &evidence)
            source.window.performClose(nil)
            try await wait(coordinator, "actual source window close") { state in
                !state.model.windows.contains(where: { $0.id == sourceID }) &&
                host(coordinator, sourceID) == nil && !source.window.isVisible &&
                coordinator.completedHistories[pane.selectedSurface]?.window === destination.window
            }
            try retained(view, output: output, scroll: scroll, range: range, origin: origin, expected: expected,
                         surface: pane.selectedSurface, coordinator: coordinator)
            guard focused(destination.window, output) else { throw failure("Source close lost saved-output responder") }
            record("after_source_close", output: output, scroll: scroll, evidence: &evidence)
            try copied("after_source_close", expected: selected, window: destination.window, output: output, evidence: &evidence)
            evidence.checks.append("source_window_close_retains_saved_output_state")
            evidence.state = "assertions_complete"
            try await save(evidence, to: report)
            try await coordinator.shutdown()
            evidence.state = "complete"
            try await save(evidence, to: report)
        } catch {
            evidence.state = "failed"; evidence.failure = String(describing: error)
            try await save(evidence, to: report)
            throw error
        }
    }

    private static func retained(_ view: CompletedTerminalHistoryView, output: NSTextView, scroll: NSScrollView,
                                 range: NSRange, origin: NSPoint, expected: String, surface: UInt64,
                                 coordinator: ApplicationCoordinator) throws {
        guard coordinator.completedHistories[surface] === view, view.outputResponder === output,
              output.selectedRange() == range, abs(scroll.contentView.bounds.origin.y - origin.y) < 0.5,
              abs(scroll.contentView.bounds.origin.x - origin.x) < 0.5,
              output.string.utf8.elementsEqual(expected.utf8) else {
            throw failure("Saved output was recreated or lost its exact selection, scroll or contents")
        }
    }
    private static func record(_ stage: String, output: NSTextView, scroll: NSScrollView, evidence: inout Evidence) {
        let selection = output.selectedRange()
        evidence.positions.append(Position(stage: stage, location: selection.location, length: selection.length,
                                           scrollY: Double(scroll.contentView.bounds.origin.y)))
    }
    private static func copied(_ name: String, expected: String, window: NSWindow, output: NSTextView,
                               evidence: inout Evidence) throws {
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("SAVED_OUTPUT_COPY_SENTINEL", forType: .string)
        try menu("c", code: 8, window: window, output: output)
        let observed = NSPasteboard.general.string(forType: .string)
        evidence.copies.append(Copy(name: name, expected: expected, observed: observed))
        guard observed?.utf8.elementsEqual(expected.utf8) == true else { throw failure("\(name) clipboard differs from independent literal") }
    }
    private static func menu(_ value: String, code: UInt16, window: NSWindow, output: NSTextView) throws {
        guard focused(window, output) else { throw failure("Actual saved-output focus unavailable before menu action") }
        NSApp.mainMenu?.items.forEach { $0.submenu?.update() }
        guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.command],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
            characters: value, charactersIgnoringModifiers: value, isARepeat: false, keyCode: code),
              NSApp.mainMenu?.performKeyEquivalent(with: event) == true else {
            throw failure("Real Command-\(value.uppercased()) menu action unavailable")
        }
    }
    private static func key(_ value: String, code: UInt16, window: NSWindow) throws {
        guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
            characters: value, charactersIgnoringModifiers: value, isARepeat: false, keyCode: code) else {
            throw failure("Cannot construct native typing event")
        }
        window.sendEvent(event)
    }
    private static func focused(_ window: NSWindow, _ output: NSTextView) -> Bool {
        NSApp.isActive && window.isVisible && window.isKeyWindow && NSApp.keyWindow === window && window.firstResponder === output
    }
    private static func host(_ coordinator: ApplicationCoordinator, _ id: UInt64) -> MainWindowHost? {
        coordinator.mainWindowHosts.first(where: { $0.key.windowId == id })?.value
    }
    private static func labels(in view: NSView) -> [String] {
        (view as? NSTextField).map { [$0.stringValue] } ?? view.subviews.flatMap { labels(in: $0) }
    }
    private static func wait(_ coordinator: ApplicationCoordinator, _ description: String,
                             until predicate: (DesktopSnapshot) -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(8)
        while ContinuousClock.now < deadline {
            if let snapshot = coordinator.snapshot, predicate(snapshot) { return }
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
                else { continuation.resume(throwing: BridgeFailure(message: "Saved-output activation returned no process")) }
            }
        }
    }
    private static func save(_ value: Evidence, to report: URL) async throws {
        try await Task.detached {
            let data = try JSONEncoder().encode(value)
            guard data.count <= 65_536 else { throw BridgeFailure(message: "Completed-history report exceeds its byte bound") }
            try data.write(to: report, options: .atomic)
        }.value
    }
    private static func failure(_ text: String) -> BridgeFailure { BridgeFailure(message: "Completed-history native presentation: " + text) }
}

import AppKit
import Foundation

/// Packaged acceptance through the Workspace menu, AppKit editor, Rust desktop
/// owner, managed SSH master, and the remote scheduler's durable receipts.
@MainActor
enum ScheduleMessagesSmoke {
    private struct Configuration: Decodable, Sendable {
        let root: String
        let clientConfig: String
        let identity: String
        let schedulerRpcRequestLog: String
        let schedulerRpcDropResponseMarker: String
    }

    private struct Route: Sendable {
        let identityNamespace: UUID
        let surface: UInt64
        let session: UInt64
    }

    private struct MutationRecord: Sendable {
        let requestID: UUID
        let action: String
        let patchKeys: [String]
        let jobID: String?
        let message: String?
    }

    private struct CapturedLog: Sendable {
        let data: Data
        let permissions: UInt16
    }

    private struct SchedulerActionReply {
        let kind: String
        let value: [String: Any]
    }

    @MainActor
    private struct Controls {
        let window: NSWindow
        let refresh: NSButton
        let target: NSPopUpButton
        let message: NSTextView
        let deadline: NSTextField
        let viewMode: NSSegmentedControl
        let table: NSTableView
        let split: NSSplitView
        let queueScroll: NSScrollView
        let editorScroll: NSScrollView
        let new: NSButton
        let save: NSButton
        let cancel: NSButton
        let requestStatus: NSButton

        init(window: NSWindow) throws {
            guard let content = window.contentView else {
                throw BridgeFailure(message: "Scheduled messages window has no content view")
            }
            let views = ScheduleMessagesSmoke.descendants(content)
            @MainActor
            func required<T: NSView>(_ type: T.Type, label: String) throws -> T {
                guard let value = views.first(where: { $0.accessibilityLabel() == label }) as? T else {
                    throw BridgeFailure(message: "Scheduled messages window is missing the \(label) control")
                }
                return value
            }
            self.window = window
            refresh = try required(NSButton.self, label: "Refresh scheduled messages")
            target = try required(NSPopUpButton.self, label: "Remote tmux pane")
            message = try required(NSTextView.self, label: "Scheduled message text")
            deadline = try required(NSTextField.self, label: "Scheduled local date and time")
            viewMode = try required(NSSegmentedControl.self, label: "Scheduled message list view")
            table = try required(NSTableView.self, label: "Scheduled message list")
            split = try required(NSSplitView.self, label: "Scheduled message queue and editor")
            queueScroll = try required(NSScrollView.self, label: "Pending scheduled messages and history")
            editorScroll = try required(NSScrollView.self, label: "Scheduled message editor scroll area")
            new = try required(NSButton.self, label: "Create a new scheduled message")
            save = try required(NSButton.self, label: "Save or schedule message")
            cancel = try required(NSButton.self, label: "Cancel selected pending scheduled message")
            requestStatus = try required(NSButton.self, label: "Check the remote receipt for the unresolved request")
        }
    }

    static func run(coordinator: ApplicationCoordinator, host: MainWindowHost,
                    report: URL, configuration: URL) async throws {
        let config = try await Task.detached(priority: .utility) {
            try JSONDecoder().decode(Configuration.self, from: Data(contentsOf: configuration))
        }.value
        let sourceSHA = Bundle.main.object(forInfoDictionaryKey: "ZmuxSourceSHA") as? String ?? "local"
        var evidence: [String: Any] = [
            "scope": "native_schedule_messages",
            "source_sha": sourceSHA,
            "state": "running",
            "checks": [],
            "unknown_outcome_ui_exercised": false
        ]
        var originalWorkspace: UInt64?
        var remoteWorkspaceID: UInt64?
        var scheduleWindow: NSWindow?
        do {
            try await waitUntil("a valid initial coordinator workspace") {
                guard let snapshot = coordinator.snapshot,
                      let focusedWorkspace = snapshot.model.focusedWorkspace else { return false }
                return snapshot.model.workspaces.contains(where: { $0.id == focusedWorkspace })
            }
            guard let initialSnapshot = coordinator.snapshot,
                  let initialWorkspace = initialSnapshot.model.focusedWorkspace,
                  initialSnapshot.model.workspaces.contains(where: { $0.id == initialWorkspace }) else {
                throw BridgeFailure(message: "Initial focused workspace disappeared from the coordinator model")
            }
            originalWorkspace = initialWorkspace
            let workspaceName = "Native scheduled messages " + String(UUID().uuidString.prefix(8))
            _ = try await host.nativeCLI([
                "ssh", "fixture", "--ssh-config", config.clientConfig, "--identity", config.identity,
                "--name", String(workspaceName), "--command", "printf '\\nZMUX_SCHEDULE_MESSAGES_READY\\n'; exec /bin/sleep 3600"
            ], timeout: 15_000, externalCaller: true)
            let remoteWorkspace = try await waitForRemoteWorkspace(named: String(workspaceName), coordinator: coordinator)
            remoteWorkspaceID = remoteWorkspace.id
            _ = try await host.nativeCLI(["select-workspace", String(remoteWorkspace.id)], externalCaller: true)
            try await waitUntil("the selected managed SSH session") {
                Self.route(coordinator: coordinator, workspaceID: remoteWorkspace.id) != nil
                    && coordinator.snapshot?.model.focusedWorkspace == remoteWorkspace.id
            }
            guard let route = route(coordinator: coordinator, workspaceID: remoteWorkspace.id) else {
                throw BridgeFailure(message: "Remote scheduler smoke lost its managed SSH surface")
            }
            evidence["remote_surface"] = route.surface
            evidence["remote_session"] = route.session

            host.window.makeKeyAndOrderFront(nil)
            host.installMenus()
            NSApp.activate(ignoringOtherApps: true)
            guard let mainMenu = NSApp.mainMenu,
                  let workspaceMenu = mainMenu.items.first(where: { $0.title == "Workspace" })?.submenu else {
                throw BridgeFailure(message: "The real Workspace menu is unavailable")
            }
            workspaceMenu.update()
            guard let scheduleItem = workspaceMenu.items.first(where: { $0.title == "Scheduled Messages…" }),
                  scheduleItem.isEnabled else {
                throw BridgeFailure(message: "Workspace menu did not enable Scheduled Messages for the managed SSH terminal")
            }
            workspaceMenu.performActionForItem(at: workspaceMenu.index(of: scheduleItem))
            try await waitUntil("the Scheduled Messages window") {
                scheduleWindow = NSApp.windows.first(where: { $0.title == "Scheduled Messages" && $0.isVisible })
                return scheduleWindow != nil
            }
            guard let scheduleWindow else {
                throw BridgeFailure(message: "The Workspace menu did not open Scheduled Messages")
            }
            let controls = try Controls(window: scheduleWindow)
            scheduleWindow.makeKeyAndOrderFront(nil)
            try await waitUntil("remote scheduler targets and first queue refresh") {
                controls.target.numberOfItems > 0
                    && Self.visibleText(in: scheduleWindow).contains("Remote scheduler refreshed")
                    && Self.visibleText(in: scheduleWindow).contains("tmux available")
            }
            guard controls.target.selectedItem?.representedObject as? String != nil else {
                throw BridgeFailure(message: "Remote scheduler did not select an identity-fenced tmux target")
            }
            append("workspace_menu_opens_real_scheduler_window", to: &evidence)
            append("managed_ssh_refresh_shows_remote_tmux_target", to: &evidence)

            let token = String(UUID().uuidString.prefix(10))
            let originalMessage = "Native schedule create \(token)"
            let editedMessage = "Native schedule message edit \(token)"
            let combinedMessage = "Native schedule combined edit \(token)"
            let firstDeadline = localMinuteText(offsetMinutes: 30)
            let timeOnlyDeadline = localMinuteText(offsetMinutes: 33)
            let combinedDeadline = localMinuteText(offsetMinutes: 36)
            let fixtureRoot = URL(fileURLWithPath: config.root, isDirectory: true).standardizedFileURL
            let dropMarker = URL(fileURLWithPath: config.schedulerRpcDropResponseMarker).standardizedFileURL
            guard dropMarker.deletingLastPathComponent() == fixtureRoot else {
                throw BridgeFailure(message: "Scheduler RPC drop marker does not belong to the private SSH fixture root")
            }

            try replaceMessage(originalMessage, in: controls)
            try setDeadline(firstDeadline, in: controls)
            try await waitUntil("a valid create action") { controls.save.isEnabled }
            try createPrivateMarker(at: dropMarker, privateRoot: fixtureRoot)
            try click(controls.save, named: "Schedule Message")
            try await waitUntil("the create request's unknown-outcome state") {
                !controls.requestStatus.isHidden
                    && controls.requestStatus.isEnabled
                    && controls.save.title == "Awaiting Request Status"
                    && !controls.save.isEnabled
                    && !FileManager.default.fileExists(atPath: dropMarker.path)
                    && Self.visibleText(in: scheduleWindow).contains("may have been applied")
            }
            evidence["unknown_outcome_ui_exercised"] = true
            append("lost_create_reply_enters_unknown_outcome_ui", to: &evidence)
            try click(controls.requestStatus, named: "Check Request Status")
            try await waitUntil("applied create receipt selecting one populated job") {
                guard let row = Self.rowIndex(message: originalMessage, in: controls.table) else { return false }
                return controls.requestStatus.isHidden
                    && controls.table.numberOfRows == 1
                    && controls.table.selectedRow == row
                    && Self.cellText(column: 2, row: row, in: controls.table) == firstDeadline
                    && Self.cellText(column: 3, row: row, in: controls.table) == "pending · v1"
                    && controls.message.string == originalMessage
                    && controls.deadline.stringValue == firstDeadline
                    && controls.save.title == "Save Changes"
                    && !controls.save.isEnabled
            }
            append("applied_create_receipt_selects_one_populated_job_without_duplicate", to: &evidence)

            try replaceMessage(editedMessage, in: controls)
            try await waitUntil("message-only edit readiness") { controls.save.isEnabled }
            try click(controls.save, named: "Save Changes")
            try await waitForRow(message: editedMessage, deadline: firstDeadline, version: 2, in: controls)
            append("message_only_edit_preserves_deadline", to: &evidence)

            try setDeadline(timeOnlyDeadline, in: controls)
            try await waitUntil("time-only edit readiness") { controls.save.isEnabled }
            try click(controls.save, named: "Save Changes")
            try await waitForRow(message: editedMessage, deadline: timeOnlyDeadline, version: 3, in: controls)
            append("time_only_edit_preserves_message", to: &evidence)

            try replaceMessage(combinedMessage, in: controls)
            try setDeadline(combinedDeadline, in: controls)
            try await waitUntil("combined edit readiness") { controls.save.isEnabled }
            try click(controls.save, named: "Save Changes")
            try await waitForRow(message: combinedMessage, deadline: combinedDeadline, version: 4, in: controls)
            append("combined_message_and_time_edit", to: &evidence)

            try await waitUntil("pending cancellation readiness") { controls.cancel.isEnabled }
            try click(controls.cancel, named: "Cancel Job")
            try await waitUntil("cancellation in remote history") {
                Self.rowIndex(message: combinedMessage, in: controls.table) != nil
                    && controls.viewMode.selectedSegment == 1
            }
            guard let cancelledRow = Self.rowIndex(message: combinedMessage, in: controls.table),
                  Self.cellText(column: 3, row: cancelledRow, in: controls.table)?.lowercased().contains("cancel") == true else {
                throw BridgeFailure(message: "Cancelled scheduler job did not appear with its terminal state in history")
            }
            controls.viewMode.selectedSegment = 0
            try sendAction(controls.viewMode, named: "Pending scheduled messages")
            try await waitUntil("empty pending queue after cancellation") {
                controls.viewMode.selectedSegment == 0 && controls.table.numberOfRows == 0
            }
            controls.viewMode.selectedSegment = 1
            try sendAction(controls.viewMode, named: "Scheduled message history")
            try await waitUntil("cancelled history row after switching views") {
                controls.viewMode.selectedSegment == 1 && Self.rowIndex(message: combinedMessage, in: controls.table) != nil
            }
            guard let historyRow = Self.rowIndex(message: combinedMessage, in: controls.table) else {
                throw BridgeFailure(message: "Cancelled scheduler job disappeared before its history details could be selected")
            }
            controls.table.selectRowIndexes(IndexSet(integer: historyRow), byExtendingSelection: false)
            try await waitUntil("populated cancelled history details") {
                controls.table.selectedRow == historyRow
                    && controls.message.string == combinedMessage
                    && controls.deadline.stringValue == combinedDeadline
            }
            try assertLayoutGeometryAndVisibleText(controls, expectedMessage: combinedMessage)
            append("scheduled_messages_layout_geometry_and_visible_text", to: &evidence)
            append("cancel_removes_pending_job_and_history_view_retains_receipt", to: &evidence)
            scheduleWindow.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            guard scheduleWindow.isVisible else {
                throw BridgeFailure(message: "Scheduled messages window was not visible for its populated history screenshot")
            }
            let screenshot = report.deletingLastPathComponent().appendingPathComponent("zmux-scheduled-messages.png")
            try capture(scheduleWindow, to: screenshot)
            evidence["screenshot"] = screenshot.lastPathComponent
            append("populated_history_editor_screenshot_retained", to: &evidence)

            let requestLog = URL(fileURLWithPath: config.schedulerRpcRequestLog).standardizedFileURL
            guard requestLog.deletingLastPathComponent() == fixtureRoot else {
                throw BridgeFailure(message: "Captured scheduler RPC log does not belong to the private SSH fixture root")
            }
            let mutations = try await capturedMutations(at: requestLog,
                                                        expectedCreateMessage: originalMessage)
            try validateMutationSequence(mutations)
            var receiptEvidence: [[String: String]] = []
            for mutation in mutations {
                let state = try await checkReceipt(mutation.requestID, bridge: coordinator.bridge, route: route)
                receiptEvidence.append([
                    "request_id": mutation.requestID.uuidString.lowercased(),
                    "action": mutation.action,
                    "receipt_state": state
                ])
            }
            guard receiptEvidence.count == 5, receiptEvidence.allSatisfy({ $0["receipt_state"] == "applied" }) else {
                throw BridgeFailure(message: "Captured scheduler mutation IDs did not all resolve to applied receipts")
            }
            evidence["mutation_receipts"] = receiptEvidence
            append("captured_create_edit_cancel_ids_resolve_through_managed_status", to: &evidence)

            let casOriginalMessage = "Native schedule CAS original \(token)"
            let casDraftMessage = "Native schedule CAS draft \(token)"
            let casExternalMessage = "Native schedule CAS external edit \(token)"
            let casDeadline = localMinuteText(offsetMinutes: 45)
            controls.viewMode.selectedSegment = 0
            try sendAction(controls.viewMode, named: "Pending scheduled messages")
            try await waitUntil("pending view before the concurrent-edit regression") {
                controls.viewMode.selectedSegment == 0 && controls.table.numberOfRows == 0
            }
            try click(controls.new, named: "New")
            try replaceMessage(casOriginalMessage, in: controls)
            try setDeadline(casDeadline, in: controls)
            try await waitUntil("concurrent-edit fixture create readiness") { controls.save.isEnabled }
            try click(controls.save, named: "Schedule Message")
            try await waitForRow(message: casOriginalMessage, deadline: casDeadline, version: 1, in: controls)

            let jobsReply = try await schedulerAction(
                ["action": "list", "limit": 100], bridge: coordinator.bridge, route: route)
            guard jobsReply.kind == "jobs",
                  let jobs = jobsReply.value["jobs"] as? [[String: Any]],
                  let casJob = jobs.first(where: { $0["message"] as? String == casOriginalMessage }),
                  let casJobIDText = casJob["id"] as? String,
                  let casJobID = UUID(uuidString: casJobIDText),
                  (casJob["version"] as? NSNumber)?.uint64Value == 1 else {
                throw BridgeFailure(message: "Concurrent-edit fixture was not present in the remote scheduler at version 1")
            }

            try replaceMessage(casDraftMessage, in: controls)
            try await waitUntil("dirty concurrent-edit draft readiness") { controls.save.isEnabled }
            let externalEdit = try await schedulerAction([
                "action": "edit",
                "job_id": casJobID.uuidString.lowercased(),
                "expected_version": 1,
                "patch": ["message": casExternalMessage]
            ], requestID: UUID(), bridge: coordinator.bridge, route: route)
            guard externalEdit.kind == "mutation",
                  let externalJob = externalEdit.value["job"] as? [String: Any],
                  externalJob["message"] as? String == casExternalMessage,
                  (externalJob["version"] as? NSNumber)?.uint64Value == 2 else {
                throw BridgeFailure(message: "Out-of-band scheduler edit did not advance the fixture to version 2")
            }

            try click(controls.refresh, named: "Refresh")
            try await waitUntil("refresh preserves the draft while showing the remote version 2 row") {
                guard let row = Self.rowIndex(message: casExternalMessage, in: controls.table) else { return false }
                return Self.visibleText(in: scheduleWindow).contains("Remote scheduler refreshed")
                    && controls.table.selectedRow == row
                    && Self.cellText(column: 3, row: row, in: controls.table) == "pending · v2"
                    && controls.message.string == casDraftMessage
                    && controls.save.isEnabled
            }

            try click(controls.save, named: "Save Changes")
            try await waitUntil("stale-version rejection for the retained editor baseline") {
                Self.visibleText(in: scheduleWindow).contains("version changed")
                    && controls.message.string == casDraftMessage
            }
            let authoritativeReply = try await schedulerAction([
                "action": "show",
                "job_id": casJobID.uuidString.lowercased()
            ], bridge: coordinator.bridge, route: route)
            guard authoritativeReply.kind == "job",
                  let authoritativeJob = authoritativeReply.value["job"] as? [String: Any],
                  authoritativeJob["id"] as? String == casJobID.uuidString.lowercased(),
                  authoritativeJob["message"] as? String == casExternalMessage,
                  authoritativeJob["state"] as? String == "pending",
                  (authoritativeJob["version"] as? NSNumber)?.uint64Value == 2 else {
                throw BridgeFailure(message: "Stale AppKit save changed the authoritative version 2 scheduler job")
            }
            let cleanupReply = try await schedulerAction([
                "action": "cancel",
                "job_id": casJobID.uuidString.lowercased(),
                "expected_version": 2
            ], requestID: UUID(), bridge: coordinator.bridge, route: route)
            guard cleanupReply.kind == "mutation",
                  let cleanupJob = cleanupReply.value["job"] as? [String: Any],
                  cleanupJob["state"] as? String == "cancelled",
                  (cleanupJob["version"] as? NSNumber)?.uint64Value == 3 else {
                throw BridgeFailure(message: "Concurrent-edit fixture could not be cancelled at its known version 2")
            }
            evidence["cas_refresh_rejection"] = [
                "original_version": 1,
                "remote_version": 2,
                "retained_message": casDraftMessage,
                "remote_message_after_rejection": casExternalMessage,
                "cleanup_version": 3
            ]
            append("dirty_refresh_retains_original_version_and_rejects_concurrent_overwrite", to: &evidence)
            evidence["state"] = "passed"
            try await writeEvidence(evidence, to: report)

            scheduleWindow.close()
            try await cleanUpRemoteWorkspace(remoteWorkspaceID, restoring: originalWorkspace, host: host,
                                             coordinator: coordinator)
        } catch {
            let primary = error
            if let scheduleWindow {
                let failureScreenshot = report.deletingLastPathComponent()
                    .appendingPathComponent("zmux-scheduled-messages.png")
                if scheduleWindow.isVisible {
                    do {
                        try capture(scheduleWindow, to: failureScreenshot)
                        evidence["failure_screenshot"] = failureScreenshot.lastPathComponent
                    } catch {
                        evidence["failure_screenshot_error"] = String(String(describing: error).prefix(512))
                    }
                } else {
                    evidence["failure_screenshot_error"] = "Scheduled Messages window was not visible at failure"
                }
            } else {
                evidence["failure_screenshot_error"] = "Scheduled Messages window was unavailable at failure"
            }
            scheduleWindow?.close()
            do {
                try await cleanUpRemoteWorkspace(remoteWorkspaceID, restoring: originalWorkspace, host: host,
                                                 coordinator: coordinator)
            } catch {
                FileHandle.standardError.write(Data("Scheduled messages smoke workspace cleanup failed: \(error.localizedDescription)\n".utf8))
            }
            evidence["state"] = "failed"
            evidence["failure"] = String(String(describing: primary).prefix(2048))
            try? await writeEvidence(evidence, to: report)
            throw primary
        }
    }

    private static func waitForRemoteWorkspace(named name: String,
                                              coordinator: ApplicationCoordinator) async throws -> Workspace {
        var found: Workspace?
        try await waitUntil("managed SSH workspace \(name)") {
            found = coordinator.snapshot?.model.workspaces.first(where: { $0.name == name })
            guard let found, let pane = found.root.panes.first,
                  let surface = pane.surfaces.first(where: { $0.id == pane.selectedSurface }),
                  let session = surface.sessionId,
                  let frame = coordinator.snapshot?.frames.first(where: { $0.sessionId == session }) else { return false }
            return surface.remote != nil && frame.snapshot?.state == "running" && !frame.reconnectable
        }
        guard let found else { throw BridgeFailure(message: "Managed SSH workspace did not appear") }
        return found
    }

    private static func route(coordinator: ApplicationCoordinator, workspaceID: UInt64) -> Route? {
        guard let snapshot = coordinator.snapshot,
              let workspace = snapshot.model.workspaces.first(where: { $0.id == workspaceID }),
              let pane = workspace.root.panes.first(where: { $0.id == workspace.focusedPane }),
              let surface = pane.surfaces.first(where: { $0.id == pane.selectedSurface && $0.kind == "terminal" }),
              surface.remote != nil, let session = surface.sessionId,
              let frame = snapshot.frames.first(where: { $0.sessionId == session }),
              frame.snapshot?.state == "running", !frame.reconnectable else { return nil }
        return Route(identityNamespace: snapshot.model.identityNamespace, surface: surface.id, session: session)
    }

    private static func cleanUpRemoteWorkspace(_ workspaceID: UInt64?, restoring original: UInt64?,
                                               host: MainWindowHost, coordinator: ApplicationCoordinator) async throws {
        if let workspaceID,
           coordinator.snapshot?.model.workspaces.contains(where: { $0.id == workspaceID }) == true {
            _ = try await host.nativeCLI(["close-workspace", String(workspaceID)], externalCaller: true)
        }
        if let original,
           coordinator.snapshot?.model.workspaces.contains(where: { $0.id == original }) == true {
            _ = try await host.nativeCLI(["select-workspace", String(original)], externalCaller: true)
            try await waitUntil("restoring the initial workspace") {
                coordinator.snapshot?.model.focusedWorkspace == original
                    && coordinator.snapshot?.model.workspaces.contains(where: { $0.id == workspaceID }) != true
            }
        }
    }

    private static func replaceMessage(_ value: String, in controls: Controls) throws {
        controls.window.makeFirstResponder(controls.message)
        let range = NSRange(location: 0, length: (controls.message.string as NSString).length)
        controls.message.setSelectedRange(range)
        controls.message.insertText(value, replacementRange: range)
        guard controls.message.string == value else {
            throw BridgeFailure(message: "Scheduled message editor did not retain the literal smoke text")
        }
    }

    private static func setDeadline(_ value: String, in controls: Controls) throws {
        controls.deadline.stringValue = value
        try sendAction(controls.deadline, named: "Scheduled local date and time")
    }

    private static func sendAction(_ control: NSControl, named name: String) throws {
        guard let action = control.action,
              NSApp.sendAction(action, to: control.target, from: control) else {
            throw BridgeFailure(message: "Could not invoke the \(name) control action")
        }
    }

    private static func click(_ button: NSButton, named name: String) throws {
        guard button.isEnabled else {
            throw BridgeFailure(message: "Scheduled messages smoke tried to click a disabled control: \(name)")
        }
        button.performClick(nil)
    }

    private static func assertLayoutGeometryAndVisibleText(_ controls: Controls,
                                                           expectedMessage: String) throws {
        guard let content = controls.window.contentView else {
            throw BridgeFailure(message: "Scheduled messages window has no content view for its layout check")
        }
        content.layoutSubtreeIfNeeded()
        controls.split.layoutSubtreeIfNeeded()
        controls.queueScroll.layoutSubtreeIfNeeded()
        controls.editorScroll.layoutSubtreeIfNeeded()
        controls.table.layoutSubtreeIfNeeded()
        controls.message.layoutSubtreeIfNeeded()

        let panes = controls.split.arrangedSubviews
        guard panes.count == 2 else {
            throw BridgeFailure(message: "Scheduled messages split view does not contain both panes")
        }
        let splitRect = rect(controls.split, in: content)
        let queuePaneRect = rect(panes[0], in: controls.split)
        let editorPaneRect = rect(panes[1], in: controls.split)
        guard contains(content.bounds, splitRect),
              contains(controls.split.bounds, queuePaneRect),
              contains(controls.split.bounds, editorPaneRect),
              !queuePaneRect.intersects(editorPaneRect) else {
            throw BridgeFailure(message: "Scheduled messages panes escape or overlap their split view")
        }

        let queueScrollRect = rect(controls.queueScroll, in: panes[0])
        let editorScrollRect = rect(controls.editorScroll, in: panes[1])
        guard contains(panes[0].bounds, queueScrollRect),
              contains(panes[1].bounds, editorScrollRect),
              !rect(controls.queueScroll, in: content).intersects(rect(controls.editorScroll, in: content)),
              controls.editorScroll.contentView.bounds.width >= 300 else {
            throw BridgeFailure(message: "Scheduled messages scroll views escape their panes, overlap, or leave the editor too narrow")
        }

        let contentHeight = content.bounds.height
        let headerHeight = content.bounds.maxY - controls.split.frame.maxY
        guard contentHeight > 0, headerHeight >= 0, headerHeight <= contentHeight * 0.35 else {
            throw BridgeFailure(message: "Scheduled messages header consumes too much vertical space")
        }

        guard controls.table.tableColumns.last?.title == "State",
              let stateColumn = controls.table.tableColumns.indices.last else {
            throw BridgeFailure(message: "Scheduled messages queue has no final State column")
        }
        let stateColumnRect = controls.table.rect(ofColumn: stateColumn)
        let tableVisibleRect = controls.table.visibleRect
        guard stateColumnRect.minX >= tableVisibleRect.minX - 1,
              stateColumnRect.maxX <= tableVisibleRect.maxX + 1 else {
            throw BridgeFailure(message: "Scheduled messages State column is outside the visible queue viewport")
        }

        guard controls.message.string == expectedMessage,
              let textContainer = controls.message.textContainer,
              let layoutManager = controls.message.layoutManager else {
            throw BridgeFailure(message: "Scheduled messages editor does not contain the selected history message")
        }
        layoutManager.ensureLayout(for: textContainer)
        let viewportInTextView = controls.editorScroll.contentView.convert(
            controls.editorScroll.contentView.bounds, to: controls.message
        ).intersection(controls.message.bounds)
        let viewportInTextContainer = viewportInTextView.offsetBy(
            dx: -controls.message.textContainerOrigin.x,
            dy: -controls.message.textContainerOrigin.y
        )
        let visibleGlyphs = layoutManager.glyphRange(forBoundingRect: viewportInTextContainer,
                                                     in: textContainer)
        let visibleCharacters = layoutManager.characterRange(forGlyphRange: visibleGlyphs,
                                                             actualGlyphRange: nil)
        let expectedNSString = expectedMessage as NSString
        guard visibleGlyphs.length > 0,
              visibleCharacters.length > 0,
              NSMaxRange(visibleCharacters) <= expectedNSString.length,
              expectedNSString.substring(with: visibleCharacters).contains(where: { !$0.isWhitespace }) else {
            throw BridgeFailure(message: "Scheduled messages editor viewport contains no visible message glyphs")
        }
    }

    private static func rect(_ view: NSView, in ancestor: NSView) -> NSRect {
        view.convert(view.bounds, to: ancestor).standardized
    }

    private static func contains(_ outer: NSRect, _ inner: NSRect) -> Bool {
        outer.insetBy(dx: -1, dy: -1).contains(inner)
    }

    private static func createPrivateMarker(at marker: URL, privateRoot: URL) throws {
        guard marker.deletingLastPathComponent().standardizedFileURL == privateRoot.standardizedFileURL,
              !FileManager.default.fileExists(atPath: marker.path),
              FileManager.default.createFile(atPath: marker.path, contents: Data(),
                                             attributes: [.posixPermissions: NSNumber(value: 0o600)]) else {
            throw BridgeFailure(message: "Could not create the one-shot scheduler response-drop marker inside the private fixture root")
        }
        let attributes = try FileManager.default.attributesOfItem(atPath: marker.path)
        let permissions = (attributes[.posixPermissions] as? NSNumber)?.uint16Value
        guard permissions == 0o600 else {
            try? FileManager.default.removeItem(at: marker)
            throw BridgeFailure(message: "Scheduler response-drop marker does not have mode 0600")
        }
    }

    private static func capture(_ window: NSWindow, to destination: URL) throws {
        guard let content = window.contentView,
              let image = content.bitmapImageRepForCachingDisplay(in: content.bounds) else {
            throw BridgeFailure(message: "Scheduled messages history screenshot is unavailable")
        }
        content.layoutSubtreeIfNeeded()
        content.displayIfNeeded()
        content.effectiveAppearance.performAsCurrentDrawingAppearance {
            content.cacheDisplay(in: content.bounds, to: image)
        }
        guard let png = image.representation(using: .png, properties: [:]), png.count > 4096 else {
            throw BridgeFailure(message: "Scheduled messages history screenshot is empty")
        }
        try png.write(to: destination, options: .atomic)
    }

    private static func waitForRow(message: String, deadline: String, version: Int,
                                   in controls: Controls) async throws {
        try await waitUntil("remote queue row \(message)") {
            guard let row = rowIndex(message: message, in: controls.table) else { return false }
            let deadlineText = cellText(column: 2, row: row, in: controls.table)
            let stateText = cellText(column: 3, row: row, in: controls.table) ?? ""
            return deadlineText == deadline && stateText == "pending · v\(version)"
        }
    }

    private static func rowIndex(message: String, in table: NSTableView) -> Int? {
        guard table.numberOfColumns > 0 else { return nil }
        for row in 0..<table.numberOfRows where cellText(column: 0, row: row, in: table) == message {
            return row
        }
        return nil
    }

    private static func cellText(column: Int, row: Int, in table: NSTableView) -> String? {
        guard table.tableColumns.indices.contains(column), table.rows(in: table.bounds).contains(row) else { return nil }
        table.layoutSubtreeIfNeeded()
        return (table.view(atColumn: column, row: row, makeIfNecessary: true) as? NSTableCellView)?.textField?.stringValue
    }

    private static func localMinuteText(offsetMinutes: Int) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone.current
        formatter.dateFormat = "yyyy-MM-dd HH:mm"
        return formatter.string(from: Date().addingTimeInterval(TimeInterval(offsetMinutes * 60)))
    }

    private static func capturedMutations(at url: URL,
                                          expectedCreateMessage: String) async throws -> [MutationRecord] {
        let log = try await Task.detached(priority: .utility) {
            let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
            let mode = (attributes[.posixPermissions] as? NSNumber)?.uint16Value ?? 0xffff
            let data = try Data(contentsOf: url)
            guard data.count <= 1_048_576 else {
                throw BridgeFailure(message: "Scheduler SSH request log exceeds its 1 MiB bound")
            }
            return CapturedLog(data: data, permissions: mode)
        }.value
        guard log.permissions & 0o077 == 0 else {
            throw BridgeFailure(message: "Scheduler SSH request log is not private to the fixture account")
        }
        var mutations: [MutationRecord] = []
        for line in String(decoding: log.data, as: UTF8.self).split(whereSeparator: \.isNewline) {
            guard let object = try JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any],
                  let action = object["action"] as? [String: Any],
                  let name = action["action"] as? String,
                  ["create", "edit", "cancel"].contains(name) else { continue }
            guard let requestIDText = object["request_id"] as? String,
                  let requestID = UUID(uuidString: requestIDText) else {
                throw BridgeFailure(message: "Captured scheduler mutation omitted its durable request UUID")
            }
            let patch = action["patch"] as? [String: Any] ?? [:]
            mutations.append(MutationRecord(
                requestID: requestID,
                action: name,
                patchKeys: patch.keys.sorted(),
                jobID: action["job_id"] as? String,
                message: (name == "create" ? action["message"] as? String : patch["message"] as? String)
            ))
        }
        guard mutations.first?.message == expectedCreateMessage else {
            throw BridgeFailure(message: "Captured create request did not match the message entered in AppKit")
        }
        return mutations
    }

    private static func validateMutationSequence(_ mutations: [MutationRecord]) throws {
        let expectedActions = ["create", "edit", "edit", "edit", "cancel"]
        guard mutations.map(\.action) == expectedActions,
              mutations.count == 5,
              mutations[1].patchKeys == ["message"],
              mutations[2].patchKeys == ["deadline"],
              mutations[3].patchKeys == ["deadline", "message"] else {
            throw BridgeFailure(message: "Captured SSH mutations did not cover create, single-field edits, combined edit, and cancel")
        }
        let editedJobIDs = Set(mutations.dropFirst().compactMap(\.jobID))
        guard editedJobIDs.count == 1,
              mutations.map(\.requestID).count == Set(mutations.map(\.requestID)).count else {
            throw BridgeFailure(message: "Scheduler edit/cancel requests did not retain one job identity and unique request IDs")
        }
    }

    private static func schedulerAction(_ action: [String: Any], requestID: UUID? = nil,
                                        bridge: NativeBridge, route: Route) async throws -> SchedulerActionReply {
        let requestIDValue: Any = requestID.map { $0.uuidString.lowercased() as Any } ?? NSNull()
        let request = try JSONSerialization.data(withJSONObject: [
            "protocol_version": 1,
            "request_id": requestIDValue,
            "action": action
        ], options: [.sortedKeys])
        let response = try await bridge.scheduleRequest(identityNamespace: route.identityNamespace,
            surface: route.surface, expectedSession: route.session, request: request)
        guard let wrapper = try JSONSerialization.jsonObject(with: response) as? [String: Any],
              let ok = wrapper["ok"] as? Bool,
              let outcomeUnknown = wrapper["outcome_unknown"] as? Bool else {
            throw BridgeFailure(message: "Managed SSH scheduler returned an invalid bridge wrapper")
        }
        guard ok else {
            let error = wrapper["error"] as? [String: Any] ?? [:]
            throw BridgeFailure(message: error["message"] as? String ?? "Managed SSH scheduler request failed")
        }
        guard !outcomeUnknown,
              let reply = wrapper["reply"] as? [String: Any],
              let result = reply["result"] as? [String: Any],
              let status = result["status"] as? String,
              let data = result["data"] as? [String: Any] else {
            throw BridgeFailure(message: "Managed SSH scheduler returned an incomplete or uncertain reply")
        }
        guard status == "success" else {
            throw BridgeFailure(message: data["message"] as? String ?? "Managed SSH scheduler rejected the request")
        }
        guard let kind = data["kind"] as? String else {
            throw BridgeFailure(message: "Managed SSH scheduler reply omitted its result kind")
        }
        return SchedulerActionReply(kind: kind, value: data["value"] as? [String: Any] ?? data)
    }

    private static func checkReceipt(_ requestID: UUID, bridge: NativeBridge, route: Route) async throws -> String {
        let request = try JSONSerialization.data(withJSONObject: [
            "protocol_version": 1,
            "request_id": NSNull(),
            "action": ["action": "status", "receipt_request_id": requestID.uuidString.lowercased()]
        ], options: [.sortedKeys])
        let response = try await bridge.scheduleRequest(identityNamespace: route.identityNamespace,
            surface: route.surface, expectedSession: route.session, request: request)
        guard let wrapper = try JSONSerialization.jsonObject(with: response) as? [String: Any],
              wrapper["ok"] as? Bool == true,
              wrapper["outcome_unknown"] as? Bool == false,
              let reply = wrapper["reply"] as? [String: Any],
              let result = reply["result"] as? [String: Any],
              result["status"] as? String == "success",
              let data = result["data"] as? [String: Any],
              data["kind"] as? String == "request_status",
              let receipt = data["receipt"] as? [String: Any],
              receipt["request_id"] as? String == requestID.uuidString.lowercased(),
              let state = receipt["state"] as? String else {
            throw BridgeFailure(message: "Managed SSH status returned no matching scheduler receipt for \(requestID)")
        }
        return state
    }

    private static func append(_ check: String, to evidence: inout [String: Any]) {
        var checks = evidence["checks"] as? [String] ?? []
        checks.append(check)
        evidence["checks"] = checks
    }

    private static func writeEvidence(_ evidence: [String: Any], to report: URL) async throws {
        let data = try JSONSerialization.data(withJSONObject: evidence, options: [.prettyPrinted, .sortedKeys])
        try await Task.detached(priority: .utility) {
            try data.write(to: report, options: .atomic)
        }.value
    }

    private static func visibleText(in window: NSWindow) -> String {
        guard let content = window.contentView else { return "" }
        return descendants(content).compactMap { ($0 as? NSTextField)?.stringValue }.joined(separator: "\n")
    }

    private static func descendants(_ view: NSView) -> [NSView] {
        [view] + view.subviews.flatMap(descendants)
    }

    private static func waitUntil(_ description: String, timeout: Duration = .seconds(30),
                                  _ predicate: @MainActor () -> Bool) async throws {
        let deadline = ContinuousClock.now + timeout
        while ContinuousClock.now < deadline {
            if predicate() { return }
            try await Task.sleep(for: .milliseconds(32))
        }
        throw BridgeFailure(message: "Timed out waiting for \(description)")
    }
}

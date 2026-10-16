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
    private struct UTF8Evidence: Encodable, Sendable {
        let name: String
        let expected: [UInt8]
        let observed: [UInt8]
    }
    private struct MountedControls: Encodable, Sendable {
        let operationStatus: String
        let replyUTF8: [UInt8]
        let sendEnabled: Bool
        let checkStatusEnabled: Bool
    }
    private struct HeldCapacity: Sendable { let core: UInt64; let buffers: [UInt64] }
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
        var savedReplies: [UUID] = []
        var pendingRequestId: UUID?
        var unicodeBytes: [UTF8Evidence] = []
        var mountedControls: MountedControls?
        var knownLockRefusal: MountedControls?
        var capacityRefusal: MountedControls?
        var knownStatusChecks = 0
        var capacityStatusChecks = 0
        var recoveryStatusChecks = 0
    }
    private struct Message: Decodable {
        struct Address: Decodable { let surface_id: UUID }
        struct Sender: Decodable { let surface_id: UUID? }
        let id: UUID
        let request_id: UUID
        let reply_to: UUID?
        let thread_id: UUID
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
        var heldCapacity: HeldCapacity?
        var observedContent: NSView?
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
                  listed.messages.contains(where: { $0.id == primary.id && $0.state == "queued" && $0.body.utf8.elementsEqual(body.utf8) &&
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
            observedContent = content
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
                return views.contains { exact(($0 as? NSTextView)?.string, body) || exact(($0 as? NSTextField)?.stringValue, body) } &&
                    views.compactMap { $0 as? NSTextView }.contains { $0.isEditable && $0.accessibilityLabel() == "Reply" && inbox.firstResponder === $0 }
            }
            let rendered = descendants(content)
            guard rendered.contains(where: { exact(($0 as? NSTextView)?.string, body) || exact(($0 as? NSTextField)?.stringValue, body) }),
                  let reply = rendered.compactMap({ $0 as? NSTextView }).first(where: { $0.isEditable && $0.accessibilityLabel() == "Reply" }),
                  inbox.firstResponder === reply else { throw failure("Inbox body or actual reply first responder differs") }
            evidence.checks.append("actual_source_owned_inbox_exact_body_and_reply_responder")
            // Distinct durable records have canonically equivalent text but different
            // UTF-8 bytes. Actual row actions must replace the mounted body exactly.
            let decomposedBody = "Canonical body e\u{301} · retained bytes"
            let composedBody = "Canonical body é · retained bytes"
            guard !decomposedBody.utf8.elementsEqual(composedBody.utf8) else { throw failure("Independent Unicode oracles are not distinct") }
            let decomposed = try await send(source, from: otherChild, to: sourceChild.surface, body: decomposedBody)
            let composed = try await send(source, from: otherChild, to: sourceChild.surface, body: composedBody)
            guard decomposed.body.utf8.elementsEqual(decomposedBody.utf8), composed.body.utf8.elementsEqual(composedBody.utf8) else {
                throw failure("Independent CLI canonical body byte prerequisites differ")
            }
            evidence.checks.append("independent_cli_canonical_equivalent_bodies_have_distinct_utf8")
            evidence.stage = "unicode_body"
            try await wait(coordinator, "both exact canonical message UUID rows") { _ in
                let ids = Set(descendants(content).compactMap { $0.accessibilityIdentifier() })
                return ids.contains(decomposed.id.uuidString.lowercased()) && ids.contains(composed.id.uuidString.lowercased())
            }
            guard let decomposedRow = descendants(content).compactMap({ $0 as? NSButton }).first(where: { $0.accessibilityIdentifier() == decomposed.id.uuidString.lowercased() }),
                  let composedRow = descendants(content).compactMap({ $0 as? NSButton }).first(where: { $0.accessibilityIdentifier() == composed.id.uuidString.lowercased() }),
                  let mountedBody = descendants(content).compactMap({ $0 as? NSTextView }).first(where: { $0.accessibilityLabel() == "Message body" }) else {
                throw failure("Actual canonical body rows/document unavailable")
            }
            decomposedRow.performClick(nil)
            try await wait(coordinator, "selected decomposed body document") { _ in
                decomposedRow.state == .on && mountedBody.string.utf8.elementsEqual(decomposedBody.utf8)
            }
            evidence.unicodeBytes.append(UTF8Evidence(name: "body_decomposed", expected: Array(decomposedBody.utf8), observed: Array(mountedBody.string.utf8)))
            composedRow.performClick(nil)
            guard composedRow.state == .on else { throw failure("Actual composed row selection did not commit") }
            evidence.unicodeBytes.append(UTF8Evidence(name: "body_precomposed", expected: Array(composedBody.utf8), observed: Array(mountedBody.string.utf8)))
            guard mountedBody.string.utf8.elementsEqual(composedBody.utf8) else {
                throw failure("Selected canonical-equivalent message did not preserve exact UTF-8 body")
            }
            evidence.checks.append("native_row_selection_preserves_distinct_canonical_equivalent_utf8_bodies")
            row.performClick(nil)
            evidence.stage = "journey"
            let draft = "  Native reply α界 e\u{301}🙂\t\nkept whitespace  \n"
            reply.insertText(draft, replacementRange: NSRange(location: 0, length: (reply.string as NSString).length))
            guard reply.string.utf8.elementsEqual(draft.utf8) else { throw failure("Actual reply editing did not retain exact text") }
            let arrival = try await send(source, from: otherChild, to: sourceChild.surface, body: "Later arrival stays queued")
            guard let refresh = button("Refresh", in: content), let submit = button("Send Reply", in: content),
                  let markRead = button("Mark Read", in: content), let status = button("Check Status", in: content),
                  let receiving = button("Allow receiving", in: content),
                  let scopes = descendants(content).compactMap({ $0 as? NSPopUpButton }).first(where: { $0.accessibilityLabel() == "Receiving settings scope" }) else {
                throw failure("Mounted inbox production controls unavailable")
            }
            refresh.performClick(nil)
            try await wait(coordinator, "real new arrival rendered without losing draft") { _ in
                descendants(content).contains { $0.accessibilityIdentifier() == arrival.id.uuidString.lowercased() } && reply.string.utf8.elementsEqual(draft.utf8)
            }
            inbox.orderOut(nil)
            other.window.makeKeyAndOrderFront(nil)
            try await wait(coordinator, "foreign focus after hidden draft") { state in
                NSApp.keyWindow === other.window && state.model.focusedWindow == otherID && state.observedKeyWindow == otherID
            }
            menu.performActionForItem(at: index)
            try await wait(coordinator, "same reopened inbox draft and responder") { _ in
                inbox.isVisible && inbox.parent === source.window && inbox.firstResponder === reply && reply.string.utf8.elementsEqual(draft.utf8)
            }
            evidence.checks.append("draft_survives_real_arrival_refresh_hide_and_foreign_focus")
            evidence.children = try await counters(root, children: children, nonce: nonce, livePIDs: livePIDs)
            let submittedDraft = "  Native reply α界 é🙂\t\nkept whitespace  \n"
            reply.insertText(submittedDraft, replacementRange: NSRange(location: 0, length: (reply.string as NSString).length))
            evidence.unicodeBytes.append(UTF8Evidence(name: "editor_precomposed", expected: Array(submittedDraft.utf8), observed: Array(reply.string.utf8)))
            guard reply.string.utf8.elementsEqual(submittedDraft.utf8) else { throw failure("Actual precomposed draft editor bytes differ") }
            guard submit.isEnabled else { throw failure("Reply submit disabled despite a live original sender and draft") }
            submit.performClick(nil)
            try submitKey(in: inbox)
            let laterEdit = submittedDraft + "edited while saving"
            reply.insertText("edited while saving", replacementRange: NSRange(location: (reply.string as NSString).length, length: 0))
            other.window.makeKeyAndOrderFront(nil)
            let replies = try await inboxUntil(source, "single durable native reply") { rows in
                rows.messages.filter { $0.reply_to == primary.id && $0.body.utf8.elementsEqual(submittedDraft.utf8) }.count == 1
            }
            guard let saved = replies.messages.first(where: { $0.reply_to == primary.id && $0.body.utf8.elementsEqual(submittedDraft.utf8) }),
                  saved.sender.surface_id == sourceChild.surface, saved.recipient.surface_id == otherChild.surface,
                  saved.thread_id == primary.thread_id, saved.state == "queued", saved.body.utf8.elementsEqual(submittedDraft.utf8),
                  replies.messages.filter({ $0.reply_to == primary.id }).count == 1 else { throw failure("Reply lost original parent/thread/sender/recipient or duplicated") }
            try await wait(coordinator, "saved receipt preserves newer draft revision") { _ in
                !status.isEnabled && submit.isEnabled && reply.string.utf8.elementsEqual(laterEdit.utf8) && operationStatus(content).hasPrefix("Saved")
            }
            evidence.unicodeBytes.append(UTF8Evidence(name: "saved_precomposed", expected: Array(submittedDraft.utf8), observed: Array(saved.body.utf8)))
            evidence.savedReplies.append(saved.id)
            evidence.checks.append("button_and_submit_key_save_one_exact_reply_and_keep_later_edit")
            let beforeRead = try JSONDecoder().decode(Inbox.self, from: await cli(source, ["agent", "inbox"]))
            guard beforeRead.messages.first(where: { $0.id == primary.id })?.state == "queued",
                  beforeRead.messages.first(where: { $0.id == arrival.id })?.state == "queued" else { throw failure("Focus or selection implicitly marked messages read") }
            markRead.performClick(nil)
            _ = try await inboxUntil(source, "exact selected message read only") { rows in
                rows.messages.first(where: { $0.id == primary.id })?.state == "read" &&
                rows.messages.first(where: { $0.id == arrival.id })?.state == "queued"
            }
            evidence.checks.append("explicit_read_leaves_later_arrival_queued")
            try await wait(coordinator, "read receipt settled") { _ in operationStatus(content).hasPrefix("Saved") && submit.isEnabled }
            scopes.selectItem(at: 2)
            guard let scopeAction = scopes.action, NSApp.sendAction(scopeAction, to: scopes.target, from: scopes) else { throw failure("Actual receiving scope action unavailable") }
            try await wait(coordinator, "global configured receiving on") { _ in receiving.isEnabled && receiving.state == .on }
            receiving.performClick(nil)
            try await settingsUntil(source, enabled: false)
            try await wait(coordinator, "native receiving off receipt") { _ in receiving.isEnabled && receiving.state == .off && operationStatus(content).contains("settings saved") }
            submit.performClick(nil)
            try await wait(coordinator, "native opt-out refusal keeps draft") { _ in
                operationStatus(content).contains("messaging disabled") && submit.isEnabled && reply.string.utf8.elementsEqual(laterEdit.utf8)
            }
            let failed = try await inboxUntil(source, "queued messages failed by explicit receiving off") { rows in
                rows.messages.first(where: { $0.id == arrival.id })?.state == "failed" &&
                rows.messages.first(where: { $0.id == saved.id })?.state == "failed"
            }
            guard failed.messages.count == 7, failed.messages.first(where: { $0.id == saved.id })?.failure == "messaging_disabled" else { throw failure("Refused native send mutated inbox or failed messages lost their reason") }
            try await wait(coordinator, "configured off after refused reply") { _ in receiving.isEnabled && receiving.state == .off }
            receiving.performClick(nil)
            try await settingsUntil(source, enabled: true)
            let restored = try JSONDecoder().decode(Inbox.self, from: await cli(source, ["agent", "inbox"]))
            guard restored.messages.first(where: { $0.id == arrival.id })?.state == "failed",
                  restored.messages.first(where: { $0.id == saved.id })?.state == "failed",
                  restored.messages.first(where: { $0.id == closed.id })?.failure == "recipient_closed" else { throw failure("Turning receiving on resurrected retained failures") }
            evidence.checks.append("native_receiving_settings_refuse_send_preserve_draft_and_never_resurrect")
            try await wait(coordinator, "receiving-on receipt settled before uncertainty") { _ in submit.isEnabled && operationStatus(content).contains("settings saved") }
            // A single held lock must produce the storage owner's authoritative
            // precommit refusal, not fabricated uncertainty or an automatic retry.
            try await signal(root.appendingPathComponent("lock-request"))
            try await file(root.appendingPathComponent("lock-held"))
            guard submit.isEnabled else { throw failure("Reply not available before real lock contention") }
            submit.performClick(nil)
            try submitKey(in: inbox)
            evidence.knownStatusChecks = try await checkStatusUntil(status, "known journal lock refusal retains draft without pending status", beforeCheck: {
                guard !submit.isEnabled, reply.string.utf8.elementsEqual(laterEdit.utf8),
                      operationStatus(content).split(separator: " ").contains(where: { UUID(uuidString: String($0)) != nil }) else {
                    throw failure("Background refresh replaced the uncertain operation recovery UUID or draft")
                }
            }, predicate: {
                operationStatus(content) == "another instance is reserving session generations; retry" &&
                submit.isEnabled && !status.isEnabled && reply.string.utf8.elementsEqual(laterEdit.utf8)
            })
            evidence.knownLockRefusal = mountedControls(content)
            try await signal(root.appendingPathComponent("lock-release"))
            try await file(root.appendingPathComponent("lock-ended"))
            let refused = try JSONDecoder().decode(Inbox.self, from: await cli(source, ["agent", "inbox"]))
            guard refused.messages.count == 7, refused.messages.filter({ $0.reply_to == primary.id }).count == 1 else {
                throw failure("Known lock refusal created an extra durable reply")
            }
            evidence.checks.append("real_journal_lock_known_refusal_preserves_exact_draft")
            refresh.performClick(nil)
            try await wait(coordinator, "receiving controls recovered after known lock refusal") { _ in receiving.isEnabled && submit.isEnabled }
            // The independent owner queues five actual public reads behind another
            // flock. Retained admission/status receipts prove the backlog; elapsed
            // sleeps never establish that the original mutation was accepted.
            try await signal(root.appendingPathComponent("pending-lock-request"))
            try await file(root.appendingPathComponent("pending-lock-held.json"))
            try await file(root.appendingPathComponent("pending-controls-admitted.json"))
            guard submit.isEnabled else { throw failure("Reply not available before observed public backlog") }
            submit.performClick(nil)
            try submitKey(in: inbox)
            try await wait(coordinator, "real journal wait becomes explicit Check Status") { _ in
                status.isEnabled && !submit.isEnabled && reply.string.utf8.elementsEqual(laterEdit.utf8) &&
                operationStatus(content).split(separator: " ").contains { UUID(uuidString: String($0)) != nil }
            }
            guard let operation = operationStatus(content).split(separator: " ").compactMap({ UUID(uuidString: String($0)) }).last else { throw failure("Uncertain operation did not expose its original UUID") }
            evidence.pendingRequestId = operation
            let operationData = try JSONSerialization.data(withJSONObject: ["requestId": operation.uuidString.lowercased()])
            let operationPath = root.appendingPathComponent("pending-operation.json")
            try await Task.detached { try operationData.write(to: operationPath, options: .atomic) }.value
            try await file(root.appendingPathComponent("pending-public-receipts.json"))
            evidence.checks.append("public_owner_pending_original_and_queued_controls")
            status.performClick(nil)
            try await wait(coordinator, "pending Check Status retains exact UUID and draft") { _ in
                status.isEnabled && operationStatus(content).contains(operation.uuidString.lowercased()) && reply.string.utf8.elementsEqual(laterEdit.utf8) && !submit.isEnabled
            }
            heldCapacity = try await reserveCapacity()
            evidence.capacityStatusChecks = try await checkStatusUntil(status, "rejected status transport cannot unlock original uncertain send", minimumChecks: 1, beforeCheck: {
                guard !submit.isEnabled, reply.string.utf8.elementsEqual(laterEdit.utf8),
                      operationStatus(content).contains(operation.uuidString.lowercased()) else {
                    throw failure("Status refusal unlocked or replaced the original uncertain send")
                }
            }, predicate: {
                status.isEnabled && !submit.isEnabled && reply.string.utf8.elementsEqual(laterEdit.utf8) &&
                operationStatus(content).contains("(-5)") && operationStatus(content).contains(operation.uuidString.lowercased())
            })
            evidence.capacityRefusal = mountedControls(content)
            if let heldCapacity { try await releaseCapacity(heldCapacity) }
            heldCapacity = nil
            evidence.checks.append("status_transport_capacity_refusal_preserves_original_uncertain_request")
            try await signal(root.appendingPathComponent("pending-lock-release"))
            try await file(root.appendingPathComponent("pending-lock-ended.json"))
            evidence.recoveryStatusChecks = try await checkStatusUntil(status, "original UUID resolves saved and clears unchanged draft", minimumChecks: 1, beforeCheck: {
                guard !submit.isEnabled, reply.string.utf8.elementsEqual(laterEdit.utf8),
                      operationStatus(content).contains(operation.uuidString.lowercased()) else {
                    throw failure("Original uncertain send changed before saved recovery")
                }
            }, predicate: {
                reply.string.isEmpty && operationStatus(content).hasPrefix("Saved") && !status.isEnabled
            })
            let recovered = try await inboxUntil(source, "one original durable reply after real lock release") { rows in
                rows.messages.contains { $0.request_id == operation && $0.reply_to == primary.id && $0.body.utf8.elementsEqual(laterEdit.utf8) }
            }
            guard recovered.messages.count == 8,
                  recovered.messages.filter({ $0.request_id == operation }).count == 1,
                  let recoveredReply = recovered.messages.first(where: { $0.request_id == operation }),
                  recoveredReply.sender.surface_id == sourceChild.surface,
                  recoveredReply.recipient.surface_id == otherChild.surface,
                  recoveredReply.thread_id == primary.thread_id, recoveredReply.state == "queued" else {
                throw failure("Uncertain native request was replayed, retargeted or falsely delivered")
            }
            evidence.savedReplies.append(recoveredReply.id)
            evidence.checks.append("real_journal_lock_uncertainty_retains_uuid_and_check_status_saves_once")
            evidence.children = try await counters(root, children: children, nonce: nonce, livePIDs: livePIDs)
            evidence.checks.append("independent_pty_progress_after_all_ui_actions_zero_input")
            inbox.makeKeyAndOrderFront(nil)
            try await capture(inbox, to: report.deletingLastPathComponent().appendingPathComponent("native-agent-inbox.png"))
            try await coordinator.shutdown()
            evidence.stage = "complete"; evidence.state = "complete"
            try await save(evidence, to: report)
        } catch {
            if let heldCapacity {
                do { try await releaseCapacity(heldCapacity) }
                catch { FileHandle.standardError.write(Data("Capacity cleanup failed: \(error)\n".utf8)) }
            }
            // Release only this fixture's independently owned lock holder before
            // shared shutdown drains any accepted write. Never cancel/replay it.
            do {
                try await signal(root.appendingPathComponent("lock-release"))
                try await signal(root.appendingPathComponent("pending-lock-release"))
            }
            catch { FileHandle.standardError.write(Data("Lock release signal failed: \(error)\n".utf8)) }
            evidence.state = "failed"; evidence.failure = error.localizedDescription
            if let content = observedContent { evidence.mountedControls = mountedControls(content) }
            do { evidence.children = try await counters(root, children: children, nonce: nonce, livePIDs: livePIDs) }
            catch { evidence.failure = (evidence.failure ?? "") + "; counter verification: " + error.localizedDescription }
            try await save(evidence, to: report)
            throw error
        }
    }

    private static func reserveCapacity() async throws -> HeldCapacity {
        try await Task.detached {
            var core = zmux_core_create()
            let creationDeadline = ContinuousClock.now + .seconds(3)
            while core.status == -4 && ContinuousClock.now < creationDeadline {
                try await Task.sleep(for: .milliseconds(5)); core = zmux_core_create()
            }
            guard core.status == 0 else { throw failure("Fixture could not create its independent public ABI core") }
            let snapshot = Data("{\"version\":1,\"command\":{\"method\":\"snapshot\"}}".utf8)
            var buffers: [UInt64] = []
            do {
                let deadline = ContinuousClock.now + .seconds(3)
                while buffers.count < 16, ContinuousClock.now < deadline {
                    let result = snapshot.withUnsafeBytes { bytes in
                        zmux_core_dispatch(core.value, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
                    }
                    if result.status == 0 { buffers.append(result.value) }
                    else if result.status != -4 && result.status != -5 { throw failure("Independent ABI reservation failed") }
                    if buffers.count < 16 { try await Task.sleep(for: .milliseconds(5)) }
                }
                guard buffers.count == 16 else { throw failure("Fixture did not own all response capacity") }
                return HeldCapacity(core: core.value, buffers: buffers)
            } catch {
                try await releaseCapacity(HeldCapacity(core: core.value, buffers: buffers))
                throw error
            }
        }.value
    }
    nonisolated private static func releaseCapacity(_ held: HeldCapacity) async throws {
        for (isCore, token) in held.buffers.map({ (false, $0) }) + [(true, held.core)] {
            let deadline = ContinuousClock.now + .seconds(3)
            var status: Int32
            repeat {
                status = isCore ? zmux_core_destroy(token) : zmux_buffer_release(token)
                if status == -4 { try await Task.sleep(for: .milliseconds(5)) }
            } while status == -4 && ContinuousClock.now < deadline
            guard status == 0 else { throw failure("Fixture response/core ownership cleanup failed: \(status)") }
        }
    }
    nonisolated private static func exact(_ value: String?, _ expected: String) -> Bool {
        value?.utf8.elementsEqual(expected.utf8) == true
    }
    private static func button(_ title: String, in content: NSView) -> NSButton? {
        descendants(content).compactMap { $0 as? NSButton }.first { $0.title == title || $0.accessibilityLabel() == title }
    }
    private static func mountedControls(_ content: NSView) -> MountedControls {
        let editor = descendants(content).compactMap { $0 as? NSTextView }.first { $0.isEditable && $0.accessibilityLabel() == "Reply" }
        return MountedControls(operationStatus: operationStatus(content), replyUTF8: Array((editor?.string ?? "").utf8),
            sendEnabled: button("Send Reply", in: content)?.isEnabled == true,
            checkStatusEnabled: button("Check Status", in: content)?.isEnabled == true)
    }
    private static func operationStatus(_ content: NSView) -> String {
        descendants(content).compactMap { $0 as? NSTextField }.first { $0.accessibilityLabel() == "Message operation status" }?.stringValue ?? ""
    }
    private static func submitKey(in window: NSWindow) throws {
        guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.command],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
            characters: "\r", charactersIgnoringModifiers: "\r", isARepeat: false, keyCode: 36) else { throw failure("Submit key event unavailable") }
        // A disabled button may reject this duplicate key equivalent. It must
        // never create a second mutation through a separate keyboard path.
        _ = window.performKeyEquivalent(with: event)
    }
    private static func checkStatusUntil(_ button: NSButton, _ description: String, minimumChecks: Int = 0,
                                         beforeCheck: () throws -> Void, predicate: () -> Bool) async throws -> Int {
        let deadline = ContinuousClock.now + .seconds(8)
        var checks = 0
        while ContinuousClock.now < deadline {
            if checks >= minimumChecks, predicate() { return checks }
            if button.isEnabled {
                try beforeCheck()
                button.performClick(nil); checks += 1
            }
            try await Task.sleep(for: .milliseconds(32))
        }
        throw failure("Timed out awaiting " + description)
    }
    private static func inboxUntil(_ host: MainWindowHost, _ description: String, predicate: (Inbox) -> Bool) async throws -> Inbox {
        let deadline = ContinuousClock.now + .seconds(8)
        while ContinuousClock.now < deadline {
            let rows = try JSONDecoder().decode(Inbox.self, from: await cli(host, ["agent", "inbox"]))
            if predicate(rows) { return rows }
            try await Task.sleep(for: .milliseconds(50))
        }
        throw failure("Timed out awaiting " + description)
    }
    private static func settingsUntil(_ host: MainWindowHost, enabled: Bool) async throws {
        struct Settings: Decodable { let enabled: Bool; let effective_enabled: Bool }
        let deadline = ContinuousClock.now + .seconds(8)
        while ContinuousClock.now < deadline {
            let result = try JSONDecoder().decode(Settings.self, from: await cli(host, ["agent", "messages", "status", "--global"]))
            if result.enabled == enabled && result.effective_enabled == enabled { return }
            try await Task.sleep(for: .milliseconds(50))
        }
        throw failure("Durable receiving settings differ from the native action")
    }
    private static func signal(_ path: URL) async throws {
        try await Task.detached { try Data("fixture".utf8).write(to: path, options: .atomic) }.value
    }
    private static func file(_ path: URL) async throws {
        let deadline = ContinuousClock.now + .seconds(5)
        while ContinuousClock.now < deadline {
            if await Task.detached(operation: { FileManager.default.fileExists(atPath: path.path) }).value { return }
            try await Task.sleep(for: .milliseconds(32))
        }
        throw failure("Independent lock holder did not reach " + path.lastPathComponent)
    }
    private static func capture(_ window: NSWindow, to path: URL) async throws {
        guard let view = window.contentView, view.bounds.width >= 560, view.bounds.height >= 640,
              let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { throw failure("Mounted inbox capture unavailable") }
        view.layoutSubtreeIfNeeded(); view.cacheDisplay(in: view.bounds, to: bitmap)
        guard let data = bitmap.representation(using: .png, properties: [:]), data.count <= 8_388_608 else { throw failure("Inbox capture exceeds bound") }
        try await Task.detached { try data.write(to: path, options: .atomic) }.value
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
    private static func cli(_ host: MainWindowHost, _ arguments: [String], caller: Child? = nil, input: Data? = nil) async throws -> Data {
        if caller == nil {
            guard input == nil else { throw failure("Host CLI cannot substitute the fixture stdin caller") }
            return try await host.nativeCLI(arguments, timeout: 5000, externalCaller: true)
        }
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
            let stdin = Pipe()
            if input == nil { process.standardInput = FileHandle.nullDevice }
            else { process.standardInput = stdin }
            try process.run()
            if let input {
                try stdin.fileHandleForWriting.write(contentsOf: input)
                try stdin.fileHandleForWriting.close()
            }
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
        // Darwin's argument-string bridge may decompose Unicode. Seed through
        // the public byte-preserving stdin path, independent of AppKit rendering.
        let response = try await cli(host, ["agent", "message", recipient.uuidString, "--", "-"],
                                     caller: sender, input: Data(body.utf8))
        let sent = try JSONDecoder().decode(Sent.self, from: response)
        // Keep the independent CLI receipt and each exact assertion input even
        // when a prerequisite fails before the selected-body Unicode oracle.
        if let root = ProcessInfo.processInfo.environment["ZMUX_AGENT_INBOX_ROOT"] {
            let proof = try JSONSerialization.data(withJSONObject: [
                "inputTransport": "stdin_utf8",
                "expectedBodyUTF8": Array(body.utf8), "observedBodyUTF8": Array(sent.message.body.utf8),
                "expectedSender": sender.surface.uuidString, "expectedRecipient": recipient.uuidString,
                "receipt": try JSONSerialization.jsonObject(with: response)
            ], options: [.sortedKeys])
            let path = URL(fileURLWithPath: root).appendingPathComponent("cli-send-" + sent.message.request_id.uuidString.lowercased() + ".json")
            try await Task.detached { try proof.write(to: path, options: .atomic) }.value
        }
        guard sent.durability == "saved", sent.message.state == "queued", sent.message.body.utf8.elementsEqual(body.utf8),
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

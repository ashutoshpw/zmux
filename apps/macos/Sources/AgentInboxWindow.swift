import AppKit

@MainActor
final class AgentInboxWindowController: NSWindowController, NSWindowDelegate, NSTextViewDelegate {
    private struct SettingContext {
        let scope: String
        let target: String?
        let generation: UInt64
    }
    private struct Pending {
        enum Kind { case reply(AgentMessage, AgentInboxDrafts.Key, AgentInboxDrafts.Draft), read, settings(SettingContext) }
        let request: AgentMessageRequest
        let kind: Kind
    }
    private let origin: MainWindowIdentity
    private let client: AgentMessageClient
    private let drafts: AgentInboxDrafts
    private let rows = NSStackView()
    private let body = NSTextView()
    private let reply = NSTextView()
    private let attribution = NSTextField(wrappingLabelWithString: "Select a message to reply.")
    private let feedback = NSTextField(wrappingLabelWithString: "")
    private let settingDetail = NSTextField(wrappingLabelWithString: "")
    private let refreshButton = IconButton(symbol: "arrow.clockwise", label: "Refresh", target: nil, action: nil)
    private let firstButton = IconButton(symbol: "backward.end", label: "First Page", target: nil, action: nil)
    private let nextButton = IconButton(symbol: "chevron.right", label: "Next Page", target: nil, action: nil)
    private let readButton = IconButton(symbol: "envelope.open", label: "Mark Read", target: nil, action: nil)
    private let sendButton = IconButton(symbol: "paperplane", label: "Send Reply", target: nil, action: nil)
    private let statusButton = IconButton(symbol: "questionmark.circle", label: "Check Status", target: nil, action: nil)
    private let scope = NSPopUpButton(frame: .zero, pullsDown: false)
    private let receiving = NSButton(checkboxWithTitle: "Allow receiving", target: nil, action: nil)
    private var messages: [AgentMessage] = []
    private var rowButtons: [UUID: NSButton] = [:]
    private var selected: UUID?
    private var cursor: UInt64?
    private var page: UInt64?
    private var pending: Pending?
    private var uncertain = false
    private var activeOrigin = true
    private var settingKnown = false
    private var refreshing = false
    private var renderedReplyKey: AgentInboxDrafts.Key?
    private var settingGeneration: UInt64 = 0
    private var lastSettingRefresh: ContinuousClock.Instant?
    private var poll: Task<Void, Never>?
    private var refreshTask: Task<Void, Never>?
    private var mutationTask: Task<Void, Never>?
    private var settingTask: Task<Void, Never>?

    init(origin: MainWindowIdentity, client: AgentMessageClient, drafts: AgentInboxDrafts) {
        self.origin = origin; self.client = client; self.drafts = drafts
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 660, height: 820),
                              styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = "Agent Inbox"; window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 560, height: 760)
        super.init(window: window)
        window.delegate = self
        buildContent()
        controls()
    }
    required init?(coder: NSCoder) { nil }
    private var message: AgentMessage? { messages.first { $0.id == selected } }
    private var draftKey: AgentInboxDrafts.Key? { selected.map { AgentInboxDrafts.Key(origin: origin, parent: $0) } }
    private func request(_ action: AgentMessageRequest.Action, id: UUID = UUID()) -> AgentMessageRequest {
        AgentMessageRequest(origin: .init(namespace: origin.namespace, windowId: origin.windowId), requestId: id, action: action)
    }
    func show(relativeTo parent: NSWindow) {
        guard activeOrigin, let window else { return }
        if window.parent !== parent {
            window.parent?.removeChildWindow(window)
            parent.addChildWindow(window, ordered: .above)
            let screen = parent.screen?.visibleFrame ?? parent.frame
            window.setFrameOrigin(NSPoint(
                x: max(screen.minX, min(parent.frame.midX - window.frame.width / 2, screen.maxX - window.frame.width)),
                y: max(screen.minY, min(parent.frame.midY - window.frame.height / 2, screen.maxY - window.frame.height))))
        }
        showWindow(nil); window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(reply)
        refresh()
        poll?.cancel()
        poll = Task { [weak self] in
            while !Task.isCancelled {
                do { try await Task.sleep(for: .milliseconds(500)) } catch { return }
                guard let self, self.activeOrigin, self.window?.isVisible == true else { return }
                self.refresh()
            }
        }
    }
    func invalidateOrigin() {
        activeOrigin = false
        if let pending, case .reply(_, let key, _) = pending.kind { drafts.unpin(key) }
        poll?.cancel(); refreshTask?.cancel(); mutationTask?.cancel(); settingTask?.cancel()
        feedback.stringValue = "The original window is closed. This inbox cannot be retargeted."
        controls()
        close()
    }
    func windowWillClose(_ notification: Notification) { poll?.cancel(); poll = nil }
    private func buildContent() {
        guard let content = window?.contentView else { return }
        let stack = NSStackView(); stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 10
        stack.translatesAutoresizingMaskIntoConstraints = false; content.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 18),
            stack.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -18),
            stack.topAnchor.constraint(equalTo: content.topAnchor, constant: 18),
            stack.bottomAnchor.constraint(lessThanOrEqualTo: content.bottomAnchor, constant: -18)
        ])
        let explanation = NSTextField(wrappingLabelWithString: "Messages across this installation. Replies are saved to the inbox and do not type into terminals.")
        explanation.textColor = .secondaryLabelColor
        stack.addArrangedSubview(explanation)
        let toolbar = NSStackView(views: [refreshButton, firstButton, nextButton, readButton]); toolbar.spacing = 8
        stack.addArrangedSubview(toolbar)
        rows.orientation = .vertical; rows.alignment = .leading; rows.spacing = 4
        rows.translatesAutoresizingMaskIntoConstraints = false
        let list = NSScrollView(); list.hasVerticalScroller = true; list.documentView = rows
        list.borderType = .bezelBorder
        stack.addArrangedSubview(list)
        list.heightAnchor.constraint(equalToConstant: 180).isActive = true
        NSLayoutConstraint.activate([
            rows.widthAnchor.constraint(equalTo: list.contentView.widthAnchor),
            rows.leadingAnchor.constraint(equalTo: list.contentView.leadingAnchor),
            rows.topAnchor.constraint(equalTo: list.contentView.topAnchor)
        ])
        attribution.font = .systemFont(ofSize: 12); attribution.isSelectable = true
        stack.addArrangedSubview(attribution)
        let bodyScroll = textScroll(body, editable: false, label: "Message body")
        stack.addArrangedSubview(bodyScroll); bodyScroll.heightAnchor.constraint(equalToConstant: 90).isActive = true
        stack.addArrangedSubview(NSTextField(labelWithString: "Reply"))
        let replyScroll = textScroll(reply, editable: true, label: "Reply")
        reply.delegate = self
        stack.addArrangedSubview(replyScroll); replyScroll.heightAnchor.constraint(equalToConstant: 90).isActive = true
        let submit = NSStackView(views: [sendButton, statusButton]); submit.spacing = 8
        stack.addArrangedSubview(submit)
        feedback.setAccessibilityLabel("Message operation status"); feedback.isSelectable = true
        stack.addArrangedSubview(feedback)
        scope.addItems(withTitles: ["Recipient tab", "Recipient workspace", "All tabs"])
        scope.setAccessibilityLabel("Receiving settings scope")
        let settings = NSStackView(views: [scope, receiving]); settings.spacing = 10
        stack.addArrangedSubview(settings)
        settingDetail.font = .systemFont(ofSize: 12); settingDetail.setAccessibilityLabel("Configured and effective receiving state")
        stack.addArrangedSubview(settingDetail)
        for view in [explanation, list, attribution, bodyScroll, replyScroll, feedback, settingDetail] { view.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true }
        for button in [refreshButton, firstButton, nextButton, readButton, sendButton, statusButton] {
            button.target = self
            button.widthAnchor.constraint(equalToConstant: 28).isActive = true
            button.heightAnchor.constraint(equalToConstant: 28).isActive = true
        }
        refreshButton.action = #selector(refreshClicked(_:)); firstButton.action = #selector(firstClicked(_:)); nextButton.action = #selector(nextClicked(_:))
        readButton.action = #selector(markRead(_:)); sendButton.action = #selector(sendReply(_:)); statusButton.action = #selector(checkStatus(_:))
        sendButton.toolTip = "Send Reply (⌘Return)"
        sendButton.keyEquivalent = "\r"; sendButton.keyEquivalentModifierMask = [.command]
        receiving.target = self; receiving.action = #selector(settingChanged(_:))
        scope.target = self; scope.action = #selector(scopeChanged(_:))
    }
    private func textScroll(_ text: NSTextView, editable: Bool, label: String) -> NSScrollView {
        let scroll = NSScrollView(); scroll.hasVerticalScroller = true; scroll.borderType = .bezelBorder
        text.isEditable = editable; text.isSelectable = true; text.isRichText = false
        text.isAutomaticQuoteSubstitutionEnabled = false; text.isAutomaticDashSubstitutionEnabled = false
        text.isAutomaticTextReplacementEnabled = false; text.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        text.setAccessibilityLabel(label); text.autoresizingMask = [.width]; text.isVerticallyResizable = true
        text.isHorizontallyResizable = false; text.textContainer?.widthTracksTextView = true
        text.minSize = NSSize(width: 0, height: 90)
        text.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        text.textContainer?.containerSize = NSSize(width: 620, height: CGFloat.greatestFiniteMagnitude)
        text.textContainerInset = NSSize(width: 8, height: 8)
        text.setFrameSize(NSSize(width: 620, height: 90))
        scroll.documentView = text
        return scroll
    }
    private func controls() {
        let available = activeOrigin && message != nil
        refreshButton.isEnabled = activeOrigin && !refreshing
        firstButton.isEnabled = activeOrigin && !refreshing && page != nil
        nextButton.isEnabled = activeOrigin && !refreshing && cursor != nil
        readButton.isEnabled = available && pending == nil
        reply.isEditable = available
        sendButton.isEnabled = available && message?.sender.surfaceId != nil && pending == nil && !reply.string.isEmpty && reply.string.utf8.count <= 4096
        statusButton.isEnabled = activeOrigin && pending != nil && uncertain && mutationTask == nil
        receiving.isEnabled = activeOrigin && settingKnown && pending == nil
        scope.isEnabled = activeOrigin && pending == nil
    }
    @objc private func refreshClicked(_ sender: Any?) { refresh() }
    @objc private func firstClicked(_ sender: Any?) { page = nil; refresh() }
    @objc private func nextClicked(_ sender: Any?) { guard let cursor else { return }; page = cursor; refresh() }
    private func refresh() {
        guard activeOrigin, !refreshing else { return }
        refreshing = true; controls()
        let query = request(.list(page))
        refreshTask = Task { [weak self] in
            guard let self else { return }
            defer { self.refreshing = false; self.refreshTask = nil; self.controls() }
            do {
                let receipt = try await self.read(query)
                guard self.activeOrigin, !Task.isCancelled, let result = receipt.result, let messages = result.messages,
                      messages.count <= 32, messages.allSatisfy({ $0.namespace == self.origin.namespace && $0.body.utf8.count <= 4096 }) else { return }
                self.messages = messages; self.cursor = result.nextCursor
                if !messages.contains(where: { $0.id == self.selected }) { self.selected = messages.first?.id }
                self.renderRows(); self.renderSelection()
                if self.pending == nil, self.lastSettingRefresh.map({ ContinuousClock.now - $0 >= .seconds(1) }) ?? true { self.refreshSettings() }
            } catch { if !Task.isCancelled { self.feedback.stringValue = "Refresh failed: " + error.localizedDescription } }
        }
    }
    private func renderRows() {
        let visible = Set(messages.map(\.id))
        for id in Array(rowButtons.keys) where !visible.contains(id) {
            if let row = rowButtons.removeValue(forKey: id) { rows.removeArrangedSubview(row); row.removeFromSuperview() }
        }
        for (index, message) in messages.enumerated() {
            let row: NSButton
            if let existing = rowButtons[message.id] { row = existing }
            else {
                row = NSButton(title: "", target: self, action: #selector(selectMessage(_:)))
                row.identifier = NSUserInterfaceItemIdentifier(message.id.uuidString.lowercased())
                row.setAccessibilityIdentifier(message.id.uuidString.lowercased())
                row.alignment = .left; row.bezelStyle = .inline; row.setButtonType(.toggle)
                row.wantsLayer = true; row.layer?.cornerRadius = 5
                rowButtons[message.id] = row
            }
            let existingIndex = rows.arrangedSubviews.firstIndex(where: { $0 === row })
            if existingIndex != index {
                let firstMount = row.superview == nil
                if existingIndex != nil { rows.removeArrangedSubview(row) }
                rows.insertArrangedSubview(row, at: index)
                if firstMount { row.widthAnchor.constraint(equalTo: rows.widthAnchor).isActive = true }
            }
            let title = "\(message.sender.name) → \(message.recipient.workspaceName) · \(message.status)\n" + String(message.body.prefix(90))
            if row.title != title { row.title = title }
            let isSelected = message.id == selected
            row.state = isSelected ? .on : .off
            row.layer?.backgroundColor = isSelected ? NSColor.controlAccentColor.withAlphaComponent(0.16).cgColor : NSColor.clear.cgColor
            row.setAccessibilityLabel("\(isSelected ? "Selected message" : "Message") \(message.id.uuidString.lowercased()) from \(message.sender.name) to \(message.recipient.workspaceName), \(message.status)")
            row.toolTip = "Message \(message.id.uuidString.lowercased())\nRecipient tab \(message.recipient.surfaceId.uuidString.lowercased())"
        }
    }
    @objc private func selectMessage(_ sender: NSButton) {
        guard let raw = sender.identifier?.rawValue, let id = UUID(uuidString: raw), messages.contains(where: { $0.id == id }) else { return }
        selected = id; renderRows(); renderSelection(); window?.makeFirstResponder(reply)
    }
    private func renderSelection() {
        guard let message, let key = draftKey else {
            body.string = ""; reply.string = ""; renderedReplyKey = nil
            attribution.stringValue = "No messages on this page."
            controls(); return
        }
        if body.string != message.body { body.string = message.body }
        attribution.stringValue = "\(message.sender.name) → \(message.recipient.workspaceName)\n\(message.status)\(message.failureLabel.map { " · " + $0 } ?? "") · \(message.id.uuidString.lowercased())"
        if renderedReplyKey != key {
            renderedReplyKey = key; reply.string = drafts.value(key).body
            refreshSettings()
        }
        controls()
    }
    func textDidChange(_ notification: Notification) {
        guard notification.object as? NSTextView === reply, let key = draftKey else { return }
        drafts.set(key, body: reply.string); controls()
    }
    func textView(_ textView: NSTextView, shouldChangeTextIn affectedCharRange: NSRange, replacementString: String?) -> Bool {
        guard textView === reply, let replacementString else { return true }
        let proposed = (textView.string as NSString).replacingCharacters(in: affectedCharRange, with: replacementString)
        guard proposed.utf8.count <= 4096 else { feedback.stringValue = "Replies are limited to 4096 UTF-8 bytes."; return false }
        return true
    }
    @objc private func sendReply(_ sender: Any?) {
        guard activeOrigin, pending == nil, let message, let key = draftKey,
              message.sender.surfaceId != nil, !reply.string.isEmpty, reply.string.utf8.count <= 4096 else { return }
        // Capture text/revision/parent once; button and Command-Return enter here.
        if drafts.value(key).body != reply.string { drafts.set(key, body: reply.string) }
        let draft = drafts.value(key)
        begin(Pending(request: request(.reply(message.id, draft.body)), kind: .reply(message, key, draft)))
    }
    @objc private func markRead(_ sender: Any?) {
        guard activeOrigin, pending == nil, let selected else { return }
        begin(Pending(request: request(.markRead([selected])), kind: .read))
    }
    private var settingScope: String { ["surface", "workspace", "global"][max(0, min(scope.indexOfSelectedItem, 2))] }
    private var settingTarget: String? {
        guard let message else { return nil }
        switch settingScope { case "surface": return message.recipient.surfaceId.uuidString.lowercased()
        case "workspace": return message.recipient.workspaceId.uuidString.lowercased()
        default: return nil }
    }
    @objc private func scopeChanged(_ sender: Any?) { refreshSettings() }
    private func refreshSettings() {
        guard activeOrigin, message != nil || settingScope == "global" else { return }
        lastSettingRefresh = ContinuousClock.now
        settingGeneration &+= 1; let generation = settingGeneration
        settingKnown = false; settingDetail.stringValue = "Loading receiving settings…"; controls()
        settingTask?.cancel()
        let context = SettingContext(scope: settingScope, target: settingTarget, generation: generation)
        let query = request(.settings(context.scope, context.target, nil))
        settingTask = Task { [weak self] in
            guard let self else { return }
            do {
                let receipt = try await self.read(query)
                guard !Task.isCancelled, self.activeOrigin, self.settingGeneration == generation, let result = receipt.result else { return }
                guard result.scope == context.scope, result.owner == context.target.flatMap({ UUID(uuidString: $0) }),
                      self.settingScope == context.scope, self.settingTarget == context.target else {
                    throw BridgeFailure(message: "Receiving settings do not match the current recipient.")
                }
                self.showSetting(result)
            } catch { if !Task.isCancelled, self.settingGeneration == generation { self.settingDetail.stringValue = error.localizedDescription } }
        }
    }
    private func showSetting(_ result: AgentMessageReceipt.Result) {
        guard let configured = result.enabled, let effective = result.effectiveEnabled else { settingKnown = false; controls(); return }
        receiving.state = configured ? .on : .off; settingKnown = true
        settingDetail.stringValue = "Configured \(configured ? "On" : "Off") · Effective \(effective ? "On" : "Off")"
        controls()
    }
    @objc private func settingChanged(_ sender: Any?) {
        guard activeOrigin, settingKnown, pending == nil else { return }
        settingTask?.cancel(); settingGeneration &+= 1
        let context = SettingContext(scope: settingScope, target: settingTarget, generation: settingGeneration)
        begin(Pending(request: request(.settings(context.scope, context.target, receiving.state == .on)), kind: .settings(context)))
    }
    private func begin(_ operation: Pending) {
        guard pending == nil, activeOrigin else { return }
        if case .reply(_, let key, _) = operation.kind { drafts.pin(key) }
        pending = operation; uncertain = false; feedback.stringValue = "Saving…"; controls()
        run(operation, statusOnly: false)
    }
    @objc private func checkStatus(_ sender: Any?) {
        guard activeOrigin, uncertain, mutationTask == nil, let pending else { return }
        run(pending, statusOnly: true)
    }
    private func run(_ operation: Pending, statusOnly: Bool) {
        mutationTask = Task { [weak self] in
            guard let self else { return }
            defer { self.mutationTask = nil; self.controls() }
            do {
                let first = statusOnly ? self.request(.status, id: operation.request.requestId) : operation.request
                let receipt = try await self.read(first)
                guard !Task.isCancelled, self.activeOrigin, self.pending?.request.requestId == operation.request.requestId else { return }
                self.accept(receipt, operation: operation)
            } catch {
                guard !Task.isCancelled, self.activeOrigin else { return }
                // A refused status query says nothing about the original accepted
                // mutation. Only its authoritative failed receipt can discharge it.
                let unknown: Bool
                if let failure = error as? AgentMessageOperationFailure { unknown = failure.outcomeUnknown }
                else { unknown = statusOnly || ((error as? AgentMessageTransportFailure)?.outcomeUnknown ?? true) }
                self.uncertain = unknown
                self.feedback.stringValue = error.localizedDescription + (unknown ? " Check Status: " + operation.request.requestId.uuidString.lowercased() : "")
                if !unknown {
                    if case .reply(_, let key, _) = operation.kind { self.drafts.unpin(key) }
                    self.pending = nil; self.refreshSettings()
                }
            }
        }
        controls()
    }
    private func accept(_ receipt: AgentMessageReceipt, operation: Pending) {
        guard receipt.state == "completed", let result = receipt.result else {
            uncertain = receipt.state != "failed" || receipt.error?.outcomeUnknown != false
            feedback.stringValue = (receipt.error?.message ?? "Submission unconfirmed.") + " Check Status: " + operation.request.requestId.uuidString.lowercased()
            if !uncertain {
                if case .reply(_, let key, _) = operation.kind { drafts.unpin(key) }
                pending = nil; refreshSettings()
            }
            controls(); return
        }
        var refreshReceiving = false
        switch operation.kind {
        case .reply(let parent, let key, let draft):
            guard result.durability == "saved", let saved = result.message, saved.requestId == operation.request.requestId,
                  saved.replyTo == parent.id, saved.threadId == parent.threadId, saved.recipient.surfaceId == parent.sender.surfaceId,
                  saved.body == draft.body else {
                uncertain = true; feedback.stringValue = "The saved reply could not be verified. Check Status."; controls(); return
            }
            drafts.clear(key, ifRevision: draft.revision)
            if draftKey == key, drafts.value(key).body.isEmpty, reply.string == draft.body { reply.string = "" }
            feedback.stringValue = "Saved · \(saved.status)"
        case .read:
            guard result.messages != nil else { uncertain = true; feedback.stringValue = "Read result unconfirmed. Check Status."; return }
            feedback.stringValue = "Saved exact message read state."
        case .settings(let context):
            guard result.scope == context.scope, result.owner == context.target.flatMap({ UUID(uuidString: $0) }),
                  result.enabled != nil, result.effectiveEnabled != nil else {
                uncertain = true; feedback.stringValue = "Receiving settings unconfirmed. Check Status."; return
            }
            if context.scope == settingScope && context.target == settingTarget && context.generation == settingGeneration {
                showSetting(result)
            } else {
                settingKnown = false; settingDetail.stringValue = "Loading receiving settings for the current recipient…"
            }
            refreshReceiving = true
            feedback.stringValue = "Receiving settings saved. Turning on does not restore failed messages."
        }
        pending = nil; uncertain = false
        if refreshReceiving { refreshSettings() }
        refresh(); controls()
    }
    private func read(_ first: AgentMessageRequest) async throws -> AgentMessageReceipt {
        var receipt = try await client.perform(first)
        let deadline = ContinuousClock.now + .seconds(4)
        while receipt.state == "pending", ContinuousClock.now < deadline, !Task.isCancelled {
            try await Task.sleep(for: .milliseconds(100))
            do { receipt = try await client.perform(request(.status, id: first.requestId)) }
            catch { throw AgentMessageTransportFailure(message: error.localizedDescription, outcomeUnknown: true) }
        }
        if receipt.state == "failed", let failure = receipt.error {
            throw AgentMessageOperationFailure(message: failure.message, outcomeUnknown: failure.outcomeUnknown)
        }
        return receipt
    }
}

import AppKit
import Foundation

struct ScheduleMessagesRoute: Sendable, Equatable {
    let identityNamespace: UUID
    let surface: UInt64
    let expectedSession: UInt64?
    let destination: String
    let connected: Bool
}

private struct ScheduleTargetOption: Equatable {
    let token: String
    let title: String
}

private struct ScheduleMessageJob: Equatable {
    let id: UUID
    let version: UInt64
    let state: String
    let message: String
    let deadline: Date
    let paneID: String
    let targetTitle: String
    let submit: Bool
}

private struct ScheduleReplyData {
    let kind: String
    let value: [String: Any]
}

private struct ScheduleBridgeFailure: Error, LocalizedError {
    let code: String
    let message: String
    let outcomeUnknown: Bool
    var errorDescription: String? { message }
}

private struct ScheduleLocalMutation {
    let requestID: UUID
    let request: Data
    let description: String
    let expectedReplyKind: String
    var outcomeUnknown: Bool
}

private struct ScheduleMutationIdentity {
    let requestID: UUID
    let operation: String
    let jobID: UUID?
}

private struct ScheduleEditorSnapshot {
    let selectedJob: ScheduleMessageJob?
    let message: String
    let relativeMode: Bool
    let durationText: String
    let deadlineText: String
    let occurrence: Int
    let targetToken: String?
    let submit: Bool
    let messageEdited: Bool
    let deadlineEdited: Bool
    let targetEdited: Bool
    let submitEdited: Bool
}

private final class ScheduleDocumentView: NSView {
    override var isFlipped: Bool { true }
}

/// AppKit client for the remote helper. It owns only transient presentation and offline drafts;
/// the helper owns queue state, deadlines, claims, and delivery history.
@MainActor
final class ScheduleMessagesPanel: NSObject, NSTableViewDataSource, NSTableViewDelegate, NSTextViewDelegate, NSTextFieldDelegate {
    let view = NSScrollView()
    var onFocus: (() -> Void)?
    var preferredResponder: NSResponder { messageText.isEditable ? messageText : table }
    private var countdownTask: Task<Void, Never>?
    private var didLoad = false
    private let bridge: NativeBridge
    private let currentRoute: @MainActor () -> ScheduleMessagesRoute?
    private var route: ScheduleMessagesRoute
    private var currentJobs: [ScheduleMessageJob] = []
    private var historyJobs: [ScheduleMessageJob] = []
    private var targets: [ScheduleTargetOption] = []
    private var draftTargetToken: String?
    private var selectedJob: ScheduleMessageJob?
    private var localMutation: ScheduleLocalMutation?
    private var editorBaseMessage = ""
    private var editorBaseRelative = false
    private var editorBaseDuration = "1h"
    private var editorBaseDeadline = ""
    private var editorBaseOccurrence = 0
    private var editorBaseTargetToken: String?
    private var editorBaseSubmit = false
    private var messageEdited = false
    private var deadlineEdited = false
    private var targetEdited = false
    private var submitEdited = false
    private var isWorking = false
    private var isHistory = false
    private var suppressEditorEvents = false
    private var setupInfo: [String: Any]?
    private var statusText = "Not checked"

    private let destinationLabel = NSTextField(labelWithString: "")
    private let connectionLabel = NSTextField(labelWithString: "")
    private let statusLabel = NSTextField(wrappingLabelWithString: "Not checked")
    private let refreshButton = NSButton(title: "Refresh", target: nil, action: nil)
    private let setupCheckButton = NSButton(title: "Check Setup", target: nil, action: nil)
    private let setupInstallButton = NSButton(title: "Install / Enable", target: nil, action: nil)
    private let viewMode = NSSegmentedControl(labels: ["Pending", "History"], trackingMode: .selectOne, target: nil, action: nil)
    private let table = NSTableView()
    private let targetPicker = NSPopUpButton(frame: .zero, pullsDown: false)
    private let messageText = NSTextView(frame: NSRect(x: 0, y: 0, width: 430, height: 120))
    private let deadlineMode = NSSegmentedControl(labels: ["In", "At"], trackingMode: .selectOne, target: nil, action: nil)
    private let durationField = NSTextField(string: "1h")
    private let durationPresets = NSSegmentedControl(labels: ["1m", "5m", "15m", "1h"], trackingMode: .momentary, target: nil, action: nil)
    private let timeDisplay = NSSegmentedControl(labels: ["Relative", "Absolute"], trackingMode: .selectOne, target: nil, action: nil)
    private let deadlineField = NSTextField(string: "")
    private let occurrencePicker = NSPopUpButton(frame: .zero, pullsDown: false)
    private let resolvedDeadlineLabel = NSTextField(wrappingLabelWithString: "")
    private let submitCheckbox = NSButton(checkboxWithTitle: "Press Return after the message is sent", target: nil, action: nil)
    private let newButton = NSButton(title: "New", target: nil, action: nil)
    private let discardDraftButton = NSButton(title: "Discard Draft", target: nil, action: nil)
    private let saveButton = NSButton(title: "Schedule Message", target: nil, action: nil)
    private let cancelButton = NSButton(title: "Cancel Job", target: nil, action: nil)
    private let requestStatusButton = NSButton(title: "Check Request Status", target: nil, action: nil)

    init(bridge: NativeBridge, route: ScheduleMessagesRoute,
         currentRoute: @escaping @MainActor () -> ScheduleMessagesRoute?) {
        self.bridge = bridge
        self.route = route
        self.currentRoute = currentRoute
        super.init()
        configureControls()
        installLayout()
        updateRoutePresentation()
        beginNewMessage()
        updateControls()
    }

    required init?(coder: NSCoder) { nil }

    func update(_ newRoute: ScheduleMessagesRoute, visible: Bool) {
        if route != newRoute {
            route = newRoute
            updateRoutePresentation()
        }
        if visible {
            if countdownTask == nil {
                countdownTask = Task { @MainActor [weak self] in
                    while !Task.isCancelled {
                        self?.tickCountdowns()
                        try? await Task.sleep(for: .seconds(1))
                    }
                }
            }
            if !didLoad && isRemoteReady {
                didLoad = true
                refreshFromRemote()
            }
        } else {
            countdownTask?.cancel()
            countdownTask = nil
        }
    }

    func close() {
        countdownTask?.cancel()
        countdownTask = nil
        view.removeFromSuperview()
    }

    private func tickCountdowns() {
        guard !isHistory, timeDisplay.selectedSegment == 0,
              let column = table.tableColumns.firstIndex(where: { $0.identifier.rawValue == "deadline" }) else { return }
        for row in visibleJobs.indices {
            (table.view(atColumn: column, row: row, makeIfNecessary: false) as? NSTableCellView)?
                .textField?.stringValue = Self.countdown(to: visibleJobs[row].deadline)
        }
    }

    private func configureControls() {
        for (button, action, label) in [
            (refreshButton, #selector(refreshPressed), "Refresh scheduled messages"),
            (setupCheckButton, #selector(checkSetupPressed), "Check remote scheduler setup"),
            (setupInstallButton, #selector(installSetupPressed), "Install and enable the remote scheduler"),
            (newButton, #selector(newPressed), "Create a new scheduled message"),
            (discardDraftButton, #selector(discardDraftPressed), "Discard the unsent local draft"),
            (saveButton, #selector(savePressed), "Save or schedule message"),
            (cancelButton, #selector(cancelPressed), "Cancel selected pending scheduled message"),
            (requestStatusButton, #selector(requestStatusPressed), "Check the remote receipt for the unresolved request")
        ] {
            button.target = self
            button.action = action
            button.setAccessibilityLabel(label)
        }
        refreshButton.bezelStyle = .rounded
        setupCheckButton.bezelStyle = .rounded
        setupInstallButton.bezelStyle = .rounded
        newButton.bezelStyle = .rounded
        discardDraftButton.bezelStyle = .rounded
        saveButton.bezelStyle = .rounded
        cancelButton.bezelStyle = .rounded
        requestStatusButton.bezelStyle = .rounded

        viewMode.selectedSegment = 0
        viewMode.setAccessibilityLabel("Scheduled message list view")
        viewMode.setAccessibilityHelp("Switch between pending messages and delivery history")
        viewMode.target = self
        viewMode.action = #selector(viewModeChanged)

        table.addTableColumn(makeColumn("Message", identifier: "message", width: 150))
        table.addTableColumn(makeColumn("Remote target", identifier: "target", width: 100))
        table.addTableColumn(makeColumn("Scheduled", identifier: "deadline", width: 110))
        table.addTableColumn(makeColumn("State", identifier: "state", width: 90))
        table.headerView = NSTableHeaderView()
        table.rowHeight = 42
        table.usesAlternatingRowBackgroundColors = true
        table.allowsMultipleSelection = false
        table.dataSource = self
        table.delegate = self
        table.setAccessibilityLabel("Scheduled message list")

        targetPicker.setAccessibilityLabel("Remote tmux pane")
        targetPicker.setAccessibilityHelp("These targets come from the selected remote helper. Local Zmux panes are not targets.")
        targetPicker.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        targetPicker.target = self
        targetPicker.action = #selector(targetChanged)

        messageText.isRichText = false
        messageText.importsGraphics = false
        messageText.allowsUndo = true
        messageText.font = .systemFont(ofSize: 13)
        messageText.isVerticallyResizable = true
        messageText.isHorizontallyResizable = false
        messageText.autoresizingMask = [.width]
        messageText.minSize = .zero
        messageText.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude,
                                     height: CGFloat.greatestFiniteMagnitude)
        messageText.textContainer?.containerSize = NSSize(width: CGFloat.greatestFiniteMagnitude,
                                                          height: CGFloat.greatestFiniteMagnitude)
        messageText.textContainer?.widthTracksTextView = true
        messageText.textContainerInset = NSSize(width: 8, height: 8)
        messageText.delegate = self
        messageText.setAccessibilityLabel("Scheduled message text")
        messageText.setAccessibilityHelp("Up to 4096 UTF-8 bytes. Text is sent literally; Return is optional and defaults off.")

        deadlineMode.selectedSegment = 0
        deadlineMode.target = self
        deadlineMode.action = #selector(deadlineChanged)
        deadlineMode.setAccessibilityLabel("Schedule time mode")
        durationField.placeholderString = "5m or 1h30m"
        durationField.delegate = self
        durationField.target = self
        durationField.action = #selector(deadlineChanged)
        durationField.setAccessibilityLabel("Schedule delay")
        durationField.setAccessibilityHelp("1 second through 365 days. Combine descending d, h, m and s units, for example 1h30m. Delay begins when the remote helper accepts the request.")
        durationPresets.target = self
        durationPresets.action = #selector(presetPressed)
        durationPresets.setAccessibilityLabel("Schedule delay presets")
        timeDisplay.selectedSegment = 0
        timeDisplay.target = self
        timeDisplay.action = #selector(timeDisplayChanged)
        timeDisplay.setAccessibilityLabel("Scheduled time display")

        deadlineField.placeholderString = "YYYY-MM-DD HH:mm"
        deadlineField.font = .monospacedDigitSystemFont(ofSize: 13, weight: .regular)
        deadlineField.delegate = self
        deadlineField.target = self
        deadlineField.action = #selector(deadlineChanged)
        deadlineField.setAccessibilityLabel("Scheduled local date and time")
        deadlineField.setAccessibilityHelp("Enter a strict local wall time as YYYY-MM-DD HH:mm. Daylight-saving gaps are rejected; repeated times require a first or second occurrence choice.")

        occurrencePicker.addItems(withTitles: ["Choose occurrence…", "First occurrence", "Second occurrence"])
        occurrencePicker.target = self
        occurrencePicker.action = #selector(occurrenceChanged)
        occurrencePicker.setAccessibilityLabel("Repeated local time occurrence")
        occurrencePicker.selectItem(at: 0)
        occurrencePicker.isHidden = true

        submitCheckbox.state = .off
        submitCheckbox.target = self
        submitCheckbox.action = #selector(editorChanged)
        submitCheckbox.setAccessibilityHelp("Unchecked by default. When checked, one Return is sent after the message text.")

        destinationLabel.font = .systemFont(ofSize: 14, weight: .semibold)
        destinationLabel.lineBreakMode = .byTruncatingMiddle
        connectionLabel.font = .systemFont(ofSize: 12)
        connectionLabel.textColor = .secondaryLabelColor
        connectionLabel.lineBreakMode = .byTruncatingMiddle
        statusLabel.font = .systemFont(ofSize: 12)
        statusLabel.textColor = .secondaryLabelColor
        statusLabel.maximumNumberOfLines = 2

        resolvedDeadlineLabel.font = .systemFont(ofSize: 11)
        resolvedDeadlineLabel.textColor = .secondaryLabelColor
        resolvedDeadlineLabel.maximumNumberOfLines = 3
    }

    private func makeColumn(_ title: String, identifier: String, width: CGFloat) -> NSTableColumn {
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(identifier))
        column.title = title
        column.width = width
        column.minWidth = 72
        column.resizingMask = .userResizingMask
        return column
    }

    private func installLayout() {
        let root = ScheduleDocumentView()
        root.translatesAutoresizingMaskIntoConstraints = false
        let heading = verticalStack([destinationLabel, connectionLabel], spacing: 4)
        let controls = horizontalStack([refreshButton, setupCheckButton, setupInstallButton], spacing: 8)
        let top = verticalStack([heading, controls, statusLabel], spacing: 8)

        let tableScroll = NSScrollView()
        tableScroll.hasVerticalScroller = true
        tableScroll.hasHorizontalScroller = true
        tableScroll.autohidesScrollers = true
        tableScroll.documentView = table
        tableScroll.setAccessibilityLabel("Pending scheduled messages and history")
        let left = verticalStack([horizontalStack([viewMode, timeDisplay], spacing: 8), tableScroll], spacing: 8)
        viewMode.widthAnchor.constraint(equalToConstant: 190).isActive = true
        tableScroll.translatesAutoresizingMaskIntoConstraints = false
        tableScroll.widthAnchor.constraint(equalTo: left.widthAnchor).isActive = true
        tableScroll.heightAnchor.constraint(greaterThanOrEqualToConstant: 360).isActive = true
        left.widthAnchor.constraint(greaterThanOrEqualToConstant: 470).isActive = true

        let targetHeading = NSTextField(labelWithString: "Remote tmux pane")
        targetHeading.font = .systemFont(ofSize: 12, weight: .medium)
        let targetGroup = verticalStack([targetHeading, targetPicker], spacing: 5)
        let messageHeading = NSTextField(labelWithString: "Message")
        messageHeading.font = .systemFont(ofSize: 12, weight: .medium)
        let messageScroll = NSScrollView()
        messageScroll.hasVerticalScroller = true
        messageScroll.drawsBackground = true
        messageScroll.borderType = .bezelBorder
        messageScroll.hasHorizontalScroller = false
        messageScroll.documentView = messageText
        messageScroll.setAccessibilityLabel("Scheduled message editor scroll area")
        messageScroll.heightAnchor.constraint(equalToConstant: 120).isActive = true
        let messageGroup = verticalStack([messageHeading, messageScroll], spacing: 5)

        let deadlineHeading = NSTextField(labelWithString: "When")
        deadlineHeading.font = .systemFont(ofSize: 12, weight: .medium)
        let deadlineGroup = verticalStack([deadlineHeading, deadlineMode, durationField, durationPresets, deadlineField, occurrencePicker, resolvedDeadlineLabel], spacing: 5)
        let editorButtons = horizontalStack([newButton, discardDraftButton, saveButton, cancelButton], spacing: 8)
        let right = verticalStack([targetGroup, messageGroup, deadlineGroup, submitCheckbox, editorButtons, requestStatusButton], spacing: 14)
        let rightSpacer = NSView(frame: .zero)
        rightSpacer.setContentHuggingPriority(.defaultLow, for: .vertical)
        rightSpacer.setContentCompressionResistancePriority(.defaultLow, for: .vertical)
        right.addArrangedSubview(rightSpacer)
        right.widthAnchor.constraint(greaterThanOrEqualToConstant: 430).isActive = true

        for view in [targetGroup, messageGroup, deadlineGroup, submitCheckbox] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            view.widthAnchor.constraint(equalTo: right.widthAnchor).isActive = true
            view.setContentHuggingPriority(.required, for: .vertical)
        }
        for view in [editorButtons, requestStatusButton] {
            view.setContentHuggingPriority(.required, for: .vertical)
        }
        for (view, stack) in [
            (targetHeading as NSView, targetGroup), (targetPicker as NSView, targetGroup),
            (messageHeading as NSView, messageGroup), (messageScroll as NSView, messageGroup),
            (deadlineField as NSView, deadlineGroup), (occurrencePicker as NSView, deadlineGroup),
            (resolvedDeadlineLabel as NSView, deadlineGroup)
        ] {
            view.translatesAutoresizingMaskIntoConstraints = false
            view.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        }
        messageScroll.translatesAutoresizingMaskIntoConstraints = false
        top.setContentHuggingPriority(.required, for: .vertical)
        top.setContentCompressionResistancePriority(.required, for: .vertical)

        let divider = NSSplitView()
        divider.isVertical = true
        divider.dividerStyle = .thin
        divider.addArrangedSubview(left)
        divider.addArrangedSubview(right)
        divider.setAccessibilityLabel("Scheduled message queue and editor")

        top.translatesAutoresizingMaskIntoConstraints = false
        divider.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(top)
        root.addSubview(divider)
        NSLayoutConstraint.activate([
            top.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 18),
            top.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -18),
            top.topAnchor.constraint(equalTo: root.topAnchor, constant: 16),
            divider.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 18),
            divider.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -18),
            divider.topAnchor.constraint(equalTo: top.bottomAnchor, constant: 14),
            divider.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -16)
        ])
        view.hasVerticalScroller = true
        view.hasHorizontalScroller = true
        view.autohidesScrollers = true
        view.drawsBackground = true
        view.documentView = root
        view.setAccessibilityLabel("Schedules tab")
        let width = root.widthAnchor.constraint(equalTo: view.contentView.widthAnchor)
        width.priority = .defaultLow
        let height = root.heightAnchor.constraint(equalTo: view.contentView.heightAnchor)
        height.priority = .defaultLow
        NSLayoutConstraint.activate([
            root.leadingAnchor.constraint(equalTo: view.contentView.leadingAnchor),
            root.topAnchor.constraint(equalTo: view.contentView.topAnchor),
            root.widthAnchor.constraint(greaterThanOrEqualToConstant: 950),
            root.heightAnchor.constraint(greaterThanOrEqualToConstant: 690), width, height
        ])
    }

    private func verticalStack(_ views: [NSView], spacing: CGFloat) -> NSStackView {
        let stack = NSStackView(views: views)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.distribution = .fill
        stack.spacing = spacing
        stack.translatesAutoresizingMaskIntoConstraints = false
        return stack
    }

    private func horizontalStack(_ views: [NSView], spacing: CGFloat) -> NSStackView {
        let stack = NSStackView(views: views)
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.distribution = .fill
        stack.spacing = spacing
        stack.translatesAutoresizingMaskIntoConstraints = false
        return stack
    }

    private func updateRoutePresentation() {
        destinationLabel.stringValue = route.destination
        connectionLabel.stringValue = route.connected && route.expectedSession != nil
            ? "Managed SSH connected · remote scheduler state is authoritative"
            : "Managed SSH disconnected · drafts remain local until you send them"
        updateControls()
    }

    private var visibleJobs: [ScheduleMessageJob] { isHistory ? historyJobs : currentJobs }
    private var selectedTarget: ScheduleTargetOption? {
        guard let represented = targetPicker.selectedItem?.representedObject as? String else { return nil }
        return targets.first { $0.token == represented }
    }
    private var isRemoteReady: Bool {
        guard let current = currentRoute() else { return false }
        return current.identityNamespace == route.identityNamespace && current.surface == route.surface
            && current.connected && current.expectedSession != nil
    }
    private var unresolvedMutation: Bool { localMutation?.outcomeUnknown == true }
    private var hasOfflineDraft: Bool { localMutation != nil && localMutation?.outcomeUnknown == false }
    private var hasUnsavedEditorChanges: Bool {
        hasOfflineDraft || messageEdited || deadlineEdited || targetEdited || submitEdited
    }

    private func updateControls() {
        let busy = isWorking
        let canSubmitDraft = hasOfflineDraft && isRemoteReady && !busy
        let currentPending = selectedJob.map { $0.state == "pending" && !isHistory } ?? false
        let editable = !busy && !unresolvedMutation && !isHistory && (selectedJob == nil || currentPending)
        refreshButton.isEnabled = !busy && isRemoteReady
        setupCheckButton.isEnabled = !busy && isRemoteReady
        let linuxSupported = (setupInfo?["os"] as? String)?.lowercased() == "linux"
        let supported = setupInfo?["supported"] as? Bool ?? false
        setupInstallButton.isEnabled = !busy && isRemoteReady && supported && linuxSupported
        viewMode.isEnabled = !busy
        targetPicker.isEnabled = editable && selectedJob == nil && !targets.isEmpty
        messageText.isEditable = editable
        deadlineMode.isEnabled = editable
        durationField.isEnabled = editable
        durationPresets.isEnabled = editable
        deadlineField.isEnabled = editable
        occurrencePicker.isEnabled = editable && !occurrencePicker.isHidden
        submitCheckbox.isEnabled = editable && selectedJob == nil
        newButton.isEnabled = !busy && !unresolvedMutation
        discardDraftButton.isHidden = !hasUnsavedEditorChanges
        discardDraftButton.title = hasOfflineDraft ? "Discard Draft" : "Discard Changes"
        discardDraftButton.isEnabled = hasUnsavedEditorChanges && !busy && !unresolvedMutation
        cancelButton.isEnabled = !busy && !unresolvedMutation && currentPending
        requestStatusButton.isHidden = !unresolvedMutation
        requestStatusButton.isEnabled = !busy && isRemoteReady
        requestStatusButton.title = localMutation?.expectedReplyKind == "setup" ? "Check Setup State" : "Check Request Status"
        if unresolvedMutation {
            saveButton.title = "Awaiting Request Status"
            saveButton.isEnabled = false
        } else if canSubmitDraft {
            saveButton.title = "Send Draft"
            saveButton.isEnabled = true
        } else if !isRemoteReady && !busy {
            saveButton.title = selectedJob == nil ? "Save Draft" : "Save Edit Draft"
            saveButton.isEnabled = editable && canBuildMutation()
        } else {
            saveButton.title = selectedJob == nil ? "Schedule Message" : "Save Changes"
            saveButton.isEnabled = editable && isRemoteReady && canBuildMutation()
        }
    }

    private func beginWork(_ message: String) {
        isWorking = true
        statusText = message
        statusLabel.stringValue = message
        updateControls()
    }

    private func endWork() {
        isWorking = false
        updateControls()
    }

    private func refreshFromRemote(selectingJobID requestedJobID: UUID? = nil,
                                   preloadedJob: ScheduleMessageJob? = nil,
                                   resolvingMutationRequestID: UUID? = nil) {
        guard !isWorking else { return }
        let editorSnapshot = hasUnsavedEditorChanges ? captureEditorSnapshot() : nil
        let previousJobID = selectedJob?.id
        if let refreshed = currentRoute(), refreshed.identityNamespace == route.identityNamespace,
           refreshed.surface == route.surface {
            route = refreshed
        }
        updateRoutePresentation()
        guard isRemoteReady else {
            statusText = "Remote data is unavailable. Your editor contents remain a local draft."
            statusLabel.stringValue = statusText
            updateControls()
            return
        }
        beginWork("Refreshing remote scheduler, targets, pending jobs, and history…")
        Task { [weak self] in
            guard let self else { return }
            do {
                let status = try await self.performRead(action: ["action": "status"])
                if status.kind == "status" { self.showStatusPayload(status.value) }
                let targetReply = try await self.performRead(action: ["action": "targets"])
                guard targetReply.kind == "targets" else { throw self.invalidReply("targets") }
                self.targets = try self.parseTargets(targetReply.value)
                self.rebuildTargetPicker()
                let jobsReply = try await self.performRead(action: ["action": "list", "limit": 100])
                guard jobsReply.kind == "jobs" else { throw self.invalidReply("pending jobs") }
                self.currentJobs = try self.parseJobs(jobsReply.value)
                let historyReply = try await self.performRead(action: ["action": "history", "limit": 100])
                guard historyReply.kind == "history" else { throw self.invalidReply("history") }
                self.historyJobs = try self.parseJobs(historyReply.value)
                if let requestedJobID {
                    if let preloadedJob {
                        guard preloadedJob.id == requestedJobID else {
                            throw self.unresolvedReceiptFailure()
                        }
                    } else if resolvingMutationRequestID != nil {
                        throw self.unresolvedReceiptFailure()
                    }
                    if let preloadedJob,
                       !self.currentJobs.contains(where: { $0.id == requestedJobID })
                        && !self.historyJobs.contains(where: { $0.id == requestedJobID }) {
                        if preloadedJob.state == "pending" || preloadedJob.state == "dispatching" {
                            self.currentJobs.insert(preloadedJob, at: 0)
                        } else {
                            self.historyJobs.insert(preloadedJob, at: 0)
                        }
                    }
                }
                var jobToLoad: ScheduleMessageJob?
                var latestEditorJob: ScheduleMessageJob?
                var shouldBeginNewMessage = false
                if let requestedJobID,
                   let job = self.currentJobs.first(where: { $0.id == requestedJobID }) {
                    self.isHistory = false
                    jobToLoad = job
                } else if let requestedJobID,
                          let job = self.historyJobs.first(where: { $0.id == requestedJobID }) {
                    self.isHistory = true
                    jobToLoad = job
                } else if let editorSnapshot {
                    latestEditorJob = editorSnapshot.selectedJob.flatMap { snapshotJob in
                        self.currentJobs.first(where: { $0.id == snapshotJob.id })
                            ?? self.historyJobs.first(where: { $0.id == snapshotJob.id })
                    }
                    if let latestEditorJob {
                        self.isHistory = self.historyJobs.contains(where: { $0.id == latestEditorJob.id })
                    }
                } else if let previousJobID,
                          let job = self.currentJobs.first(where: { $0.id == previousJobID })
                            ?? self.historyJobs.first(where: { $0.id == previousJobID }) {
                    self.isHistory = self.historyJobs.contains(where: { $0.id == job.id })
                    jobToLoad = job
                } else if previousJobID != nil {
                    shouldBeginNewMessage = true
                }
                self.viewMode.selectedSegment = self.isHistory ? 1 : 0
                self.suppressEditorEvents = true
                self.table.reloadData()
                self.suppressEditorEvents = false
                if let jobToLoad {
                    self.loadEditor(jobToLoad)
                    self.selectRow(for: jobToLoad.id)
                } else if let editorSnapshot {
                    self.restoreEditor(editorSnapshot)
                    if let id = editorSnapshot.selectedJob?.id { self.selectRow(for: id) }
                    else { self.table.deselectAll(nil) }
                } else if shouldBeginNewMessage {
                    self.table.deselectAll(nil)
                    self.beginNewMessage()
                } else {
                    self.rebuildTargetPicker(preferredToken: self.draftTargetToken)
                    self.editorBaseTargetToken = self.selectedTarget?.token
                    self.table.deselectAll(nil)
                }
                self.statusText = "Remote scheduler refreshed. No local timer is active."
                if historyReply.value["truncated"] as? Bool == true {
                    self.statusText += " Showing the latest 100 history records; older records were omitted."
                }
                self.statusLabel.stringValue = self.statusText
                if let resolvingMutationRequestID {
                    guard let mutation = self.localMutation,
                          mutation.outcomeUnknown,
                          mutation.requestID == resolvingMutationRequestID,
                          let requestedJobID,
                          self.selectedJob?.id == requestedJobID else {
                        throw self.unresolvedReceiptFailure()
                    }
                    self.localMutation = nil
                    self.statusText = "Applied request receipt reconciled. \(self.statusText)"
                    self.statusLabel.stringValue = self.statusText
                    self.updateControls()
                }
            } catch {
                self.present(error)
            }
            self.endWork()
        }
    }

    private func captureEditorSnapshot() -> ScheduleEditorSnapshot {
        ScheduleEditorSnapshot(
            selectedJob: selectedJob,
            message: messageText.string,
            relativeMode: deadlineMode.selectedSegment == 0,
            durationText: durationField.stringValue,
            deadlineText: deadlineField.stringValue,
            occurrence: occurrencePicker.indexOfSelectedItem,
            targetToken: selectedTarget?.token ?? draftTargetToken,
            submit: submitCheckbox.state == .on,
            messageEdited: messageEdited,
            deadlineEdited: deadlineEdited,
            targetEdited: targetEdited,
            submitEdited: submitEdited
        )
    }

    private func restoreEditor(_ snapshot: ScheduleEditorSnapshot) {
        selectedJob = snapshot.selectedJob
        suppressEditorEvents = true
        if let job = selectedJob {
            targetPicker.removeAllItems()
            targetPicker.addItem(withTitle: job.targetTitle)
            targetPicker.lastItem?.isEnabled = false
        } else {
            rebuildTargetPicker(preferredToken: snapshot.targetToken)
        }
        messageText.string = snapshot.message
        deadlineMode.selectedSegment = snapshot.relativeMode ? 0 : 1
        durationField.stringValue = snapshot.durationText
        deadlineField.stringValue = snapshot.deadlineText
        occurrencePicker.selectItem(at: max(0, min(snapshot.occurrence, occurrencePicker.numberOfItems - 1)))
        submitCheckbox.state = snapshot.submit ? .on : .off
        suppressEditorEvents = false
        messageEdited = snapshot.messageEdited
        deadlineEdited = snapshot.deadlineEdited
        targetEdited = snapshot.targetEdited
        submitEdited = snapshot.submitEdited
        updateDeadlineSummary()
        updateControls()
    }

    private func selectRow(for id: UUID) {
        let jobs = visibleJobs
        guard let row = jobs.firstIndex(where: { $0.id == id }) else {
            table.deselectAll(nil)
            return
        }
        suppressEditorEvents = true
        table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
        suppressEditorEvents = false
    }

    private func performRead(action: [String: Any]) async throws -> ScheduleReplyData {
        try await perform(action: action, requestID: nil)
    }

    private func perform(action: [String: Any], requestID: UUID?) async throws -> ScheduleReplyData {
        guard let current = currentRoute(), current.identityNamespace == route.identityNamespace,
              current.surface == route.surface, current.connected, let session = current.expectedSession else {
            throw ScheduleBridgeFailure(code: "ssh_disconnected", message: "The selected managed SSH route is disconnected. Keep this as a local draft and send it after reconnecting.", outcomeUnknown: false)
        }
        let requestIDValue: Any = requestID.map { $0.uuidString.lowercased() as Any } ?? NSNull()
        let request = try JSONSerialization.data(withJSONObject: [
            "protocol_version": 1,
            "request_id": requestIDValue,
            "action": action
        ], options: [.sortedKeys])
        let response = try await bridge.scheduleRequest(identityNamespace: current.identityNamespace,
            surface: current.surface, expectedSession: session, request: request)
        guard let wrapper = try JSONSerialization.jsonObject(with: response) as? [String: Any],
              let ok = wrapper["ok"] as? Bool,
              let outcomeUnknown = wrapper["outcome_unknown"] as? Bool else {
            throw ScheduleBridgeFailure(code: "invalid_wrapper", message: "The remote scheduler returned an invalid bridge response.", outcomeUnknown: true)
        }
        guard ok else {
            let error = wrapper["error"] as? [String: Any] ?? [:]
            throw ScheduleBridgeFailure(code: error["code"] as? String ?? "transport_error",
                message: error["message"] as? String ?? "The remote scheduler request failed.",
                outcomeUnknown: outcomeUnknown)
        }
        guard let responseObject = wrapper["reply"] as? [String: Any],
              let result = responseObject["result"] as? [String: Any],
              let status = result["status"] as? String,
              let data = result["data"] as? [String: Any] else {
            throw ScheduleBridgeFailure(code: "invalid_reply", message: "The scheduler reply did not match protocol version 1.", outcomeUnknown: false)
        }
        guard status == "success" else {
            throw ScheduleBridgeFailure(code: data["code"] as? String ?? "scheduler_rejected",
                message: data["message"] as? String ?? "The remote scheduler rejected the request.",
                outcomeUnknown: (data["outcome_unknown"] as? Bool) ?? outcomeUnknown)
        }
        guard let kind = data["kind"] as? String else {
            throw ScheduleBridgeFailure(code: "invalid_reply", message: "The scheduler reply did not include its result kind.", outcomeUnknown: false)
        }
        let value = data["value"] as? [String: Any] ?? data
        return ScheduleReplyData(kind: kind, value: value)
    }

    private func invalidReply(_ operation: String) -> ScheduleBridgeFailure {
        ScheduleBridgeFailure(code: "invalid_reply", message: "The remote scheduler returned an unexpected \(operation) reply.", outcomeUnknown: false)
    }

    private func present(_ error: Error) {
        if let schedule = error as? ScheduleBridgeFailure {
            statusText = schedule.message
            if schedule.outcomeUnknown, let mutation = localMutation {
                localMutation = ScheduleLocalMutation(requestID: mutation.requestID, request: mutation.request,
                    description: mutation.description, expectedReplyKind: mutation.expectedReplyKind,
                    outcomeUnknown: true)
                statusText += " Request \(mutation.requestID.uuidString.lowercased()) may have been applied; check its status before any retry."
            }
        } else {
            statusText = error.localizedDescription
        }
        statusLabel.stringValue = statusText
        updateControls()
    }

    private func showStatusPayload(_ value: [String: Any]) {
        guard let status = value["status"] as? [String: Any] else { return }
        let service = status["service_mode"] as? String ?? "unknown mode"
        let store = status["store_health"] as? String ?? "unknown store state"
        let tmux = (status["tmux_available"] as? Bool).map { $0 ? "tmux available" : "tmux unavailable" } ?? "tmux state unknown"
        let version = status["helper_version"] as? String
        let suffix = version.map { " · helper \($0)" } ?? ""
        connectionLabel.stringValue = (route.connected ? "Managed SSH connected" : "Managed SSH disconnected")
            + " · scheduler \(service) · store \(store) · \(tmux)\(suffix)"
    }

    private func parseTargets(_ value: [String: Any]) throws -> [ScheduleTargetOption] {
        guard let rows = value["targets"] as? [[String: Any]] else { throw invalidReply("targets") }
        return try rows.map { target in
            guard let token = target["id"] as? String, !token.isEmpty,
                  let identity = target["identity"] as? [String: Any],
                  let display = target["display"] as? [String: Any] else {
                throw invalidReply("target")
            }
            let pane = Self.stringValue(identity["pane_id"]) ?? "remote pane"
            let session = display["session_name"] as? String ?? "tmux session"
            let window = display["window_name"] as? String ?? "window"
            let title = display["pane_title"] as? String
            let label = "\(session) / \(window) · \(pane)" + (title.map { " · \($0)" } ?? "")
            return ScheduleTargetOption(token: token, title: label)
        }
    }

    private func parseJobs(_ value: [String: Any]) throws -> [ScheduleMessageJob] {
        guard let jobs = value["jobs"] as? [[String: Any]] else { throw invalidReply("jobs") }
        return try jobs.map { try parseJob($0) }
    }

    private func parseJob(_ job: [String: Any]) throws -> ScheduleMessageJob {
        guard let idText = job["id"] as? String, let id = UUID(uuidString: idText),
              let version = Self.uint64Value(job["version"]),
              let state = job["state"] as? String,
              let message = job["message"] as? String,
              let deadlineText = job["deadline_utc"] as? String,
              let deadline = Self.parseDate(deadlineText),
              let identity = job["target"] as? [String: Any],
              let display = job["display"] as? [String: Any] else { throw invalidReply("job") }
        let pane = Self.stringValue(identity["pane_id"]) ?? "remote pane"
        let session = display["session_name"] as? String ?? "tmux session"
        let window = display["window_name"] as? String ?? "window"
        let title = display["pane_title"] as? String
        let targetTitle = "\(session) / \(window) · \(pane)" + (title.map { " · \($0)" } ?? "")
        return ScheduleMessageJob(id: id, version: version, state: state, message: message,
            deadline: deadline, paneID: pane, targetTitle: targetTitle, submit: job["submit"] as? Bool ?? false)
    }

    private func rebuildTargetPicker(preferredToken: String? = nil) {
        let currentToken = preferredToken ?? selectedTarget?.token ?? draftTargetToken
        targetPicker.removeAllItems()
        for target in targets {
            targetPicker.addItem(withTitle: target.title)
            targetPicker.lastItem?.representedObject = target.token
        }
        if targets.isEmpty {
            targetPicker.addItem(withTitle: "No remote tmux panes available")
            targetPicker.lastItem?.isEnabled = false
        } else if let currentToken, let index = targets.firstIndex(where: { $0.token == currentToken }) {
            targetPicker.selectItem(at: index)
        } else {
            targetPicker.selectItem(at: 0)
        }
        draftTargetToken = targetPicker.selectedItem?.representedObject as? String
        updateControls()
    }

    private func beginNewMessage() {
        selectedJob = nil
        suppressEditorEvents = true
        messageText.string = ""
        deadlineMode.selectedSegment = 0
        durationField.stringValue = "1h"
        deadlineField.stringValue = Self.localFormatter.string(from: Date().addingTimeInterval(3600))
        occurrencePicker.selectItem(at: 0)
        occurrencePicker.isHidden = true
        submitCheckbox.state = .off
        suppressEditorEvents = false
        rebuildTargetPicker(preferredToken: draftTargetToken)
        editorBaseMessage = messageText.string
        editorBaseRelative = deadlineMode.selectedSegment == 0
        editorBaseDuration = durationField.stringValue
        editorBaseDeadline = deadlineField.stringValue
        editorBaseOccurrence = occurrencePicker.indexOfSelectedItem
        editorBaseTargetToken = selectedTarget?.token
        editorBaseSubmit = submitCheckbox.state == .on
        messageEdited = false
        deadlineEdited = false
        targetEdited = false
        submitEdited = false
        updateDeadlineSummary()
        updateControls()
    }

    private func loadEditor(_ job: ScheduleMessageJob?) {
        selectedJob = job
        suppressEditorEvents = true
        deadlineMode.selectedSegment = job == nil ? 0 : 1
        durationField.stringValue = "1h"
        if let job {
            messageText.string = job.message
            deadlineField.stringValue = Self.localFormatter.string(from: job.deadline)
            submitCheckbox.state = job.submit ? .on : .off
            targetPicker.removeAllItems()
            targetPicker.addItem(withTitle: job.targetTitle)
            targetPicker.lastItem?.isEnabled = false
            occurrencePicker.selectItem(at: 0)
            selectOccurrence(for: job.deadline)
        } else {
            messageText.string = ""
            deadlineField.stringValue = Self.localFormatter.string(from: Date().addingTimeInterval(3600))
            submitCheckbox.state = .off
            rebuildTargetPicker()
        }
        suppressEditorEvents = false
        editorBaseMessage = messageText.string
        editorBaseRelative = deadlineMode.selectedSegment == 0
        editorBaseDuration = durationField.stringValue
        editorBaseDeadline = deadlineField.stringValue
        editorBaseOccurrence = occurrencePicker.indexOfSelectedItem
        editorBaseTargetToken = selectedTarget?.token
        editorBaseSubmit = submitCheckbox.state == .on
        messageEdited = false
        deadlineEdited = false
        targetEdited = false
        submitEdited = false
        updateDeadlineSummary()
        updateControls()
    }

    private func selectOccurrence(for date: Date) {
        guard let candidates = Self.resolveWallTime(deadlineField.stringValue) else { return }
        if candidates.isRepeated, abs(candidates.first.timeIntervalSince(date)) > 1 {
            occurrencePicker.selectItem(at: 2)
        } else if candidates.isRepeated {
            occurrencePicker.selectItem(at: 1)
        } else {
            occurrencePicker.selectItem(at: 0)
        }
    }

    private func deadlineResolution() -> ScheduleDeadlineResolution? {
        guard let candidates = Self.resolveWallTime(deadlineField.stringValue) else { return nil }
        if candidates.isRepeated && occurrencePicker.indexOfSelectedItem == 0 { return nil }
        return ScheduleDeadlineResolution(
            date: candidates.isRepeated && occurrencePicker.indexOfSelectedItem == 2 ? candidates.last : candidates.first,
            first: candidates.first,
            last: candidates.last,
            isRepeated: candidates.isRepeated
        )
    }

    /// Strict syntax shared with the CLI contract: descending integer d/h/m/s units.
    static func parseDelay(_ text: String) -> UInt64? {
        guard !text.isEmpty else { return nil }
        var number: UInt64 = 0
        var digits = false
        var previous: UInt64 = UInt64.max
        var total: UInt64 = 0
        for byte in text.utf8 {
            if (48...57).contains(byte) {
                let (scaled, overflow) = number.multipliedReportingOverflow(by: 10)
                let (next, addedOverflow) = scaled.addingReportingOverflow(UInt64(byte - 48))
                guard !overflow && !addedOverflow else { return nil }
                number = next; digits = true
            } else {
                let unit: UInt64
                switch byte { case 100: unit = 86400; case 104: unit = 3600; case 109: unit = 60; case 115: unit = 1; default: return nil }
                guard digits && unit < previous else { return nil }
                let (part, overflow) = number.multipliedReportingOverflow(by: unit)
                let (sum, addedOverflow) = total.addingReportingOverflow(part)
                guard !overflow && !addedOverflow && sum <= 31_536_000 else { return nil }
                total = sum; number = 0; digits = false; previous = unit
            }
        }
        return !digits && total > 0 ? total : nil
    }

    static func countdown(to date: Date, now: Date = Date()) -> String {
        let delta = date.timeIntervalSince(now)
        if abs(delta) < 1 { return "Due now" }
        let seconds = Int(ceil(abs(delta)))
        let clock = String(format: "%02d:%02d:%02d", seconds / 3600, (seconds / 60) % 60, seconds % 60)
        return delta > 0 ? "In " + clock : clock + " overdue"
    }

    private func deadlineInput() -> [String: Any]? {
        if deadlineMode.selectedSegment == 0 {
            guard let seconds = Self.parseDelay(durationField.stringValue) else { return nil }
            return ["kind": "in_seconds", "seconds": seconds]
        }
        guard let resolution = deadlineResolution(), resolution.date > Date() else { return nil }
        return ["kind": "at", "rfc3339": Self.utcFormatter.string(from: resolution.date)]
    }

    private func updateDeadlineSummary() {
        let relative = deadlineMode.selectedSegment == 0
        durationField.isHidden = !relative
        durationPresets.isHidden = !relative
        deadlineField.isHidden = relative
        if relative {
            occurrencePicker.isHidden = true
            resolvedDeadlineLabel.stringValue = Self.parseDelay(durationField.stringValue).map {
                "Delay: \($0) seconds after remote acceptance. The helper sets the exact deadline."
            } ?? "Enter a delay from 1s to 365d, for example 5m or 1h30m."
            return
        }
        if let job = selectedJob, !deadlineEdited {
            occurrencePicker.isHidden = true
            resolvedDeadlineLabel.stringValue = "Scheduled instant: " + Self.utcFormatter.string(from: job.deadline)
            return
        }
        guard Self.parseWallTime(deadlineField.stringValue) != nil else {
            resolvedDeadlineLabel.stringValue = "Enter a valid local date and time in YYYY-MM-DD HH:mm format."
            occurrencePicker.isHidden = true
            return
        }
        guard let candidates = Self.resolveWallTime(deadlineField.stringValue) else {
            resolvedDeadlineLabel.stringValue = "This date or local time does not exist in the current calendar and time zone. Choose another time."
            occurrencePicker.isHidden = true
            return
        }
        occurrencePicker.isHidden = !candidates.isRepeated
        guard let resolution = deadlineResolution() else {
            resolvedDeadlineLabel.stringValue = "This local time occurs twice because of a daylight-saving change. Choose the first or second occurrence."
            return
        }
        let local = Self.localFormatter.string(from: resolution.date)
        let zone = TimeZone.current.identifier
        let offset = Self.offsetLabel(TimeZone.current.secondsFromGMT(for: resolution.date))
        let instant = Self.utcFormatter.string(from: resolution.date)
        let ambiguity = resolution.isRepeated
            ? " · \(occurrencePicker.indexOfSelectedItem == 1 ? "first" : "second") occurrence selected"
            : ""
        resolvedDeadlineLabel.stringValue = "\(local) \(zone) (UTC\(offset))\(ambiguity)\nResolved instant: \(instant)"
    }

    private func canBuildMutation() -> Bool {
        guard !unresolvedMutation else { return false }
        let message = messageText.string
        guard !message.isEmpty, message.utf8.count <= 4096,
              Self.hasAllowedMessageScalars(message) else { return false }
        if selectedJob == nil { return selectedTarget != nil && deadlineInput() != nil }
        guard let selectedJob, selectedJob.state == "pending" else { return false }
        if deadlineEdited && deadlineInput() == nil { return false }
        return (messageEdited && message != selectedJob.message) || deadlineEdited
    }

    private func buildMutationAction() -> [String: Any]? {
        guard canBuildMutation() else { return nil }
        let message = messageText.string
        if let job = selectedJob {
            var patch: [String: Any] = [:]
            if messageEdited && message != job.message { patch["message"] = message }
            if deadlineEdited, let deadline = deadlineInput() { patch["deadline"] = deadline }
            guard !patch.isEmpty else { return nil }
            return ["action": "edit", "job_id": job.id.uuidString.lowercased(),
                    "expected_version": job.version, "patch": patch]
        }
        guard let target = selectedTarget, let deadline = deadlineInput() else { return nil }
        return ["action": "create", "target_id": target.token, "deadline": deadline,
                "message": message, "submit": submitCheckbox.state == .on]
    }

    private static func hasAllowedMessageScalars(_ message: String) -> Bool {
        !message.unicodeScalars.contains { scalar in
            let value = scalar.value
            let isControl = value < 0x20 || (0x7F...0x9F).contains(value)
            return isControl && value != 0x09 && value != 0x0A
        }
    }

    private func prepareMutation(_ action: [String: Any], description: String) {
        guard !isWorking, !unresolvedMutation else { return }
        let requestID: UUID
        if hasOfflineDraft, let localMutation {
            requestID = localMutation.requestID
        } else {
            requestID = UUID()
        }
        do {
            let request = try Self.makeRequest(action: action, requestID: requestID)
            let mutation = ScheduleLocalMutation(requestID: requestID, request: request,
                description: description, expectedReplyKind: "mutation", outcomeUnknown: false)
            localMutation = mutation
            guard isRemoteReady else {
                statusText = "Draft saved locally in this tab. The remote job has not changed."
                statusLabel.stringValue = statusText
                updateControls()
                return
            }
            sendMutation(mutation)
        } catch {
            present(error)
        }
    }

    private func sendMutation(_ mutation: ScheduleLocalMutation) {
        guard !isWorking, !unresolvedMutation, isRemoteReady else {
            statusText = "Draft retained locally. Reconnect to the same managed SSH terminal before sending it."
            statusLabel.stringValue = statusText
            updateControls()
            return
        }
        localMutation = mutation
        beginWork("Waiting for the remote scheduler acknowledgement…")
        Task { [weak self] in
            guard let self else { return }
            do {
                let reply = try await self.sendRawRequest(mutation.request)
                guard reply.kind == mutation.expectedReplyKind else {
                    throw ScheduleBridgeFailure(code: "invalid_reply",
                        message: "The scheduler returned an unexpected \(reply.kind) reply for \(mutation.description).",
                        outcomeUnknown: true)
                }
                self.localMutation = nil
                let acknowledgedJobID: UUID?
                if let jobID = (reply.value["job"] as? [String: Any])?["id"] as? String {
                    acknowledgedJobID = UUID(uuidString: jobID)
                } else {
                    acknowledgedJobID = nil
                }
                self.messageEdited = false
                self.deadlineEdited = false
                self.targetEdited = false
                self.submitEdited = false
                self.statusText = "Remote scheduler acknowledged: \(mutation.description)."
                self.statusLabel.stringValue = self.statusText
                if reply.kind == "setup", let setup = reply.value["setup"] as? [String: Any] {
                    self.setupInfo = setup
                    self.showSetup(setup)
                }
                self.endWork()
                if reply.kind == "mutation" {
                    self.refreshFromRemote(selectingJobID: acknowledgedJobID)
                } else if reply.kind == "setup" {
                    self.refreshFromRemote()
                }
            } catch {
                if let failure = error as? ScheduleBridgeFailure, failure.outcomeUnknown {
                    self.localMutation = ScheduleLocalMutation(requestID: mutation.requestID,
                        request: mutation.request, description: mutation.description,
                        expectedReplyKind: mutation.expectedReplyKind, outcomeUnknown: true)
                } else if let failure = error as? ScheduleBridgeFailure, failure.code == "ssh_disconnected" {
                    self.localMutation = ScheduleLocalMutation(requestID: mutation.requestID,
                        request: mutation.request, description: mutation.description,
                        expectedReplyKind: mutation.expectedReplyKind, outcomeUnknown: false)
                } else if let failure = error as? ScheduleBridgeFailure, !failure.outcomeUnknown {
                    self.localMutation = nil
                } else {
                    self.localMutation = ScheduleLocalMutation(requestID: mutation.requestID,
                        request: mutation.request, description: mutation.description,
                        expectedReplyKind: mutation.expectedReplyKind, outcomeUnknown: true)
                }
                self.present(error)
                self.endWork()
            }
        }
    }

    private func sendRawRequest(_ request: Data) async throws -> ScheduleReplyData {
        guard let current = currentRoute(), current.identityNamespace == route.identityNamespace,
              current.surface == route.surface, current.connected, let session = current.expectedSession else {
            throw ScheduleBridgeFailure(code: "ssh_disconnected", message: "The SSH connection ended before the request was sent. The request remains a local draft.", outcomeUnknown: false)
        }
        let response: Data
        do {
            response = try await bridge.scheduleRequest(identityNamespace: current.identityNamespace,
                surface: current.surface, expectedSession: session, request: request)
        } catch {
            throw ScheduleBridgeFailure(code: "transport_unknown",
                message: "The bridge failed before the scheduler acknowledgement could be decoded. Check the original request status before retrying.",
                outcomeUnknown: true)
        }
        let object: Any
        do { object = try JSONSerialization.jsonObject(with: response) }
        catch {
            throw ScheduleBridgeFailure(code: "invalid_wrapper",
                message: "The scheduler response could not be decoded. Check the original request status before retrying.",
                outcomeUnknown: true)
        }
        guard let wrapper = object as? [String: Any],
              let ok = wrapper["ok"] as? Bool,
              let outcomeUnknown = wrapper["outcome_unknown"] as? Bool else {
            throw ScheduleBridgeFailure(code: "invalid_wrapper", message: "The scheduler returned an invalid response wrapper. Check the original request status before retrying.", outcomeUnknown: true)
        }
        guard ok else {
            let error = wrapper["error"] as? [String: Any] ?? [:]
            throw ScheduleBridgeFailure(code: error["code"] as? String ?? "transport_error",
                message: error["message"] as? String ?? "The scheduler request failed.", outcomeUnknown: outcomeUnknown)
        }
        guard let responseObject = wrapper["reply"] as? [String: Any],
              let result = responseObject["result"] as? [String: Any],
              let status = result["status"] as? String,
              let data = result["data"] as? [String: Any] else {
            throw ScheduleBridgeFailure(code: "invalid_reply", message: "The remote scheduler returned a malformed reply. Check the original request status before retrying.", outcomeUnknown: true)
        }
        guard status == "success" else {
            throw ScheduleBridgeFailure(code: data["code"] as? String ?? "scheduler_rejected",
                message: data["message"] as? String ?? "The remote scheduler rejected the request.",
                outcomeUnknown: (data["outcome_unknown"] as? Bool) ?? outcomeUnknown)
        }
        guard let kind = data["kind"] as? String else {
            throw ScheduleBridgeFailure(code: "invalid_reply", message: "The scheduler reply did not include its result kind. Check the original request status before retrying.", outcomeUnknown: true)
        }
        return ScheduleReplyData(kind: kind, value: data["value"] as? [String: Any] ?? data)
    }

    private static func makeRequest(action: [String: Any], requestID: UUID?) throws -> Data {
        try JSONSerialization.data(withJSONObject: [
            "protocol_version": 1,
            "request_id": requestID?.uuidString.lowercased() as Any? ?? NSNull(),
            "action": action
        ], options: [.sortedKeys])
    }

    private func checkUnresolvedStatus() {
        guard let mutation = localMutation, mutation.outcomeUnknown, !isWorking, isRemoteReady else { return }
        let isSetup = mutation.expectedReplyKind == "setup"
        beginWork(isSetup
            ? "Checking current setup state. The installation will not be repeated."
            : "Checking the original request receipt. The mutation will not be replayed.")
        Task { [weak self] in
            guard let self else { return }
            do {
                let reply: ScheduleReplyData
                if isSetup {
                    reply = try await self.perform(action: ["action": "setup_check"], requestID: nil)
                    guard reply.kind == "setup" else { throw self.invalidReply("setup check") }
                    let setup = reply.value["setup"] as? [String: Any] ?? reply.value
                    self.showSetup(setup)
                    if setup["helper_installed"] as? Bool == true,
                       setup["service_active"] as? Bool == true {
                        self.localMutation = nil
                        self.statusText = "The scheduler helper and service are active. Setup state was verified; the installation was not repeated."
                        self.statusLabel.stringValue = self.statusText
                    } else {
                        self.statusText = "Current setup state was checked. The original install result remains uncertain, so it was not retried."
                        self.statusLabel.stringValue = self.statusText
                    }
                    self.endWork()
                    return
                }
                reply = try await self.perform(action: ["action": "status", "receipt_request_id": mutation.requestID.uuidString.lowercased()], requestID: nil)
                guard reply.kind == "request_status" else { throw self.invalidReply("request status") }
                let receipt = reply.value["receipt"] as? [String: Any] ?? reply.value
                let identity = try self.originalMutationIdentity(for: mutation)
                guard let receiptRequestIDText = receipt["request_id"] as? String,
                      let receiptRequestID = UUID(uuidString: receiptRequestIDText),
                      receiptRequestID == identity.requestID,
                      receipt["operation"] as? String == identity.operation else {
                    throw self.unresolvedReceiptFailure()
                }
                let receiptState = receipt["state"] as? String ?? receipt["status"] as? String
                guard let receiptState, ["applied", "rejected"].contains(receiptState) else {
                    self.statusText = "The helper cannot prove whether this request was applied. It will not be sent again. Inspect the remote queue before creating another job."
                    self.statusLabel.stringValue = self.statusText
                    self.endWork()
                    return
                }
                let receiptJobID = (receipt["job_id"] as? String).flatMap(UUID.init(uuidString:))
                if identity.operation == "edit" || identity.operation == "cancel" {
                    guard let expectedJobID = identity.jobID, receiptJobID == expectedJobID else {
                        throw self.unresolvedReceiptFailure()
                    }
                }
                if receiptState == "rejected" {
                    self.localMutation = nil
                    self.statusText = "Request receipt found (rejected). Refreshing remote queue and history; the attempted editor draft is retained."
                    self.statusLabel.stringValue = self.statusText
                    self.endWork()
                    self.refreshFromRemote()
                    return
                }
                guard let receiptJobID else { throw self.unresolvedReceiptFailure() }
                let jobReply = try await self.performRead(action: [
                    "action": "show",
                    "job_id": receiptJobID.uuidString.lowercased()
                ])
                guard jobReply.kind == "job",
                      let jobValue = jobReply.value["job"] as? [String: Any] else {
                    throw self.unresolvedReceiptFailure()
                }
                let job = try self.parseJob(jobValue)
                guard job.id == receiptJobID else { throw self.unresolvedReceiptFailure() }
                self.statusText = "Applied request receipt verified. Loading the created job and refreshing remote queue and history."
                self.statusLabel.stringValue = self.statusText
                self.endWork()
                self.refreshFromRemote(selectingJobID: receiptJobID, preloadedJob: job,
                    resolvingMutationRequestID: mutation.requestID)
                return
            } catch {
                self.present(error)
            }
            self.endWork()
        }
    }

    private func originalMutationIdentity(for mutation: ScheduleLocalMutation) throws -> ScheduleMutationIdentity {
        guard mutation.expectedReplyKind == "mutation",
              let request = try? JSONSerialization.jsonObject(with: mutation.request) as? [String: Any],
              let requestIDText = request["request_id"] as? String,
              let requestID = UUID(uuidString: requestIDText),
              requestID == mutation.requestID,
              let action = request["action"] as? [String: Any],
              let operation = action["action"] as? String,
              ["create", "edit", "cancel"].contains(operation) else {
            throw unresolvedReceiptFailure()
        }
        var jobID: UUID?
        if operation == "edit" || operation == "cancel" {
            guard let jobIDText = action["job_id"] as? String,
                  let parsedJobID = UUID(uuidString: jobIDText) else {
                throw unresolvedReceiptFailure()
            }
            jobID = parsedJobID
        }
        return ScheduleMutationIdentity(requestID: requestID, operation: operation, jobID: jobID)
    }

    private func unresolvedReceiptFailure() -> ScheduleBridgeFailure {
        ScheduleBridgeFailure(code: "unmatched_request_receipt",
            message: "The scheduler returned an incomplete or mismatched receipt. The original request remains unresolved; do not submit it again.",
            outcomeUnknown: true)
    }

    private func showSetup(_ value: [String: Any]) {
        setupInfo = value
        let os = value["os"] as? String ?? "unknown host"
        let message = value["message"] as? String ?? "Setup check completed."
        let installed = (value["helper_installed"] as? Bool).map { $0 ? "installed" : "not installed" } ?? "unknown helper state"
        let active = (value["service_active"] as? Bool).map { $0 ? "active" : "inactive" } ?? "manual daemon state unavailable"
        statusText = "\(os) · helper \(installed) · service \(active). \(message)"
        statusLabel.stringValue = statusText
        updateControls()
    }

    private func parseDisplayDate(_ date: Date) -> String { Self.localFormatter.string(from: date) }

    @objc private func deadlineChanged() {
        guard !suppressEditorEvents else { return }
        deadlineEdited = (deadlineMode.selectedSegment == 0) != editorBaseRelative
            || (deadlineMode.selectedSegment == 0 ? durationField.stringValue != editorBaseDuration
                : deadlineField.stringValue != editorBaseDeadline || occurrencePicker.indexOfSelectedItem != editorBaseOccurrence)
        invalidateUnsentDraftForEditorChange()
        updateDeadlineSummary()
        updateControls()
    }

    @objc private func occurrenceChanged() {
        guard !suppressEditorEvents else { return }
        deadlineEdited = (deadlineMode.selectedSegment == 0) != editorBaseRelative
            || (deadlineMode.selectedSegment == 0 ? durationField.stringValue != editorBaseDuration
                : deadlineField.stringValue != editorBaseDeadline || occurrencePicker.indexOfSelectedItem != editorBaseOccurrence)
        invalidateUnsentDraftForEditorChange()
        updateDeadlineSummary()
        updateControls()
    }

    @objc private func editorChanged() {
        guard !suppressEditorEvents else { return }
        submitEdited = (submitCheckbox.state == .on) != editorBaseSubmit
        invalidateUnsentDraftForEditorChange()
        updateControls()
    }

    private func invalidateUnsentDraftForEditorChange() {
        if localMutation?.outcomeUnknown == false { localMutation = nil }
    }

    func textDidBeginEditing(_ notification: Notification) { onFocus?() }
    func controlTextDidBeginEditing(_ notification: Notification) { onFocus?() }

    func textDidChange(_ notification: Notification) {
        guard !suppressEditorEvents else { return }
        messageEdited = messageText.string != editorBaseMessage
        invalidateUnsentDraftForEditorChange()
        updateControls()
    }

    func controlTextDidChange(_ notification: Notification) {
        guard let field = notification.object as? NSTextField, field === deadlineField || field === durationField else { return }
        deadlineChanged()
    }

    @objc private func presetPressed() {
        guard durationPresets.selectedSegment >= 0 else { return }
        durationField.stringValue = ["1m", "5m", "15m", "1h"][durationPresets.selectedSegment]
        deadlineMode.selectedSegment = 0
        deadlineChanged()
    }

    @objc private func timeDisplayChanged() {
        // Presentation only: never refresh the queue, alter selection or touch drafts.
        table.reloadData(forRowIndexes: IndexSet(integersIn: 0..<visibleJobs.count),
                         columnIndexes: IndexSet(integer: 2))
    }

    @objc private func refreshPressed() { refreshFromRemote() }

    @objc private func checkSetupPressed() {
        guard !isWorking else { return }
        beginWork("Checking helper compatibility and service state on the remote host…")
        Task { [weak self] in
            guard let self else { return }
            do {
                let reply = try await self.perform(action: ["action": "setup_check"], requestID: nil)
                guard reply.kind == "setup" else { throw self.invalidReply("setup check") }
                let setup = reply.value["setup"] as? [String: Any] ?? reply.value
                self.showSetup(setup)
            } catch { self.present(error) }
            self.endWork()
        }
    }

    @objc private func installSetupPressed() {
        guard !isWorking, let setupInfo, setupInfo["supported"] as? Bool == true,
              (setupInfo["os"] as? String)?.lowercased() == "linux" else { return }
        let requestID = UUID()
        do {
            let request = try Self.makeRequest(action: ["action": "setup_install"], requestID: requestID)
            sendMutation(ScheduleLocalMutation(requestID: requestID, request: request,
                description: "scheduler setup", expectedReplyKind: "setup", outcomeUnknown: false))
        } catch { present(error) }
    }

    @objc private func viewModeChanged() {
        guard !isWorking else { return }
        guard !hasUnsavedEditorChanges else {
            viewMode.selectedSegment = isHistory ? 1 : 0
            showMessage("Save or explicitly discard the current editor changes before switching list views.")
            return
        }
        isHistory = viewMode.selectedSegment == 1
        table.deselectAll(nil)
        loadEditor(nil)
        table.reloadData()
        updateControls()
    }

    @objc private func targetChanged() {
        draftTargetToken = selectedTarget?.token
        targetEdited = draftTargetToken != editorBaseTargetToken
        invalidateUnsentDraftForEditorChange()
        updateControls()
    }

    @objc private func newPressed() {
        guard !unresolvedMutation else { return }
        guard !hasUnsavedEditorChanges else {
            showMessage("Save or explicitly discard the current editor changes before starting another message.")
            return
        }
        isHistory = false
        viewMode.selectedSegment = 0
        table.deselectAll(nil)
        table.reloadData()
        beginNewMessage()
    }

    @objc private func discardDraftPressed() {
        guard hasUnsavedEditorChanges, !isWorking, !unresolvedMutation else { return }
        let discardedOfflineDraft = hasOfflineDraft
        localMutation = nil
        if selectedJob == nil {
            beginNewMessage()
        } else {
            loadEditor(selectedJob)
        }
        showMessage(discardedOfflineDraft ? "Local draft discarded. No remote job was changed." : "Editor changes discarded.")
        updateControls()
    }

    @objc private func savePressed() {
        guard !unresolvedMutation, !isWorking else { return }
        if hasOfflineDraft, let localMutation {
            sendMutation(localMutation)
            return
        }
        guard let action = buildMutationAction() else { return }
        prepareMutation(action, description: selectedJob == nil ? "scheduled message creation" : "pending job edit")
    }

    @objc private func cancelPressed() {
        guard let job = selectedJob, job.state == "pending", !isHistory, !isWorking, !unresolvedMutation else { return }
        guard !hasUnsavedEditorChanges else {
            showMessage("Save or discard the current editor changes before cancelling this job.")
            return
        }
        let action: [String: Any] = ["action": "cancel", "job_id": job.id.uuidString.lowercased(), "expected_version": job.version]
        prepareMutation(action, description: "job cancellation")
    }

    @objc private func requestStatusPressed() { checkUnresolvedStatus() }

    func numberOfRows(in tableView: NSTableView) -> Int { visibleJobs.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard visibleJobs.indices.contains(row), let identifier = tableColumn?.identifier.rawValue else { return nil }
        let job = visibleJobs[row]
        let value: String
        switch identifier {
        case "message": value = job.message
        case "target": value = job.targetTitle
        case "deadline": value = !isHistory && timeDisplay.selectedSegment == 0 ? Self.countdown(to: job.deadline) : parseDisplayDate(job.deadline)
        case "state": value = "\(job.state) · v\(job.version)"
        default: return nil
        }
        let cell = NSTableCellView()
        let text = NSTextField(labelWithString: value)
        text.lineBreakMode = .byTruncatingTail
        text.toolTip = value
        text.font = .systemFont(ofSize: 11)
        text.translatesAutoresizingMaskIntoConstraints = false
        cell.textField = text
        cell.addSubview(text)
        NSLayoutConstraint.activate([
            text.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 4),
            text.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -4),
            text.centerYAnchor.constraint(equalTo: cell.centerYAnchor)
        ])
        return cell
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        guard !suppressEditorEvents, !isWorking else { return }
        onFocus?()
        if hasUnsavedEditorChanges {
            suppressEditorEvents = true
            if let selectedJob, let index = visibleJobs.firstIndex(where: { $0.id == selectedJob.id }) {
                table.selectRowIndexes(IndexSet(integer: index), byExtendingSelection: false)
            } else {
                table.deselectAll(nil)
            }
            suppressEditorEvents = false
            showMessage("Send or explicitly discard the current local draft before selecting another message.")
            return
        }
        let row = table.selectedRow
        guard visibleJobs.indices.contains(row) else {
            if selectedJob != nil { beginNewMessage() }
            return
        }
        loadEditor(visibleJobs[row])
    }

    private func canEditSelectedJob() -> Bool { selectedJob?.state == "pending" && !isHistory }

    private func displayJob(_ job: ScheduleMessageJob) {
        statusText = "\(job.state.capitalized) · version \(job.version) · \(job.targetTitle)"
        statusLabel.stringValue = statusText
    }

    private func showMessage(_ message: String) {
        statusText = message
        statusLabel.stringValue = message
    }

    private func updateVisibleRowsAfterMutation() {
        table.reloadData()
        updateControls()
    }

    private static func stringValue(_ value: Any?) -> String? {
        if let value = value as? String { return value }
        if let value = value as? NSNumber { return value.stringValue }
        return nil
    }

    private static func uint64Value(_ value: Any?) -> UInt64? {
        if let value = value as? UInt64 { return value }
        if let value = value as? NSNumber, value.int64Value >= 0 { return value.uint64Value }
        return nil
    }

    private static func parseDate(_ string: String) -> Date? {
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let date = fractional.date(from: string) { return date }
        let regular = ISO8601DateFormatter()
        regular.formatOptions = [.withInternetDateTime]
        return regular.date(from: string)
    }

    private static let utcFormatter: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        formatter.formatOptions = [.withInternetDateTime]
        return formatter
    }()

    private static let localFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone.current
        formatter.dateFormat = "yyyy-MM-dd HH:mm"
        return formatter
    }()

    private static func offsetLabel(_ seconds: Int) -> String {
        let sign = seconds < 0 ? "−" : "+"
        let magnitude = abs(seconds)
        return String(format: "%@%02d:%02d", sign, magnitude / 3600, (magnitude % 3600) / 60)
    }

    private static func parseWallTime(_ text: String) -> ScheduleWallTimeParts? {
        let bytes = Array(text.utf8)
        guard bytes.count == 16,
              bytes[4] == 45, bytes[7] == 45, bytes[10] == 32, bytes[13] == 58 else { return nil }
        for index in [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15] {
            guard (48...57).contains(bytes[index]) else { return nil }
        }
        func integer(_ range: Range<Int>) -> Int? {
            Int(String(decoding: bytes[range], as: UTF8.self))
        }
        guard let year = integer(0..<4), let month = integer(5..<7), let day = integer(8..<10),
              let hour = integer(11..<13), let minute = integer(14..<16),
              (1...12).contains(month), (1...31).contains(day), (0...23).contains(hour),
              (0...59).contains(minute) else { return nil }
        return ScheduleWallTimeParts(year: year, month: month, day: day, hour: hour, minute: minute)
    }

    private static func resolveWallTime(_ text: String) -> ScheduleWallTimeCandidates? {
        guard let parts = parseWallTime(text) else { return nil }
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone.current
        guard let day = calendar.date(from: DateComponents(year: parts.year, month: parts.month,
                                                            day: parts.day, hour: 12)),
              calendar.component(.year, from: day) == parts.year,
              calendar.component(.month, from: day) == parts.month,
              calendar.component(.day, from: day) == parts.day else { return nil }
        let start = day.addingTimeInterval(-48 * 60 * 60)
        var matching = DateComponents()
        matching.year = parts.year
        matching.month = parts.month
        matching.day = parts.day
        matching.hour = parts.hour
        matching.minute = parts.minute
        matching.second = 0
        let first = calendar.nextDate(after: start, matching: matching, matchingPolicy: .strict,
                                      repeatedTimePolicy: .first, direction: .forward)
        let last = calendar.nextDate(after: start, matching: matching, matchingPolicy: .strict,
                                     repeatedTimePolicy: .last, direction: .forward)
        guard let first, let last,
              calendar.component(.year, from: first) == parts.year,
              calendar.component(.month, from: first) == parts.month,
              calendar.component(.day, from: first) == parts.day,
              calendar.component(.hour, from: first) == parts.hour,
              calendar.component(.minute, from: first) == parts.minute else { return nil }
        let repeated = abs(first.timeIntervalSince(last)) > 1
        return ScheduleWallTimeCandidates(first: first, last: last, isRepeated: repeated)
    }
}

private struct ScheduleDeadlineResolution {
    let date: Date
    let first: Date
    let last: Date
    let isRepeated: Bool
}

private struct ScheduleWallTimeParts {
    let year: Int
    let month: Int
    let day: Int
    let hour: Int
    let minute: Int
}

private struct ScheduleWallTimeCandidates {
    let first: Date
    let last: Date
    let isRepeated: Bool
}

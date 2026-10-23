import AppKit

private final class ScheduleInspectorDocumentView: NSView {
    override var isFlipped: Bool { true }
}

/// The native presentation owner, shared by the scheduler controller and AppKit layout checks.
/// It retains the same controls when the modeless inspector is collapsed or the tab is moved.
@MainActor
final class ScheduleMessagesView: NSView {
    let destinationLabel = NSTextField(labelWithString: "")
    let connectionLabel = NSTextField(labelWithString: "")
    let statusLabel = NSTextField(labelWithString: "Not checked")
    let refreshButton = NSButton(title: "Refresh", target: nil, action: nil)
    let setupCheckButton = NSButton(title: "Check Setup", target: nil, action: nil)
    let setupInstallButton = NSButton(title: "Install / Enable", target: nil, action: nil)
    let viewMode = NSSegmentedControl(labels: ["Pending", "History"], trackingMode: .selectOne, target: nil, action: nil)
    let queueScroll = ScheduleQueueScrollView(frame: .zero)
    var table: NSTableView { queueScroll.table }
    let targetPicker = NSPopUpButton(frame: .zero, pullsDown: false)
    let messageText = NSTextView(frame: NSRect(x: 0, y: 0, width: 320, height: 180))
    let deadlineMode = NSSegmentedControl(labels: ["In", "At"], trackingMode: .selectOne, target: nil, action: nil)
    let durationField = NSTextField(string: "1h")
    let durationPresets = NSSegmentedControl(labels: ["1m", "5m", "15m", "1h"], trackingMode: .momentary, target: nil, action: nil)
    let timeDisplay = NSSegmentedControl(labels: ["Relative", "Absolute"], trackingMode: .selectOne, target: nil, action: nil)
    let deadlineField = NSTextField(string: "")
    let occurrencePicker = NSPopUpButton(frame: .zero, pullsDown: false)
    let resolvedDeadlineLabel = NSTextField(wrappingLabelWithString: "")
    let submitCheckbox = NSButton(checkboxWithTitle: "Press Return after the message is sent", target: nil, action: nil)
    let newButton = NSButton(title: "New", target: nil, action: nil)
    let discardDraftButton = NSButton(title: "Discard Draft", target: nil, action: nil)
    let saveButton = NSButton(title: "Schedule Message", target: nil, action: nil)
    let cancelButton = NSButton(title: "Cancel Job", target: nil, action: nil)
    let requestStatusButton = NSButton(title: "Check Request Status", target: nil, action: nil)

    let editorHeading = NSTextField(labelWithString: "New scheduled message")
    let editorState = NSTextField(labelWithString: "Draft")
    let queueCountLabel = NSTextField(labelWithString: "0 pending messages")
    let nextDeliveryLabel = NSTextField(labelWithString: "No pending messages")
    let setupButton = NSButton(image: NSImage(systemSymbolName: "info.circle", accessibilityDescription: nil) ?? NSImage(), target: nil, action: nil)
    let inspectorButton = NSButton(image: NSImage(systemSymbolName: "sidebar.right", accessibilityDescription: nil) ?? NSImage(), target: nil, action: nil)
    let setupPopover = NSPopover()
    private let statusDetails = NSTextField(wrappingLabelWithString: "Not checked")
    private let connectionSummary = NSTextField(labelWithString: "Disconnected")
    private let splitController = NSSplitViewController()
    private var inspectorItem: NSSplitViewItem?
    private var positionedDivider = false
    private var inspectorWidthBeforeCollapse: CGFloat = 358
    var split: NSSplitView { splitController.splitView }
    var isInspectorVisible: Bool { inspectorItem?.isCollapsed == false }
    override var isFlipped: Bool { true }
    override var isOpaque: Bool { true }

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        configurePresentation()
        installLayout()
    }

    required init?(coder: NSCoder) { nil }

    private func configurePresentation() {
        setAccessibilityLabel("Schedules tab")
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
        resolvedDeadlineLabel.font = .systemFont(ofSize: 11)
        resolvedDeadlineLabel.textColor = .secondaryLabelColor
        resolvedDeadlineLabel.maximumNumberOfLines = 3
        resolvedDeadlineLabel.setContentHuggingPriority(.required, for: .vertical)
        for control in [targetPicker, durationPresets, occurrencePicker] as [NSView] {
            control.setContentHuggingPriority(.required, for: .vertical)
        }
        deadlineField.font = .monospacedDigitSystemFont(ofSize: 13, weight: .regular)
        for control in [refreshButton, setupCheckButton, setupInstallButton, newButton,
                        discardDraftButton, saveButton, cancelButton, requestStatusButton,
                        setupButton, inspectorButton] {
            control.bezelStyle = .rounded
            control.controlSize = .small
            control.font = .systemFont(ofSize: 12)
        }
        for control in [viewMode, deadlineMode, durationPresets, timeDisplay] {
            control.controlSize = .small
            control.font = .systemFont(ofSize: 12)
        }
        for label in [connectionSummary, queueCountLabel, nextDeliveryLabel, editorState] {
            label.font = .systemFont(ofSize: 11)
            label.textColor = .secondaryLabelColor
            label.lineBreakMode = .byTruncatingTail
        }
        destinationLabel.lineBreakMode = .byTruncatingMiddle
        destinationLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        destinationLabel.setContentHuggingPriority(.defaultLow, for: .horizontal)
        editorHeading.font = .systemFont(ofSize: 13, weight: .semibold)
        newButton.title = "New message"
        newButton.image = NSImage(systemSymbolName: "plus", accessibilityDescription: nil)
        newButton.imagePosition = .imageLeading
        refreshButton.title = ""
        refreshButton.image = NSImage(systemSymbolName: "arrow.clockwise", accessibilityDescription: nil)
        refreshButton.toolTip = "Refresh scheduled messages"
        setupButton.bezelStyle = .inline
        setupButton.isBordered = false
        setupButton.target = self
        setupButton.action = #selector(toggleSetup)
        setupButton.setAccessibilityLabel("Show remote scheduler setup details")
        setupButton.toolTip = "Remote scheduler setup and complete status"
        inspectorButton.setButtonType(.pushOnPushOff)
        inspectorButton.state = .on
        inspectorButton.target = self
        inspectorButton.action = #selector(toggleInspector)
        inspectorButton.setAccessibilityLabel("Show or hide scheduled message inspector")
        inspectorButton.toolTip = "Show or hide inspector"
        statusLabel.font = .systemFont(ofSize: 11)
        statusLabel.textColor = .secondaryLabelColor
        statusLabel.lineBreakMode = .byTruncatingTail
        statusLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        statusLabel.setContentHuggingPriority(.defaultLow, for: .horizontal)
        nextDeliveryLabel.setContentHuggingPriority(.required, for: .horizontal)
        requestStatusButton.setContentHuggingPriority(.required, for: .horizontal)
        submitCheckbox.title = "Press Return in tmux after sending"
        submitCheckbox.font = .systemFont(ofSize: 12)
        targetPicker.controlSize = .small
        durationField.font = .systemFont(ofSize: 12)
        occurrencePicker.controlSize = .small
        for field in [durationField, deadlineField] {
            field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        }
        setupPopover.behavior = .transient
        let detailController = NSViewController()
        let detailView = NSView()
        detailView.frame = NSRect(x: 0, y: 0, width: 370, height: 280)
        detailView.setAccessibilityLabel("Remote scheduler setup details")
        detailController.view = detailView
        connectionLabel.font = .systemFont(ofSize: 12)
        connectionLabel.maximumNumberOfLines = 4
        connectionLabel.cell?.wraps = true
        statusDetails.font = .systemFont(ofSize: 11)
        statusDetails.textColor = .secondaryLabelColor
        statusDetails.maximumNumberOfLines = 8
        let title = label("Remote scheduler", weight: .semibold)
        let details = column([title, connectionLabel, statusDetails,
                              row([setupCheckButton, setupInstallButton])], spacing: 12)
        pin(details, in: detailView, inset: 16)
        detailView.widthAnchor.constraint(equalToConstant: 370).isActive = true
        for field in [connectionLabel, statusDetails] {
            field.widthAnchor.constraint(equalTo: details.widthAnchor).isActive = true
        }
        setupPopover.contentViewController = detailController
        setupPopover.contentSize = NSSize(width: 370, height: 280)
    }

    private func installLayout() {
        let toolbar = NSView()
        let title = label("Schedules", weight: .semibold)
        let titleIcon = NSImageView(image: NSImage(systemSymbolName: "clock", accessibilityDescription: nil) ?? NSImage())
        let toolbarRow = row([titleIcon, title, separator(vertical: true), destinationLabel,
                              connectionSummary, spacer(), viewMode, separator(vertical: true),
                              refreshButton, inspectorButton, newButton], spacing: 9)
        pin(toolbarRow, in: toolbar, inset: 10, verticalInset: 7)
        toolbar.heightAnchor.constraint(equalToConstant: 42).isActive = true
        refreshButton.widthAnchor.constraint(equalToConstant: 28).isActive = true
        inspectorButton.widthAnchor.constraint(equalToConstant: 28).isActive = true
        titleIcon.widthAnchor.constraint(equalToConstant: 14).isActive = true
        titleIcon.heightAnchor.constraint(equalToConstant: 14).isActive = true
        viewMode.widthAnchor.constraint(equalToConstant: 164).isActive = true

        let queuePane = NSView()
        let queueFooter = NSView()
        let displayTitle = label("Show time:", size: 11)
        displayTitle.textColor = .secondaryLabelColor
        pin(row([queueCountLabel, spacer(), displayTitle, timeDisplay], spacing: 7), in: queueFooter, inset: 9, verticalInset: 3)
        queueFooter.heightAnchor.constraint(equalToConstant: 27).isActive = true
        let queueContent = column([queueScroll, separator(), queueFooter], spacing: 0)
        pin(queueContent, in: queuePane)
        queueScroll.widthAnchor.constraint(equalTo: queueContent.widthAnchor).isActive = true
        queueFooter.widthAnchor.constraint(equalTo: queueContent.widthAnchor).isActive = true
        queueScroll.setContentHuggingPriority(.defaultLow, for: .vertical)
        queueScroll.setContentCompressionResistancePriority(.defaultLow, for: .vertical)

        let inspector = NSView()
        let editorScroll = NSScrollView()
        editorScroll.hasVerticalScroller = true
        editorScroll.hasHorizontalScroller = false
        editorScroll.autohidesScrollers = true
        editorScroll.drawsBackground = false
        editorScroll.setAccessibilityLabel("Scheduled message inspector form")
        let document = ScheduleInspectorDocumentView()
        document.translatesAutoresizingMaskIntoConstraints = false
        editorScroll.documentView = document
        let messageScroll = NSScrollView()
        messageScroll.hasVerticalScroller = true
        messageScroll.hasHorizontalScroller = false
        messageScroll.autohidesScrollers = true
        messageScroll.drawsBackground = true
        messageScroll.borderType = .bezelBorder
        messageScroll.documentView = messageText
        messageScroll.setAccessibilityLabel("Scheduled message editor scroll area")
        let minimumMessage = messageScroll.heightAnchor.constraint(greaterThanOrEqualToConstant: 100)
        minimumMessage.isActive = true
        let preferredMessage = messageScroll.heightAnchor.constraint(equalToConstant: 140)
        preferredMessage.priority = .defaultLow
        preferredMessage.isActive = true
        let targetGroup = column([label("Remote tmux pane"), targetPicker], spacing: 5)
        let messageGroup = column([label("Message"), messageScroll], spacing: 5)
        let timeEntry = row([deadlineMode, durationField, deadlineField, spacer()], spacing: 8)
        // A stack has no intrinsic height; spare space belongs to the message editor.
        timeEntry.heightAnchor.constraint(lessThanOrEqualToConstant: 28).isActive = true
        let deadlineGroup = column([label("When"), timeEntry, durationPresets, occurrencePicker, resolvedDeadlineLabel], spacing: 6)
        let editorTitleRow = row([editorHeading, spacer(), editorState])
        editorTitleRow.heightAnchor.constraint(equalToConstant: 18).isActive = true
        editorTitleRow.setContentHuggingPriority(.required, for: .vertical)
        submitCheckbox.setContentHuggingPriority(.required, for: .vertical)
        let fields = column([editorTitleRow, targetGroup,
                             messageGroup, deadlineGroup, submitCheckbox], spacing: 12)
        pin(fields, in: document, inset: 16, verticalInset: 14)
        for group in [targetGroup, messageGroup, deadlineGroup] {
            group.widthAnchor.constraint(equalTo: fields.widthAnchor).isActive = true
        }
        fields.arrangedSubviews[0].widthAnchor.constraint(equalTo: fields.widthAnchor).isActive = true
        for control in [targetPicker, messageScroll] as [NSView] {
            control.widthAnchor.constraint(equalTo: targetGroup.widthAnchor).isActive = true
        }
        resolvedDeadlineLabel.widthAnchor.constraint(equalTo: fields.widthAnchor).isActive = true
        timeEntry.widthAnchor.constraint(lessThanOrEqualTo: fields.widthAnchor).isActive = true
        deadlineMode.widthAnchor.constraint(equalToConstant: 64).isActive = true
        durationPresets.widthAnchor.constraint(equalToConstant: 150).isActive = true
        timeDisplay.widthAnchor.constraint(equalToConstant: 132).isActive = true
        durationField.widthAnchor.constraint(equalToConstant: 104).isActive = true
        deadlineField.widthAnchor.constraint(equalToConstant: 206).isActive = true
        NSLayoutConstraint.activate([
            document.leadingAnchor.constraint(equalTo: editorScroll.contentView.leadingAnchor),
            document.topAnchor.constraint(equalTo: editorScroll.contentView.topAnchor),
            document.widthAnchor.constraint(equalTo: editorScroll.contentView.widthAnchor),
            document.heightAnchor.constraint(greaterThanOrEqualTo: editorScroll.contentView.heightAnchor)
        ])
        let fitHeight = document.heightAnchor.constraint(equalTo: editorScroll.contentView.heightAnchor)
        fitHeight.priority = .defaultLow
        fitHeight.isActive = true
        messageGroup.setContentHuggingPriority(.defaultLow, for: .vertical)
        messageScroll.setContentHuggingPriority(.defaultLow, for: .vertical)
        for group in [targetGroup, deadlineGroup] {
            group.setContentHuggingPriority(.required, for: .vertical)
        }

        let actions = NSView()
        pin(row([cancelButton, spacer(), discardDraftButton, saveButton], spacing: 7), in: actions, inset: 12, verticalInset: 8)
        actions.heightAnchor.constraint(equalToConstant: 40).isActive = true
        let inspectorContent = column([editorScroll, separator(), actions], spacing: 0)
        pin(inspectorContent, in: inspector)
        editorScroll.widthAnchor.constraint(equalTo: inspectorContent.widthAnchor).isActive = true
        actions.widthAnchor.constraint(equalTo: inspectorContent.widthAnchor).isActive = true
        editorScroll.setContentHuggingPriority(.defaultLow, for: .vertical)
        editorScroll.setContentCompressionResistancePriority(.defaultLow, for: .vertical)

        let queueController = NSViewController()
        queueController.view = queuePane
        let editorController = NSViewController()
        editorController.view = inspector
        let queueItem = NSSplitViewItem(viewController: queueController)
        queueItem.minimumThickness = 300
        queueItem.holdingPriority = NSLayoutConstraint.Priority(rawValue: 249)
        let editorItem = NSSplitViewItem(viewController: editorController)
        editorItem.minimumThickness = 344
        editorItem.maximumThickness = 520
        editorItem.holdingPriority = NSLayoutConstraint.Priority(rawValue: 251)
        editorItem.canCollapse = true
        editorItem.collapseBehavior = .preferResizingSiblingsWithFixedSplitView
        inspectorItem = editorItem
        split.isVertical = true
        split.dividerStyle = .thin
        split.setAccessibilityLabel("Scheduled message queue and editor")
        splitController.addSplitViewItem(queueItem)
        splitController.addSplitViewItem(editorItem)
        NotificationCenter.default.addObserver(self, selector: #selector(splitResized),
                                               name: NSSplitView.didResizeSubviewsNotification, object: split)

        let status = NSView()
        pin(row([setupButton, statusLabel, requestStatusButton, nextDeliveryLabel], spacing: 7), in: status, inset: 10, verticalInset: 3)
        status.heightAnchor.constraint(equalToConstant: 27).isActive = true
        let all = column([toolbar, separator(), splitController.view, separator(), status], spacing: 0)
        pin(all, in: self)
        for section in [toolbar, splitController.view, status] {
            section.widthAnchor.constraint(equalTo: all.widthAnchor).isActive = true
        }
        splitController.view.setContentHuggingPriority(.defaultLow, for: .vertical)
        splitController.view.setContentCompressionResistancePriority(.defaultLow, for: .vertical)
    }

    override func layout() {
        super.layout()
        if !positionedDivider, split.bounds.width > 645 {
            positionedDivider = true
            split.setPosition(split.bounds.width - 359, ofDividerAt: 0)
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.windowBackgroundColor.setFill()
        dirtyRect.fill()
    }

    func presentStatus(_ message: String) {
        statusLabel.stringValue = message
        statusLabel.toolTip = message
        statusLabel.setAccessibilityHelp(message)
        statusDetails.stringValue = message
        statusDetails.toolTip = message
    }

    func presentConnection(connected: Bool) {
        connectionSummary.stringValue = connected ? "Connected" : "Disconnected"
        connectionSummary.textColor = connected ? .secondaryLabelColor : .systemOrange
    }

    func showInspector() {
        let wasCollapsed = inspectorItem?.isCollapsed == true
        inspectorItem?.isCollapsed = false
        inspectorButton.state = .on
        if wasCollapsed {
            split.layoutSubtreeIfNeeded()
            split.setPosition(max(300, split.bounds.width - inspectorWidthBeforeCollapse - split.dividerThickness), ofDividerAt: 0)
        }
    }

    func closeSetup() { setupPopover.close() }

    @objc private func toggleInspector() {
        guard let inspectorItem else { return }
        if inspectorItem.isCollapsed { showInspector() }
        else {
            inspectorWidthBeforeCollapse = split.arrangedSubviews.last?.frame.width ?? 358
            inspectorItem.isCollapsed = true
            inspectorButton.state = .off
            window?.makeFirstResponder(table)
        }
    }

    @objc private func splitResized(_ notification: Notification) {
        inspectorButton.state = isInspectorVisible ? .on : .off
    }

    @objc private func toggleSetup() {
        if setupPopover.isShown { setupPopover.close() }
        else { setupPopover.show(relativeTo: setupButton.bounds, of: setupButton, preferredEdge: .maxY) }
    }

    private func label(_ text: String, size: CGFloat = 12, weight: NSFont.Weight = .regular) -> NSTextField {
        let field = NSTextField(labelWithString: text)
        field.font = .systemFont(ofSize: size, weight: weight)
        field.setContentHuggingPriority(.required, for: .vertical)
        return field
    }

    private func spacer() -> NSView {
        let result = NSView()
        result.setContentHuggingPriority(.defaultLow, for: .horizontal)
        result.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return result
    }

    private func separator(vertical: Bool = false) -> NSBox {
        let result = NSBox()
        result.boxType = .separator
        result.translatesAutoresizingMaskIntoConstraints = false
        if vertical {
            result.widthAnchor.constraint(equalToConstant: 1).isActive = true
            result.heightAnchor.constraint(equalToConstant: 18).isActive = true
        }
        else { result.heightAnchor.constraint(equalToConstant: 1).isActive = true }
        return result
    }

    private func column(_ views: [NSView], spacing: CGFloat) -> NSStackView {
        let stack = NSStackView(views: views)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = spacing
        stack.distribution = .fill
        stack.translatesAutoresizingMaskIntoConstraints = false
        for item in views where item is NSBox {
            item.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        }
        return stack
    }

    private func row(_ views: [NSView], spacing: CGFloat = 8) -> NSStackView {
        let stack = NSStackView(views: views)
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = spacing
        stack.distribution = .fill
        stack.translatesAutoresizingMaskIntoConstraints = false
        return stack
    }

    private func pin(_ child: NSView, in parent: NSView, inset: CGFloat = 0, verticalInset: CGFloat? = nil) {
        parent.addSubview(child)
        child.translatesAutoresizingMaskIntoConstraints = false
        let vertical = verticalInset ?? inset
        NSLayoutConstraint.activate([
            child.leadingAnchor.constraint(equalTo: parent.leadingAnchor, constant: inset),
            child.trailingAnchor.constraint(equalTo: parent.trailingAnchor, constant: -inset),
            child.topAnchor.constraint(equalTo: parent.topAnchor, constant: vertical),
            child.bottomAnchor.constraint(equalTo: parent.bottomAnchor, constant: -vertical)
        ])
    }
}

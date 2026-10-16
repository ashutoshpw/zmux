import AppKit

@MainActor
enum ZmuxPalette {
    static let window = NSColor(calibratedWhite: 0.075, alpha: 1)
    static let sidebar = NSColor(calibratedWhite: 0.095, alpha: 1)
    static let terminal = NSColor(calibratedRed: 28 / 255, green: 28 / 255, blue: 30 / 255, alpha: 1)
    static let chrome = NSColor(calibratedWhite: 0.105, alpha: 1)
    static let separator = NSColor(calibratedWhite: 1, alpha: 0.085)
    static let selection = NSColor(calibratedRed: 0.09, green: 0.36, blue: 0.78, alpha: 1)
    static let selectionBorder = NSColor(calibratedRed: 0.24, green: 0.55, blue: 1, alpha: 0.45)
    static let subdued = NSColor(calibratedWhite: 0.64, alpha: 1)
    static let success = NSColor(calibratedRed: 0.28, green: 0.78, blue: 0.46, alpha: 1)
    static let inactive = NSColor(calibratedWhite: 0.38, alpha: 1)
}

@MainActor
class BackgroundView: NSView {
    init(color: NSColor) {
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = color.cgColor
    }

    required init?(coder: NSCoder) { nil }
}

@MainActor
class DraggableChromeView: BackgroundView {
    override var mouseDownCanMoveWindow: Bool { true }
}

@MainActor
final class SeparatorView: NSView {
    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        layer?.backgroundColor = ZmuxPalette.separator.cgColor
    }

    required init?(coder: NSCoder) { nil }
}

@MainActor
final class IconButton: NSButton {
    private var hoverTrackingArea: NSTrackingArea?

    init(symbol: String, label: String, target: AnyObject?, action: Selector?) {
        super.init(frame: .zero)
        self.target = target
        self.action = action
        title = ""
        image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)?.withSymbolConfiguration(
            NSImage.SymbolConfiguration(pointSize: 12.5, weight: .medium)
        )
        imagePosition = .imageOnly
        imageScaling = .scaleProportionallyDown
        isBordered = false
        bezelStyle = .inline
        contentTintColor = .secondaryLabelColor
        toolTip = label
        setAccessibilityLabel(label)
        focusRingType = .exterior
        wantsLayer = true
        layer?.cornerRadius = 6
    }

    required init?(coder: NSCoder) { nil }

    override func updateTrackingAreas() {
        if let hoverTrackingArea { removeTrackingArea(hoverTrackingArea) }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.activeInKeyWindow, .inVisibleRect, .mouseEnteredAndExited],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        hoverTrackingArea = area
        super.updateTrackingAreas()
    }

    override func mouseEntered(with event: NSEvent) {
        layer?.backgroundColor = NSColor.white.withAlphaComponent(0.09).cgColor
        contentTintColor = .labelColor
    }

    override func mouseExited(with event: NSEvent) {
        layer?.backgroundColor = NSColor.clear.cgColor
        contentTintColor = .secondaryLabelColor
    }
}

@MainActor
final class WorkspaceRowView: NSControl {
    let workspaceID: UInt64
    var onMetadata: (@MainActor (NSView) -> Void)?
    private let terminalIcon = NSImageView()
    private let titleLabel = NSTextField(labelWithString: "")
    private let subtitleLabel = NSTextField(labelWithString: "")
    private let statusDot = NSView()
    private let metadataIcon = NSImageView()
    private let metadataLabel = NSTextField(labelWithString: "")
    private let progress = NSProgressIndicator()
    private let progressLabel = NSTextField(labelWithString: "")
    private lazy var metadataButton = IconButton(symbol: "ellipsis.circle", label: "Show workspace activity", target: self, action: #selector(showMetadata))
    private var rowHeight: NSLayoutConstraint?
    private var progressWidth: NSLayoutConstraint?
    private var progressSpacing: NSLayoutConstraint?
    private var metadata: WorkspaceMetadata?

    init(workspaceID: UInt64) {
        self.workspaceID = workspaceID
        super.init(frame: .zero)
        wantsLayer = true
        layer?.cornerRadius = 7
        layer?.borderWidth = 1
        focusRingType = .exterior
        setAccessibilityElement(true)
        setAccessibilityRole(.button)

        terminalIcon.image = NSImage(systemSymbolName: "terminal", accessibilityDescription: nil)
        terminalIcon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 13, weight: .medium)
        terminalIcon.contentTintColor = .secondaryLabelColor
        terminalIcon.translatesAutoresizingMaskIntoConstraints = false

        titleLabel.font = .systemFont(ofSize: 12.5, weight: .medium)
        titleLabel.textColor = .labelColor
        titleLabel.lineBreakMode = .byTruncatingTail
        subtitleLabel.font = .systemFont(ofSize: 10.5, weight: .regular)
        subtitleLabel.textColor = ZmuxPalette.subdued
        subtitleLabel.lineBreakMode = .byTruncatingMiddle

        statusDot.wantsLayer = true
        statusDot.layer?.cornerRadius = 3
        statusDot.translatesAutoresizingMaskIntoConstraints = false

        let labels = NSStackView(views: [titleLabel, subtitleLabel])
        labels.orientation = .vertical
        labels.alignment = .leading
        labels.spacing = 1
        labels.setContentHuggingPriority(.defaultLow, for: .horizontal)
        labels.translatesAutoresizingMaskIntoConstraints = false

        metadataIcon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 10, weight: .medium)
        metadataLabel.font = .systemFont(ofSize: 10.5, weight: .medium)
        metadataLabel.lineBreakMode = .byTruncatingTail
        metadataLabel.maximumNumberOfLines = 1
        progressLabel.font = .systemFont(ofSize: 10, weight: .regular)
        progressLabel.textColor = ZmuxPalette.subdued
        progressLabel.lineBreakMode = .byTruncatingTail
        progressLabel.maximumNumberOfLines = 1
        progress.style = .bar; progress.isIndeterminate = false
        progress.minValue = 0; progress.maxValue = 1; progress.controlSize = .small
        for view in [metadataIcon, metadataLabel, progress, progressLabel, metadataButton] {
            view.translatesAutoresizingMaskIntoConstraints = false
            view.isHidden = true
            addSubview(view)
        }

        for view in [terminalIcon, labels, statusDot] {
            addSubview(view)
        }
        rowHeight = heightAnchor.constraint(equalToConstant: 50)
        progressWidth = progress.widthAnchor.constraint(equalToConstant: 44)
        progressSpacing = progressLabel.leadingAnchor.constraint(equalTo: progress.trailingAnchor, constant: 6)
        NSLayoutConstraint.activate([
            rowHeight!, progressWidth!, progressSpacing!,
            terminalIcon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            terminalIcon.topAnchor.constraint(equalTo: topAnchor, constant: 16),
            terminalIcon.widthAnchor.constraint(equalToConstant: 17),
            terminalIcon.heightAnchor.constraint(equalToConstant: 17),
            labels.leadingAnchor.constraint(equalTo: terminalIcon.trailingAnchor, constant: 9),
            labels.topAnchor.constraint(equalTo: topAnchor, constant: 10),
            labels.trailingAnchor.constraint(lessThanOrEqualTo: statusDot.leadingAnchor, constant: -8),
            statusDot.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -11),
            statusDot.topAnchor.constraint(equalTo: topAnchor, constant: 22),
            statusDot.widthAnchor.constraint(equalToConstant: 6),
            statusDot.heightAnchor.constraint(equalToConstant: 6),
            metadataIcon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            metadataIcon.topAnchor.constraint(equalTo: topAnchor, constant: 45),
            metadataIcon.widthAnchor.constraint(equalToConstant: 12), metadataIcon.heightAnchor.constraint(equalToConstant: 12),
            metadataLabel.leadingAnchor.constraint(equalTo: metadataIcon.trailingAnchor, constant: 5),
            metadataLabel.centerYAnchor.constraint(equalTo: metadataIcon.centerYAnchor),
            metadataLabel.trailingAnchor.constraint(equalTo: metadataButton.leadingAnchor, constant: -4),
            metadataButton.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -7),
            metadataButton.centerYAnchor.constraint(equalTo: metadataIcon.centerYAnchor),
            metadataButton.widthAnchor.constraint(equalToConstant: 20), metadataButton.heightAnchor.constraint(equalToConstant: 20),
            progress.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            progress.topAnchor.constraint(equalTo: topAnchor, constant: 68), progress.heightAnchor.constraint(equalToConstant: 8),
            progressLabel.centerYAnchor.constraint(equalTo: progress.centerYAnchor),
            progressLabel.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -10)
        ])
    }

    required init?(coder: NSCoder) { nil }

    override var acceptsFirstResponder: Bool { true }

    func update(title: String, subtitle: String, isSelected: Bool, isRunning: Bool, unread: Int = 0, metadata: WorkspaceMetadata? = nil) {
        titleLabel.stringValue = title
        subtitleLabel.stringValue = unread > 0 ? "\(unread) unread · \(subtitle)" : subtitle
        statusDot.layer?.backgroundColor = (unread > 0 ? NSColor.systemOrange : (isRunning ? ZmuxPalette.success : ZmuxPalette.inactive)).cgColor
        layer?.backgroundColor = isSelected ? ZmuxPalette.selection.cgColor : NSColor.clear.cgColor
        layer?.borderColor = isSelected ? ZmuxPalette.selectionBorder.cgColor : NSColor.clear.cgColor
        terminalIcon.contentTintColor = isSelected ? .white : .secondaryLabelColor
        titleLabel.textColor = isSelected ? .white : .labelColor
        subtitleLabel.textColor = isSelected ? NSColor.white.withAlphaComponent(0.74) : ZmuxPalette.subdued
        updateMetadata(metadata)
        metadataButton.setAccessibilityLabel("Show workspace activity for \(title)")
        let activity = self.metadata.map { value in
            "\(value.statuses.count) statuses, \(value.logs.count) log entries, \(value.todos.count) todo items" + (value.progress.map { ", \(Int($0.value * 100)) percent" } ?? "") + (value.pullRequest.map { ", PR #\($0.number)" } ?? "")
        } ?? ""
        setAccessibilityLabel("\(title), \(subtitle)" + (activity.isEmpty ? "" : ", " + activity))
        setAccessibilityValue(self.metadata == nil ? "" : metadataLabel.stringValue + ", " + progressLabel.stringValue)
        setAccessibilitySelected(isSelected)
        needsDisplay = true
    }

    private func updateMetadata(_ value: WorkspaceMetadata?) {
        let value = value.flatMap { $0.isEmpty ? nil : $0 }
        guard value != metadata else { return }
        metadata = value
        setAccessibilityChildren(value == nil ? [] : [metadataButton])
        rowHeight?.constant = value == nil ? 50 : 86
        for view in [metadataIcon, metadataLabel, progressLabel, metadataButton] { view.isHidden = value == nil }
        guard let value else { progress.isHidden = true; return }
        let first = value.statuses.first
        let symbol = first?.icon ?? (value.pullRequest == nil ? (value.progress == nil ? (value.todos.isEmpty ? "text.bubble" : "checklist") : "circle.dotted") : "arrow.triangle.pull")
        metadataIcon.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
            ?? NSImage(systemSymbolName: "circle.fill", accessibilityDescription: nil)
        let color = first?.color.flatMap { hexColor($0) } ?? NSColor.secondaryLabelColor
        metadataIcon.contentTintColor = color
        metadataLabel.textColor = color
        metadataLabel.stringValue = first.map { "\($0.key): \($0.value)" } ?? (value.pullRequest.map { "PR #\($0.number)" } ?? (value.progress == nil ? (value.todos.isEmpty ? "Workspace log" : "\(value.todos.count) todo items") : "Task progress"))
        metadataLabel.toolTip = metadataLabel.stringValue
        metadataButton.toolTip = "Show all workspace activity" + (value.pullRequest.map { " · " + $0.url } ?? "")
        progress.isHidden = value.progress == nil
        progressWidth?.constant = value.progress == nil ? 0 : 44
        progressSpacing?.constant = value.progress == nil ? 0 : 6
        if let task = value.progress {
            progress.doubleValue = min(1, max(0, task.value))
            let percent = Int(progress.doubleValue * 100)
            progressLabel.stringValue = "\(percent)%" + (task.label.map { " · " + $0 } ?? "")
            progress.setAccessibilityLabel(task.label ?? "Workspace progress")
        } else { progressLabel.stringValue = "\(value.logs.count) log entries" + (value.logsDropped > 0 ? " · \(value.logsDropped) older omitted" : "") }
        if !value.todos.isEmpty { progressLabel.stringValue += " · \(value.todos.filter { $0.state == .completed }.count)/\(value.todos.count) todos completed" }
        if let pullRequest = value.pullRequest {
            if first != nil || value.progress != nil { progressLabel.stringValue += " · PR #\(pullRequest.number)" }
            progressLabel.toolTip = pullRequest.url
        } else { progressLabel.toolTip = progressLabel.stringValue }
    }
    private func hexColor(_ text: String) -> NSColor? {
        guard text.count == 7, text.first == "#", let value = UInt32(text.dropFirst(), radix: 16) else { return nil }
        return NSColor(calibratedRed: CGFloat((value >> 16) & 255) / 255,
                       green: CGFloat((value >> 8) & 255) / 255, blue: CGFloat(value & 255) / 255, alpha: 1)
    }
    // Anchor details to the row, which remains present when metadata is cleared
    // and the button itself hides during an open inspection.
    @objc private func showMetadata() { onMetadata?(self) }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        guard event.type == .leftMouseDown, let action else { return }
        sendAction(action, to: target)
    }

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 36 || event.keyCode == 76 || event.charactersIgnoringModifiers == " " {
            if let action { sendAction(action, to: target) }
            return
        }
        super.keyDown(with: event)
    }

    override func becomeFirstResponder() -> Bool {
        needsDisplay = true
        return true
    }

    override func resignFirstResponder() -> Bool {
        needsDisplay = true
        return true
    }

    override func drawFocusRingMask() {
        NSBezierPath(roundedRect: bounds.insetBy(dx: 1, dy: 1), xRadius: 7, yRadius: 7).fill()
    }

    override var focusRingMaskBounds: NSRect { bounds }
}

/// A single bounded, plain-text document keeps all retained metadata accessible
/// without growing sidebar rows or interpreting caller text as markup.
@MainActor
final class WorkspaceMetadataViewController: NSViewController {
    let workspaceID: UInt64
    let identityNamespace: UUID
    private let heading = NSTextField(labelWithString: "Workspace activity")
    private let workspaceLabel = NSTextField(labelWithString: "")
    private let scroll = NSScrollView()
    private let text = NSTextView(frame: NSRect(x: 0, y: 0, width: 420, height: 380))
    private var displayed: WorkspaceMetadata?

    init(workspaceID: UInt64, identityNamespace: UUID) {
        self.workspaceID = workspaceID; self.identityNamespace = identityNamespace
        super.init(nibName: nil, bundle: nil)
    }
    required init?(coder: NSCoder) { nil }
    override func loadView() {
        let root = BackgroundView(color: ZmuxPalette.chrome)
        root.frame = NSRect(x: 0, y: 0, width: 420, height: 460)
        heading.font = .systemFont(ofSize: 14, weight: .semibold)
        workspaceLabel.font = .systemFont(ofSize: 11)
        workspaceLabel.textColor = .secondaryLabelColor
        workspaceLabel.lineBreakMode = .byTruncatingTail
        scroll.hasVerticalScroller = true; scroll.hasHorizontalScroller = false
        scroll.drawsBackground = false
        text.isEditable = false; text.isSelectable = true; text.isRichText = false
        text.importsGraphics = false; text.isAutomaticLinkDetectionEnabled = false
        text.drawsBackground = false; text.textColor = .labelColor
        text.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        text.isVerticallyResizable = true; text.isHorizontallyResizable = false
        text.autoresizingMask = [.width]
        text.minSize = .zero; text.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        text.textContainer?.containerSize = NSSize(width: 420, height: CGFloat.greatestFiniteMagnitude)
        text.textContainer?.widthTracksTextView = true
        text.textContainerInset = NSSize(width: 14, height: 12)
        scroll.documentView = text
        for child in [heading, workspaceLabel, scroll] {
            child.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(child)
        }
        NSLayoutConstraint.activate([
            heading.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 14),
            heading.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -14),
            heading.topAnchor.constraint(equalTo: root.topAnchor, constant: 14),
            workspaceLabel.leadingAnchor.constraint(equalTo: heading.leadingAnchor),
            workspaceLabel.trailingAnchor.constraint(equalTo: heading.trailingAnchor),
            workspaceLabel.topAnchor.constraint(equalTo: heading.bottomAnchor, constant: 4),
            scroll.leadingAnchor.constraint(equalTo: root.leadingAnchor), scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: workspaceLabel.bottomAnchor, constant: 10),
            scroll.bottomAnchor.constraint(equalTo: root.bottomAnchor)
        ])
        view = root
    }
    func update(_ metadata: WorkspaceMetadata, name: String) {
        _ = view
        workspaceLabel.stringValue = name
        text.setAccessibilityLabel("Workspace activity details for \(name)")
        guard displayed != metadata else { return }
        displayed = metadata
        var lines = ["PULL REQUEST"]
        if let pullRequest = metadata.pullRequest { lines += ["PR #\(pullRequest.number)", pullRequest.url] }
        else { lines.append("No manual pull request") }
        lines += ["", "STATUSES · \(metadata.statuses.count)"]
        if metadata.statuses.isEmpty { lines.append("No statuses") }
        for status in metadata.statuses {
            lines += ["\(status.key) · priority \(status.priority)", status.value]
            let appearance = [status.icon.map { "Icon: " + $0 }, status.color.map { "Color: " + $0 }].compactMap { $0 }
            if !appearance.isEmpty { lines.append(appearance.joined(separator: " · ")) }
            lines.append("")
        }
        lines += ["", "PROGRESS"]
        if let task = metadata.progress {
            lines.append("\(Int(task.value * 100))%" + (task.label.map { " · " + $0 } ?? ""))
        } else { lines.append("No progress") }
        lines += ["", "TODOS · \(metadata.todos.count)"]
        if metadata.todos.isEmpty { lines.append("No todo items") }
        for (index, item) in metadata.todos.enumerated() {
            lines += ["\(index + 1). \(item.state.rawValue) · \(item.origin.rawValue) · \(item.id.uuidString.lowercased())", item.text, ""]
        }
        lines += ["", "LOGS · \(metadata.logs.count) retained · \(metadata.logsDropped) older entries omitted"]
        if metadata.logs.isEmpty { lines.append("No log entries") }
        for log in metadata.logs {
            lines += ["#\(log.sequence) · \(log.level.rawValue.uppercased()) · \(log.source ?? "Unspecified source")", log.message, ""]
        }
        let selection = text.selectedRange()
        let origin = scroll.contentView.bounds.origin
        let replacement = lines.joined(separator: "\n")
        let previous = Array(text.string.utf16)
        let next = Array(replacement.utf16)
        var prefix = 0
        while prefix < min(previous.count, next.count), previous[prefix] == next[prefix] { prefix += 1 }
        var suffix = 0
        while suffix < min(previous.count, next.count) - prefix,
              previous[previous.count - suffix - 1] == next[next.count - suffix - 1] { suffix += 1 }
        let oldLocation = min(selection.location, previous.count)
        let oldLength = min(selection.length, previous.count - oldLocation)
        let location: Int
        if oldLocation + oldLength <= prefix { location = oldLocation }
        else if oldLocation >= previous.count - suffix { location = oldLocation + next.count - previous.count }
        else { location = min(oldLocation, next.count) }
        text.string = replacement
        text.setSelectedRange(NSRange(location: location, length: min(oldLength, next.count - location)))
        scroll.contentView.scroll(to: origin)
        scroll.reflectScrolledClipView(scroll.contentView)
    }
}

@MainActor
final class FlippedStackView: NSStackView {
    override var isFlipped: Bool { true }
}

@MainActor
final class MainHeaderView: DraggableChromeView {
    let titleLabel = NSTextField(labelWithString: "Terminal")
    let detailLabel = NSTextField(labelWithString: "Starting shell…")
    let actions = NSStackView()

    init() {
        super.init(color: ZmuxPalette.chrome)

        let icon = NSImageView(image: NSImage(systemSymbolName: "terminal.fill", accessibilityDescription: nil) ?? NSImage())
        icon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 12, weight: .medium)
        icon.contentTintColor = ZmuxPalette.subdued
        icon.translatesAutoresizingMaskIntoConstraints = false

        titleLabel.font = .systemFont(ofSize: 12.5, weight: .semibold)
        titleLabel.textColor = .labelColor
        titleLabel.lineBreakMode = .byTruncatingTail
        detailLabel.font = .systemFont(ofSize: 10.5)
        detailLabel.textColor = ZmuxPalette.subdued
        detailLabel.lineBreakMode = .byTruncatingMiddle

        let labels = NSStackView(views: [titleLabel, detailLabel])
        labels.orientation = .vertical
        labels.alignment = .leading
        labels.spacing = 0
        labels.translatesAutoresizingMaskIntoConstraints = false
        labels.setContentHuggingPriority(.defaultLow, for: .horizontal)

        actions.orientation = .horizontal
        actions.alignment = .centerY
        actions.spacing = 3
        actions.translatesAutoresizingMaskIntoConstraints = false

        let separator = SeparatorView()
        separator.translatesAutoresizingMaskIntoConstraints = false
        for view in [icon, labels, actions, separator] { addSubview(view) }
        NSLayoutConstraint.activate([
            icon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 13),
            icon.centerYAnchor.constraint(equalTo: centerYAnchor),
            icon.widthAnchor.constraint(equalToConstant: 16),
            icon.heightAnchor.constraint(equalToConstant: 16),
            labels.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: 8),
            labels.centerYAnchor.constraint(equalTo: centerYAnchor),
            labels.trailingAnchor.constraint(lessThanOrEqualTo: actions.leadingAnchor, constant: -12),
            actions.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            actions.centerYAnchor.constraint(equalTo: centerYAnchor),
            separator.leadingAnchor.constraint(equalTo: leadingAnchor),
            separator.trailingAnchor.constraint(equalTo: trailingAnchor),
            separator.bottomAnchor.constraint(equalTo: bottomAnchor),
            separator.heightAnchor.constraint(equalToConstant: 1)
        ])
    }

    required init?(coder: NSCoder) { nil }

    func update(title: String, detail: String, error: Bool) {
        titleLabel.stringValue = title
        detailLabel.stringValue = detail
        detailLabel.textColor = error ? .systemRed : ZmuxPalette.subdued
        setAccessibilityLabel("Terminal: \(title)")
    }
}

@MainActor
final class CloudFooterView: BackgroundView {
    let settingsButton: IconButton
    private let cloudIcon = NSImageView()
    private let titleLabel = NSTextField(labelWithString: "Cloud")
    private let statusLabel = NSTextField(labelWithString: "Not connected")

    init(target: AnyObject?, action: Selector) {
        settingsButton = IconButton(symbol: "gearshape", label: "Cloud settings", target: target, action: action)
        super.init(color: ZmuxPalette.sidebar)

        cloudIcon.image = NSImage(systemSymbolName: "icloud", accessibilityDescription: nil)
        cloudIcon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 13, weight: .medium)
        cloudIcon.contentTintColor = ZmuxPalette.subdued
        cloudIcon.translatesAutoresizingMaskIntoConstraints = false
        titleLabel.font = .systemFont(ofSize: 11.5, weight: .medium)
        statusLabel.font = .systemFont(ofSize: 10)
        statusLabel.textColor = ZmuxPalette.subdued
        statusLabel.lineBreakMode = .byTruncatingTail

        let labels = NSStackView(views: [titleLabel, statusLabel])
        labels.orientation = .vertical
        labels.alignment = .leading
        labels.spacing = 0
        labels.translatesAutoresizingMaskIntoConstraints = false
        settingsButton.translatesAutoresizingMaskIntoConstraints = false

        let separator = SeparatorView()
        separator.translatesAutoresizingMaskIntoConstraints = false
        for view in [cloudIcon, labels, settingsButton, separator] { addSubview(view) }
        NSLayoutConstraint.activate([
            separator.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            separator.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -10),
            separator.topAnchor.constraint(equalTo: topAnchor),
            separator.heightAnchor.constraint(equalToConstant: 1),
            cloudIcon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 13),
            cloudIcon.centerYAnchor.constraint(equalTo: centerYAnchor),
            cloudIcon.widthAnchor.constraint(equalToConstant: 17),
            cloudIcon.heightAnchor.constraint(equalToConstant: 17),
            labels.leadingAnchor.constraint(equalTo: cloudIcon.trailingAnchor, constant: 8),
            labels.centerYAnchor.constraint(equalTo: centerYAnchor),
            labels.trailingAnchor.constraint(lessThanOrEqualTo: settingsButton.leadingAnchor, constant: -6),
            settingsButton.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -9),
            settingsButton.centerYAnchor.constraint(equalTo: centerYAnchor),
            settingsButton.widthAnchor.constraint(equalToConstant: 28),
            settingsButton.heightAnchor.constraint(equalToConstant: 28)
        ])
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel("Cloud account")
    }

    required init?(coder: NSCoder) { nil }

    func update(label: String, connected: Bool) {
        statusLabel.stringValue = label
        cloudIcon.contentTintColor = connected ? ZmuxPalette.success : ZmuxPalette.subdued
        cloudIcon.image = NSImage(
            systemSymbolName: connected ? "checkmark.icloud.fill" : "icloud",
            accessibilityDescription: nil
        )
        setAccessibilityValue(label)
    }
}


/// The bar only submits shared actions; terminal sessions remain owned by Rust.
@MainActor
final class TerminalTabBar: BackgroundView {
    private let pane: Pane
    private let namespace: UUID
    private weak var owner: MainWindowHost?
    private let scroll = NSScrollView()
    private let strip = NSView()
    private var tabs: [TerminalTabControl] = []
    private static let dragType = NSPasteboard.PasteboardType("app.zmux.terminal-surface")

    init(pane: Pane, namespace: UUID, owner: MainWindowHost) {
        self.pane = pane
        self.namespace = namespace
        self.owner = owner
        super.init(color: ZmuxPalette.chrome)
        scroll.drawsBackground = false
        scroll.borderType = .noBorder
        scroll.hasHorizontalScroller = true
        scroll.autohidesScrollers = true
        scroll.horizontalScroller?.controlSize = .mini
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.documentView = strip
        strip.frame = NSRect(x: 0, y: 0, width: pane.surfaces.count * 160, height: 32)
        for (index, surface) in pane.surfaces.enumerated() {
            let tab = TerminalTabControl(surface: surface, paneID: pane.id, namespace: namespace, selected: surface.id == pane.selectedSurface, owner: owner)
            tab.frame = NSRect(x: index * 160, y: 0, width: 160, height: 32)
            strip.addSubview(tab)
            tabs.append(tab)
        }
        let plus = IconButton(symbol: "plus", label: "New terminal tab", target: self, action: #selector(addTab))
        plus.translatesAutoresizingMaskIntoConstraints = false
        addSubview(scroll); addSubview(plus)
        NSLayoutConstraint.activate([
            scroll.leadingAnchor.constraint(equalTo: leadingAnchor),
            scroll.topAnchor.constraint(equalTo: topAnchor),
            scroll.bottomAnchor.constraint(equalTo: bottomAnchor),
            scroll.trailingAnchor.constraint(equalTo: plus.leadingAnchor, constant: -2),
            plus.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -4),
            plus.centerYAnchor.constraint(equalTo: centerYAnchor),
            plus.widthAnchor.constraint(equalToConstant: 24), plus.heightAnchor.constraint(equalToConstant: 24)
        ])
        registerForDraggedTypes([Self.dragType])
        setAccessibilityLabel("Terminal tabs")
    }
    required init?(coder: NSCoder) { nil }
    override func layout() {
        super.layout()
        if let selected = tabs.first(where: { $0.surface.id == pane.selectedSurface }) {
            selected.scrollToVisible(selected.bounds)
        }
    }
    func updateAttention(_ notifications: [AttentionNotification], surfaces: [Surface]) {
        for tab in tabs {
            if let surface = surfaces.first(where: { $0.id == tab.surface.id }) { tab.surface = surface }
            tab.updateAttention(notifications.filter { !$0.read && $0.surfaceId == tab.surface.id }.count)
        }
    }
    @objc private func addTab() { owner?.newTerminalInPane(pane.id) }
    override func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
        guard let tab = sender.draggingSource as? TerminalTabControl, tab.owner === owner, tab.namespace == namespace else { return [] }
        return .move
    }
    override func prepareForDragOperation(_ sender: any NSDraggingInfo) -> Bool { draggingEntered(sender) == .move }
    override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
        guard let tab = sender.draggingSource as? TerminalTabControl, tab.owner === owner, tab.namespace == namespace,
              let value = sender.draggingPasteboard.string(forType: Self.dragType), let id = UInt64(value), id == tab.surface.id, let revision = tab.dragRevision,
              owner?.surfaceDragRevision(id, in: tab.paneID, namespace: tab.namespace) == revision else { return false }
        let x = strip.convert(sender.draggingLocation, from: nil).x
        var index = min(pane.surfaces.count, max(0, Int((x + 80) / 160)))
        if let oldIndex = pane.surfaces.firstIndex(where: { $0.id == id }), oldIndex < index { index -= 1 }
        owner?.reorderTab(id, from: tab.paneID, in: pane.id, namespace: tab.namespace, revision: revision, to: index)
        return true
    }
}

@MainActor
final class TerminalTabControl: BackgroundView, NSDraggingSource {
    var surface: Surface
    let paneID: UInt64
    let namespace: UUID
    private(set) var dragRevision: UInt64?
    weak var owner: MainWindowHost?
    private var mouseOrigin: NSPoint?
    private let titleLabel = NSTextField(labelWithString: "")

    init(surface: Surface, paneID: UInt64, namespace: UUID, selected: Bool, owner: MainWindowHost) {
        self.surface = surface
        self.paneID = paneID
        self.namespace = namespace
        self.owner = owner
        super.init(color: selected ? ZmuxPalette.terminal : ZmuxPalette.chrome)
        let icon = NSImageView(image: NSImage(systemSymbolName: surface.kind == "browser" ? "globe" : "terminal", accessibilityDescription: nil) ?? NSImage())
        icon.contentTintColor = selected ? .labelColor : ZmuxPalette.subdued
        icon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 11, weight: .regular)
        let title = titleLabel
        title.stringValue = surface.title
        title.font = .systemFont(ofSize: 11.5, weight: selected ? .medium : .regular)
        title.textColor = selected ? .labelColor : ZmuxPalette.subdued
        title.lineBreakMode = .byTruncatingTail
        let close = IconButton(symbol: "xmark", label: "Close \(surface.title)", target: self, action: #selector(closeTab))
        for view in [icon, title, close] { view.translatesAutoresizingMaskIntoConstraints = false; addSubview(view) }
        NSLayoutConstraint.activate([
            icon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10), icon.centerYAnchor.constraint(equalTo: centerYAnchor),
            icon.widthAnchor.constraint(equalToConstant: 13), icon.heightAnchor.constraint(equalToConstant: 13),
            title.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: 7), title.centerYAnchor.constraint(equalTo: centerYAnchor),
            title.trailingAnchor.constraint(equalTo: close.leadingAnchor, constant: -3),
            close.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -4), close.centerYAnchor.constraint(equalTo: centerYAnchor),
            close.widthAnchor.constraint(equalToConstant: 22), close.heightAnchor.constraint(equalToConstant: 22)
        ])
        toolTip = surface.cwd.map { "\(surface.title)\n\($0)" } ?? surface.title
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityLabel(surface.title)
        setAccessibilitySelected(selected)
        let menu = NSMenu()
        for (label, action) in [("Rename Tab…", #selector(renameTab)), ("Move Left", #selector(moveTabLeft)), ("Move Right", #selector(moveTabRight)), ("Close Tab", #selector(closeTab)), ("Close Other Tabs…", #selector(closeOthers))] {
            let item = menu.addItem(withTitle: label, action: action, keyEquivalent: ""); item.target = self
        }
        self.menu = menu
    }
    required init?(coder: NSCoder) { nil }
    override var acceptsFirstResponder: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? {
        guard let hit = super.hitTest(point) else { return nil }
        return hit is NSButton ? hit : self
    }
    override func mouseDown(with event: NSEvent) {
        mouseOrigin = event.locationInWindow
        owner?.selectTab(surface.id)
        if event.clickCount == 2 { renameTab() }
    }
    override func keyDown(with event: NSEvent) {
        if event.keyCode == 36 || event.charactersIgnoringModifiers == " " { owner?.selectTab(surface.id) }
        else { super.keyDown(with: event) }
    }
    override func accessibilityPerformPress() -> Bool { owner?.selectTab(surface.id); return owner != nil }
    override func mouseDragged(with event: NSEvent) {
        guard let origin = mouseOrigin, hypot(event.locationInWindow.x - origin.x, event.locationInWindow.y - origin.y) > 5 else { return }
        mouseOrigin = nil
        guard let revision = owner?.surfaceDragRevision(surface.id, in: paneID, namespace: namespace) else { return }
        dragRevision = revision
        let item = NSPasteboardItem()
        item.setString(String(surface.id), forType: NSPasteboard.PasteboardType("app.zmux.terminal-surface"))
        let dragging = NSDraggingItem(pasteboardWriter: item)
        let image = NSImage(size: bounds.size)
        image.lockFocus(); ZmuxPalette.chrome.setFill(); bounds.fill(); image.unlockFocus()
        dragging.setDraggingFrame(bounds, contents: image)
        beginDraggingSession(with: [dragging], event: event, source: self)
    }
    func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation {
        context == .withinApplication ? .move : []
    }
    func updateAttention(_ unread: Int) {
        titleLabel.stringValue = (unread > 0 ? "● " : "") + surface.title
        titleLabel.textColor = unread > 0 ? .systemOrange : .labelColor
        setAccessibilityLabel(surface.title + (unread > 0 ? ", \(unread) unread" : ""))
    }
    @objc private func moveTabLeft() { owner?.moveTab(surface.id, in: paneID, namespace: namespace, offset: -1) }
    @objc private func moveTabRight() { owner?.moveTab(surface.id, in: paneID, namespace: namespace, offset: 1) }
    @objc private func closeTab() { owner?.closeTerminalTab(surface.id) }
    @objc private func renameTab() { owner?.renameTerminalTab(surface.id) }
    @objc private func closeOthers() { owner?.closeOtherTabs(surface.id) }
}

@MainActor
final class NotificationRowView: BackgroundView {
    private let item: AttentionNotification
    private weak var owner: MainWindowHost?
    init(item: AttentionNotification, workspaceName: String?, available: Bool, owner: MainWindowHost) {
        self.item = item; self.owner = owner
        super.init(color: NSColor.white.withAlphaComponent(item.read ? 0.025 : 0.065))
        layer?.cornerRadius = 7
        let title = NSTextField(wrappingLabelWithString: (item.read ? "" : "● ") + item.title)
        title.font = .systemFont(ofSize: 12, weight: item.read ? .regular : .semibold)
        title.textColor = item.read ? .secondaryLabelColor : .labelColor
        let body = NSTextField(wrappingLabelWithString: item.body)
        body.font = .systemFont(ofSize: 11); body.textColor = ZmuxPalette.subdued
        let date = Date(timeIntervalSince1970: Double(item.timestampMs) / 1000)
        let destinationLabel: String
        if case .workspace? = item.target?.destination { destinationLabel = "Workspace · \(workspaceName ?? "Unavailable") · " }
        else { destinationLabel = "" }
        let detail = NSTextField(labelWithString: destinationLabel + "\(item.source) · \(date.formatted(date: .omitted, time: .shortened))" + (available ? "" : " · \(item.target?.destination.unavailableLabel ?? "Target unavailable")"))
        detail.font = .systemFont(ofSize: 10); detail.textColor = ZmuxPalette.subdued
        let labels = NSStackView(views: [title])
        if !item.subtitle.isEmpty {
            let subtitle = NSTextField(wrappingLabelWithString: item.subtitle)
            subtitle.font = .systemFont(ofSize: 11, weight: .medium)
            subtitle.textColor = .secondaryLabelColor
            labels.addArrangedSubview(subtitle)
        }
        if !item.body.isEmpty { labels.addArrangedSubview(body) }
        labels.addArrangedSubview(detail)
        labels.orientation = .vertical; labels.alignment = .leading; labels.spacing = 4
        let open = IconButton(symbol: "arrow.up.forward", label: item.target?.destination.openLabel ?? "Open notification target", target: self, action: #selector(openTarget))
        open.isEnabled = available
        let read = IconButton(symbol: "checkmark", label: "Mark read", target: self, action: #selector(markRead))
        read.isEnabled = !item.read
        let dismiss = IconButton(symbol: "xmark", label: "Dismiss notification", target: self, action: #selector(dismissItem))
        let actions = NSStackView(views: [open, read, dismiss])
        actions.spacing = 2
        for button in [open, read, dismiss] {
            button.widthAnchor.constraint(equalToConstant: 24).isActive = true
            button.heightAnchor.constraint(equalToConstant: 24).isActive = true
        }
        for view in [labels, actions] { view.translatesAutoresizingMaskIntoConstraints = false; addSubview(view) }
        NSLayoutConstraint.activate([
            labels.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10), labels.topAnchor.constraint(equalTo: topAnchor, constant: 10),
            labels.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -10), labels.trailingAnchor.constraint(equalTo: actions.leadingAnchor, constant: -6),
            actions.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -6), actions.topAnchor.constraint(equalTo: topAnchor, constant: 6)
        ])
    }
    required init?(coder: NSCoder) { nil }
    @objc private func openTarget() { owner?.openNotification(item.id) }
    @objc private func markRead() { owner?.readNotification(item.id) }
    @objc private func dismissItem() { owner?.dismissNotification(item.id) }
}

/// Owns presentation geometry explicitly. Child content constraints cannot select pane widths.
@MainActor
final class PaneSplitView: BackgroundView {
    let geometryKey: String
    let vertical: Bool
    private(set) var fraction: CGFloat
    var onFractionChanged: ((CGFloat) -> Void)?
    private let first: NSView
    private let second: NSView
    private let divider = PaneDividerView()
    override var isFlipped: Bool { true }

    init(key: String, vertical: Bool, fraction: CGFloat, first: NSView, second: NSView) {
        geometryKey = key; self.vertical = vertical; self.fraction = fraction
        self.first = first; self.second = second
        super.init(color: ZmuxPalette.terminal)
        divider.owner = self
        for child in [first, second, divider] {
            child.translatesAutoresizingMaskIntoConstraints = true
            child.autoresizingMask = []
            addSubview(child)
        }
        setAccessibilityRole(.group)
        setAccessibilityLabel(vertical ? "Horizontal terminal split" : "Vertical terminal split")
    }
    required init?(coder: NSCoder) { nil }
    override func layout() { super.layout(); layoutPanes() }
    override func resizeSubviews(withOldSize oldSize: NSSize) { layoutPanes() }
    func layoutPanes() {
        let extent = max(0, (vertical ? bounds.width : bounds.height) - 1)
        let leading = floor(extent * fraction)
        if vertical {
            first.frame = NSRect(x: 0, y: 0, width: leading, height: bounds.height)
            divider.frame = NSRect(x: leading - 3, y: 0, width: 7, height: bounds.height)
            second.frame = NSRect(x: leading + 1, y: 0, width: extent - leading, height: bounds.height)
        } else {
            first.frame = NSRect(x: 0, y: 0, width: bounds.width, height: leading)
            divider.frame = NSRect(x: 0, y: leading - 3, width: bounds.width, height: 7)
            second.frame = NSRect(x: 0, y: leading + 1, width: bounds.width, height: extent - leading)
        }
        divider.needsDisplay = true
        divider.setAccessibilityValue(NSNumber(value: Double(fraction * 100)))
        first.needsLayout = true; second.needsLayout = true
    }
    func resize(to point: NSPoint) {
        let extent = (vertical ? bounds.width : bounds.height) - 1
        guard extent > 0 else { return }
        setFraction((vertical ? point.x : point.y) / extent)
    }
    func setFraction(_ value: CGFloat) {
        guard value.isFinite else { return }
        fraction = min(0.8, max(0.2, value))
        onFractionChanged?(fraction)
        layoutPanes()
        needsLayout = true
        layoutSubtreeIfNeeded()
    }
}

@MainActor
private final class PaneDividerView: BackgroundView {
    weak var owner: PaneSplitView?
    init() {
        super.init(color: .clear)
        setAccessibilityElement(true)
        setAccessibilityRole(.splitter)
        setAccessibilityLabel("Resize panes")
    }
    required init?(coder: NSCoder) { nil }
    override var acceptsFirstResponder: Bool { true }
    override func draw(_ dirtyRect: NSRect) {
        ZmuxPalette.separator.setFill()
        let line = owner?.vertical == true ? NSRect(x: bounds.midX - 0.5, y: 0, width: 1, height: bounds.height)
            : NSRect(x: 0, y: bounds.midY - 0.5, width: bounds.width, height: 1)
        line.fill()
    }
    override func resetCursorRects() {
        addCursorRect(bounds, cursor: owner?.vertical == true ? .resizeLeftRight : .resizeUpDown)
    }
    override func mouseDown(with event: NSEvent) { /* Preserve the terminal's input focus while dragging. */ }
    override func mouseDragged(with event: NSEvent) {
        if let owner { owner.resize(to: owner.convert(event.locationInWindow, from: nil)) }
    }
    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 123, 126: if let owner { owner.setFraction(owner.fraction - 0.05) }
        case 124, 125: if let owner { owner.setFraction(owner.fraction + 0.05) }
        default: super.keyDown(with: event)
        }
    }
    override func accessibilityPerformIncrement() -> Bool {
        guard let owner else { return false }; owner.setFraction(owner.fraction + 0.05); return true
    }
    override func accessibilityPerformDecrement() -> Bool {
        guard let owner else { return false }; owner.setFraction(owner.fraction - 0.05); return true
    }
}

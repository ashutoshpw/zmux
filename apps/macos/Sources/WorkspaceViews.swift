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
    private let terminalIcon = NSImageView()
    private let titleLabel = NSTextField(labelWithString: "")
    private let subtitleLabel = NSTextField(labelWithString: "")
    private let statusDot = NSView()

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

        for view in [terminalIcon, labels, statusDot] {
            addSubview(view)
        }
        NSLayoutConstraint.activate([
            heightAnchor.constraint(equalToConstant: 50),
            terminalIcon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            terminalIcon.centerYAnchor.constraint(equalTo: centerYAnchor),
            terminalIcon.widthAnchor.constraint(equalToConstant: 17),
            terminalIcon.heightAnchor.constraint(equalToConstant: 17),
            labels.leadingAnchor.constraint(equalTo: terminalIcon.trailingAnchor, constant: 9),
            labels.centerYAnchor.constraint(equalTo: centerYAnchor),
            labels.trailingAnchor.constraint(lessThanOrEqualTo: statusDot.leadingAnchor, constant: -8),
            statusDot.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -11),
            statusDot.centerYAnchor.constraint(equalTo: centerYAnchor),
            statusDot.widthAnchor.constraint(equalToConstant: 6),
            statusDot.heightAnchor.constraint(equalToConstant: 6)
        ])
    }

    required init?(coder: NSCoder) { nil }

    override var acceptsFirstResponder: Bool { true }

    func update(title: String, subtitle: String, isSelected: Bool, isRunning: Bool) {
        titleLabel.stringValue = title
        subtitleLabel.stringValue = subtitle
        statusDot.layer?.backgroundColor = (isRunning ? ZmuxPalette.success : ZmuxPalette.inactive).cgColor
        layer?.backgroundColor = isSelected ? ZmuxPalette.selection.cgColor : NSColor.clear.cgColor
        layer?.borderColor = isSelected ? ZmuxPalette.selectionBorder.cgColor : NSColor.clear.cgColor
        terminalIcon.contentTintColor = isSelected ? .white : .secondaryLabelColor
        titleLabel.textColor = isSelected ? .white : .labelColor
        subtitleLabel.textColor = isSelected ? NSColor.white.withAlphaComponent(0.74) : ZmuxPalette.subdued
        setAccessibilityLabel("\(title), \(subtitle)")
        setAccessibilitySelected(isSelected)
        needsDisplay = true
    }

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


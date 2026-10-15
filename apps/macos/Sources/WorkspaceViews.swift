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

    func update(title: String, subtitle: String, isSelected: Bool, isRunning: Bool, unread: Int = 0) {
        titleLabel.stringValue = title
        subtitleLabel.stringValue = unread > 0 ? "\(unread) unread · \(subtitle)" : subtitle
        statusDot.layer?.backgroundColor = (unread > 0 ? NSColor.systemOrange : (isRunning ? ZmuxPalette.success : ZmuxPalette.inactive)).cgColor
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


/// The bar only submits shared actions; terminal sessions remain owned by Rust.
@MainActor
final class TerminalTabBar: BackgroundView {
    private let pane: Pane
    private weak var owner: AppController?
    private let scroll = NSScrollView()
    private let strip = NSView()
    private var tabs: [TerminalTabControl] = []
    private static let dragType = NSPasteboard.PasteboardType("app.zmux.terminal-surface")

    init(pane: Pane, owner: AppController) {
        self.pane = pane
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
            let tab = TerminalTabControl(surface: surface, selected: surface.id == pane.selectedSurface, owner: owner)
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
        guard let tab = sender.draggingSource as? TerminalTabControl, tab.owner === owner else { return [] }
        return .move
    }
    override func prepareForDragOperation(_ sender: any NSDraggingInfo) -> Bool { draggingEntered(sender) == .move }
    override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
        guard let tab = sender.draggingSource as? TerminalTabControl, tab.owner === owner,
              let value = sender.draggingPasteboard.string(forType: Self.dragType), let id = UInt64(value), id == tab.surface.id else { return false }
        let x = strip.convert(sender.draggingLocation, from: nil).x
        var index = min(pane.surfaces.count, max(0, Int((x + 80) / 160)))
        if let oldIndex = pane.surfaces.firstIndex(where: { $0.id == id }), oldIndex < index { index -= 1 }
        owner?.reorderTab(id, in: pane.id, to: index)
        return true
    }
}

@MainActor
final class TerminalTabControl: BackgroundView, NSDraggingSource {
    var surface: Surface
    weak var owner: AppController?
    private var mouseOrigin: NSPoint?
    private let titleLabel = NSTextField(labelWithString: "")

    init(surface: Surface, selected: Bool, owner: AppController) {
        self.surface = surface
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
        for (label, action) in [("Rename Tab…", #selector(renameTab)), ("Close Tab", #selector(closeTab)), ("Close Other Tabs…", #selector(closeOthers))] {
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
    @objc private func closeTab() { owner?.closeTerminalTab(surface.id) }
    @objc private func renameTab() { owner?.renameTerminalTab(surface.id) }
    @objc private func closeOthers() { owner?.closeOtherTabs(surface.id) }
}

@MainActor
final class NotificationRowView: BackgroundView {
    private let item: AttentionNotification
    private weak var owner: AppController?
    init(item: AttentionNotification, workspaceName: String?, available: Bool, owner: AppController) {
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
        fraction = min(0.8, max(0.2, value))
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

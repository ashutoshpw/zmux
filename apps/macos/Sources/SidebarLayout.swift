import AppKit

/// User width and available width are separate: a smaller window must not erase
/// the preference that will fit again when the window grows.
@MainActor
final class SidebarLayout {
    static let minimumWidth: CGFloat = 200
    static let maximumWidth: CGFloat = 480
    private static let minimumContentWidth: CGFloat = 400
    private let defaults: UserDefaults
    private let key: String
    private(set) var preferredWidth: CGFloat

    init(defaults: UserDefaults = .standard, key: String = "sidebarWidth", inheritLegacy: Bool = true) {
        self.defaults = defaults
        self.key = key
        let saved = (defaults.object(forKey: key) ?? (inheritLegacy ? defaults.object(forKey: "sidebarWidth") : nil)) as? NSNumber
        let width = saved.map { CGFloat($0.doubleValue) } ?? 224
        preferredWidth = width.isFinite ? min(Self.maximumWidth, max(Self.minimumWidth, width)) : 224
    }

    func visibleWidth(availableWidth: CGFloat) -> CGFloat {
        min(preferredWidth, widthLimit(availableWidth))
    }

    func resize(to width: CGFloat, availableWidth: CGFloat) {
        guard width.isFinite else { return }
        preferredWidth = min(max(Self.minimumWidth, widthLimit(availableWidth)), max(Self.minimumWidth, width))
        defaults.set(Double(preferredWidth), forKey: key)
    }

    private func widthLimit(_ availableWidth: CGFloat) -> CGFloat {
        max(0, min(Self.maximumWidth, availableWidth - Self.minimumContentWidth))
    }
}

/// AppKit normally focuses an accepting view before delivering its mouse-down.
/// A pointer resize must leave the current content responder untouched, while
/// the divider remains available to keyboard traversal and explicit AX focus.
@MainActor
final class SidebarFocusWindow: NSWindow {
    var onWorkspaceNumber: ((Int) -> Void)?
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if (NSApp as? BrowserInputApplication)?.browserKeyboardEvents.contain(event) == true { return true }
        (NSApp as? BrowserInputApplication)?.browserKeyboardEvents.observeUnowned(event)
        let modifiers = event.modifierFlags.intersection([.command, .option, .control, .shift])
        if event.type == .keyDown, modifiers == [.command],
           let key = event.charactersIgnoringModifiers, key.count == 1,
           let digit = Int(key), (1...9).contains(digit), let onWorkspaceNumber {
            // Consume even an unavailable row before a terminal/browser fallback.
            onWorkspaceNumber(digit)
            return true
        }
        return super.performKeyEquivalent(with: event)
    }

    override func sendEvent(_ event: NSEvent) {
        if (NSApp as? BrowserInputApplication)?.browserKeyboardEvents.contain(event) == true { return }
        (NSApp as? BrowserInputApplication)?.browserKeyboardEvents.observeUnowned(event)
        var divider: SidebarDividerView?
        if event.type == .leftMouseDown, let content = contentView {
            let point = content.superview?.convert(event.locationInWindow, from: nil) ?? event.locationInWindow
            divider = content.hitTest(point) as? SidebarDividerView
        }
        let previous = divider?.preservesContentResponder ?? false
        divider?.preservesContentResponder = true
        defer { divider?.preservesContentResponder = previous }
        super.sendEvent(event)
    }
}

/// Mouse resizing preserves the current content responder. Keyboard users can
/// focus the splitter and use arrows; accessibility uses the same width action.
@MainActor
final class SidebarDividerView: NSView {
    var onResize: (@MainActor (CGFloat) -> Void)?
    fileprivate var preservesContentResponder = false
    private var width: CGFloat = 224
    private var dragStart: (x: CGFloat, width: CGFloat)?

    init() {
        super.init(frame: .zero)
        setAccessibilityElement(true)
        setAccessibilityRole(.splitter)
        setAccessibilityLabel("Resize sidebar")
        setAccessibilityHelp("Drag to resize. Use Left and Right Arrow to adjust the sidebar width.")
    }
    required init?(coder: NSCoder) { nil }
    override var acceptsFirstResponder: Bool { !preservesContentResponder }
    override var canBecomeKeyView: Bool { window != nil && !isHiddenOrHasHiddenAncestor }

    func update(width: CGFloat) {
        self.width = width
        setAccessibilityValue(NSNumber(value: Double(width)))
        needsDisplay = true
    }

    override func resetCursorRects() { addCursorRect(bounds, cursor: .resizeLeftRight) }
    override func draw(_ dirtyRect: NSRect) {
        ZmuxPalette.separator.setFill()
        NSRect(x: bounds.midX - 0.5, y: 0, width: 1, height: bounds.height).fill()
        if window?.firstResponder === self {
            NSGraphicsContext.saveGraphicsState()
            NSFocusRingPlacement.only.set()
            bounds.insetBy(dx: 1, dy: 1).fill()
            NSGraphicsContext.restoreGraphicsState()
        }
    }
    override func becomeFirstResponder() -> Bool { needsDisplay = true; return true }
    override func resignFirstResponder() -> Bool { needsDisplay = true; return true }
    override func mouseDown(with event: NSEvent) { dragStart = (event.locationInWindow.x, width) }
    override func mouseDragged(with event: NSEvent) {
        guard let dragStart else { return }
        onResize?(dragStart.width + event.locationInWindow.x - dragStart.x)
    }
    override func mouseUp(with event: NSEvent) { dragStart = nil }
    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 123: onResize?(width - 16)
        case 124: onResize?(width + 16)
        default: super.keyDown(with: event)
        }
    }
    override func accessibilityPerformIncrement() -> Bool {
        guard let onResize else { return false }; onResize(width + 16); return true
    }
    override func accessibilityPerformDecrement() -> Bool {
        guard let onResize else { return false }; onResize(width - 16); return true
    }
}

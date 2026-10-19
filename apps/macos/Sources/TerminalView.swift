import AppKit

@MainActor
// The macOS 15 SDK's legacy input protocol lacks actor annotations. AppKit calls it
// on the UI thread; preserve MainActor isolation and runtime checking at this boundary.
final class TerminalView: NSView, @preconcurrency NSTextInputClient {
    var pane: Pane?
    var onFiles: (([String]) -> Void)?
    var onImage: ((Data) -> Bool)?
    var onPasteError: ((String) -> Void)?
    private var receivingFiles = false
    var onInput: ((String) -> Void)?
    var onKey: ((TerminalKeyEvent) -> Void)?
    var onPaste: ((String) -> Void)?
    /// Installed only for local terminal panes. Cloud viewers leave this unset so
    /// Command-click continues through their local selection route.
    var onOpenTerminalLink: ((TerminalLinkHit) -> Bool)?
    /// Remote viewers do not forward terminal mouse protocols, so pointer input remains local selection.
    var localPointerSelectionOnly = false
    private var semanticKeysDown: Set<UInt16> = []
    var onScroll: ((Int) -> Void)?
    var onWheel: ((Int, Int, Int, Int, Bool) -> Void)?
    // Admission to the bounded UI queue, not an acknowledgement of PTY delivery.
    var onPointer: ((TerminalPointerEvent) -> Bool)?
    private struct PointerSample: Equatable {
        let row: Int
        let col: Int
        let modifiers: Int
    }
    private enum PointerRoute: Equatable { case selection, application, refused }
    private struct PointerGesture {
        let route: PointerRoute
        var last: PointerSample
    }
    private struct PendingTerminalLink {
        let downEvent: NSEvent
        let startLocation: NSPoint
        let hit: TerminalLinkHit
    }
    private static let terminalLinkDragThreshold: CGFloat = 3
    private var pendingTerminalLink: PendingTerminalLink?
    private var pointerGestures: [TerminalPointerEvent.Button: PointerGesture] = [:]
    private var lastHover: PointerSample?
    private var pointerTrackingArea: NSTrackingArea?
    private weak var motionWindow: SidebarFocusWindow?
    private var wheelRemainder: CGFloat = 0
    var onResize: ((Int, Int) -> Void)?
    var onFocus: (() -> Void)?
    private var viewport: Viewport?
    private var frameState = "Starting shell…"
    private var lastSize: (Int, Int) = (0, 0)
    private var marked = NSAttributedString(string: "")
    private var markedSelection = NSRange(location: 0, length: 0)
    private let selection = TerminalSelection()
    private let font = NSFont.monospacedSystemFont(ofSize: 12.5, weight: .regular)
    private let contentInset: CGFloat = 10
    private lazy var cellWidth: CGFloat = ("M" as NSString).size(withAttributes: [.font: font]).width
    private lazy var lineHeight: CGFloat = ceil(font.ascender - font.descender + 3)
    private struct RenderStyle {
        let background: NSColor
        let attributes: [NSAttributedString.Key: Any]
    }
    private var renderStyles: [TerminalStyle: RenderStyle] = [:]
    private var fontVariants: [UInt: NSFont] = [:]
    override var acceptsFirstResponder: Bool { true }
    override var isFlipped: Bool { true }

    private var selectionSmokeTraceEnabled: Bool {
        CommandLine.arguments.contains("--terminal-selection-smoke")
    }
    private var selectionSmokeRange: String {
        guard let range = selection.range else { return "nil" }
        return "\(range.lower.row):\(range.lower.column)-\(range.upper.row):\(range.upper.column)"
    }
    private var selectionSmokePendingLink: String {
        guard let pendingTerminalLink else { return "false" }
        return "true@\(pendingTerminalLink.hit.row):\(pendingTerminalLink.hit.column)"
    }
    private func traceSelectionSmoke(_ name: String, event: NSEvent? = nil, details: String = "") {
        guard selectionSmokeTraceEnabled else { return }
        let responder = window?.firstResponder
        let windowIdentity = window.map { String(describing: ObjectIdentifier($0)) } ?? "nil"
        let responderIdentity = responder.map { String(describing: ObjectIdentifier($0)) } ?? "nil"
        let responderType = responder.map { String(describing: type(of: $0)) } ?? "nil"
        let coordinates = event.map {
            let viewPoint = convert($0.locationInWindow, from: nil)
            let grid = gridPoint($0)
            return "window=\($0.locationInWindow.x),\($0.locationInWindow.y) view=\(viewPoint.x),\(viewPoint.y) grid=\(grid.0),\(grid.1) modifiers=\($0.modifierFlags.rawValue)"
        } ?? "coordinates=none"
        print("[terminal-selection-smoke] \(name) view=\(ObjectIdentifier(self)) window=\(windowIdentity)#\(window?.windowNumber ?? -1) firstResponder=\(responderType):\(responderIdentity) pendingLink=\(selectionSmokePendingLink) pointerTracking=\(viewport?.pointerTracking?.rawValue ?? "nil") selection=\(selectionSmokeRange) \(coordinates) \(details)")
    }
    private func selectionSmokeTraceState() -> (pending: String, selection: String)? {
        guard selectionSmokeTraceEnabled else { return nil }
        return (selectionSmokePendingLink, selectionSmokeRange)
    }
    private func traceSelectionSmokeIfChanged(_ name: String, before: (pending: String, selection: String)?,
                                               event: NSEvent? = nil, details: String = "") {
        guard let before,
              before.pending != selectionSmokePendingLink || before.selection != selectionSmokeRange else { return }
        traceSelectionSmoke(name, event: event,
            details: "beforePending=\(before.pending) beforeSelection=\(before.selection) \(details)")
    }
    private func pointerRouteDescription(_ route: PointerRoute?) -> String {
        guard let route else { return "none" }
        switch route {
        case .selection: return "selection"
        case .application: return "application"
        case .refused: return "refused"
        }
    }

    init(pane: Pane? = nil) {
        self.pane = pane
        super.init(frame: .zero)
        registerForDraggedTypes(TerminalPasteboardContent.dragTypes)
        setAccessibilityElement(true)
        setAccessibilityRole(.textArea)
        setAccessibilityLabel("Terminal")
    }
    required init?(coder: NSCoder) { nil }
    override func layout() {
        super.layout()
        let rows = max(1, min(120, Int((bounds.height - contentInset * 2) / lineHeight)))
        let cols = max(1, min(240, Int((bounds.width - contentInset * 2) / cellWidth)))
        if lastSize != (rows, cols) { lastSize = (rows, cols); onResize?(rows, cols) }
    }
    override func becomeFirstResponder() -> Bool { needsDisplay = true; return true }
    override func resignFirstResponder() -> Bool {
        let before = selectionSmokeTraceState()
        semanticKeysDown.removeAll(); pendingTerminalLink = nil; cancelPointerGestures(); needsDisplay = true
        traceSelectionSmokeIfChanged("resignFirstResponder", before: before, details: "cancelled=true")
        return true
    }
    override func viewWillMove(toWindow newWindow: NSWindow?) {
        if motionWindow !== newWindow {
            motionWindow?.terminalMouseMovedEvents(owner: ObjectIdentifier(self), enabled: false)
            motionWindow = nil
        }
        super.viewWillMove(toWindow: newWindow)
    }
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        updateTrackingAreas()
    }
    override func updateTrackingAreas() {
        if let pointerTrackingArea { removeTrackingArea(pointerTrackingArea) }
        pointerTrackingArea = nil
        let enabled = !localPointerSelectionOnly && frameState == "running" && viewport?.pointerTracking == .some(.anyMotion) && !isHiddenOrHasHiddenAncestor && !visibleRect.isEmpty
        if enabled {
            let area = NSTrackingArea(rect: .zero,
                options: [.activeInKeyWindow, .inVisibleRect, .mouseMoved, .mouseEnteredAndExited],
                owner: self, userInfo: nil)
            addTrackingArea(area)
            pointerTrackingArea = area
        }
        motionWindow = window as? SidebarFocusWindow
        motionWindow?.terminalMouseMovedEvents(owner: ObjectIdentifier(self), enabled: enabled)
        super.updateTrackingAreas()
    }
    var requestedSize: (Int, Int) { lastSize }
    func update(_ frame: Frame) {
        let oldState = frameState
        let before = selectionSmokeTraceState()
        let previousViewport = viewport
        frameState = frame.failure ?? frame.snapshot?.failure ?? (frame.starting ? "Starting shell…" : frame.snapshot?.state ?? "Unavailable")
        if oldState == "running", frameState != "running" {
            pendingTerminalLink = nil
            pointerGestures.removeAll(); lastHover = nil
        }
        if let snapshot = frame.snapshot {
            let previous = viewport
            let oldTracking = viewport?.pointerTracking
            if viewport?.size.rows != snapshot.viewport.size.rows || viewport?.size.cols != snapshot.viewport.size.cols || viewport?.scrollbackOffset != snapshot.viewport.scrollbackOffset { selection.clear() }
            viewport = snapshot.viewport
            if oldTracking != snapshot.viewport.pointerTracking
                || previous?.alternateScreen != snapshot.viewport.alternateScreen
                || previous?.scrollbackOffset != snapshot.viewport.scrollbackOffset
                || previous?.size != snapshot.viewport.size
                || oldState != frameState {
                pendingTerminalLink = nil
            }
            if oldTracking != viewport?.pointerTracking || oldState != frameState { lastHover = nil; updateTrackingAreas() }
            if previous?.title != snapshot.viewport.title {
                setAccessibilityLabel(snapshot.viewport.title?.isEmpty == false ? snapshot.viewport.title : "Terminal")
            }
            if previous?.contents != snapshot.viewport.contents { setAccessibilityValue(snapshot.viewport.contents) }
            if let previous, previous.size == snapshot.viewport.size,
               previous.scrollbackOffset == snapshot.viewport.scrollbackOffset, oldState == frameState,
               previous.lines.count == snapshot.viewport.lines.count {
                for row in snapshot.viewport.lines.indices where previous.lines[row] != snapshot.viewport.lines[row] {
                    setNeedsDisplay(rowRect(row))
                }
                if window?.firstResponder === self, snapshot.viewport.scrollbackOffset == 0,
                   (!previous.hideCursor || !snapshot.viewport.hideCursor),
                   previous.cursorRow != snapshot.viewport.cursorRow || previous.cursorCol != snapshot.viewport.cursorCol
                    || previous.hideCursor != snapshot.viewport.hideCursor {
                    setNeedsDisplay(rowRect(previous.cursorRow))
                    setNeedsDisplay(rowRect(snapshot.viewport.cursorRow))
                }
            } else { needsDisplay = true }
        } else { updateTrackingAreas(); needsDisplay = true }
        let currentViewport = viewport
        let viewportChanged = previousViewport?.size != currentViewport?.size
            || previousViewport?.scrollbackOffset != currentViewport?.scrollbackOffset
            || previousViewport?.pointerTracking != currentViewport?.pointerTracking
            || previousViewport?.alternateScreen != currentViewport?.alternateScreen
        traceSelectionSmokeIfChanged("update", before: before,
            details: "stateChanged=\(oldState != frameState) viewportChanged=\(viewportChanged) rows=\(currentViewport?.size.rows ?? -1) cols=\(currentViewport?.size.cols ?? -1) scrollback=\(currentViewport?.scrollbackOffset ?? -1)")
    }
    override func draw(_ dirtyRect: NSRect) {
        NSGraphicsContext.current?.shouldAntialias = true
        ZmuxPalette.terminal.setFill()
        bounds.fill()
        if receivingFiles { NSColor.controlAccentColor.setStroke(); let border = NSBezierPath(rect: bounds.insetBy(dx: 2, dy: 2)); border.lineWidth = 2; border.stroke() }
        guard let viewport else {
            (frameState as NSString).draw(at: NSPoint(x: contentInset, y: contentInset), withAttributes: [.font: font, .foregroundColor: NSColor.secondaryLabelColor])
            return
        }
        for (row, runs) in viewport.lines.enumerated() where rowRect(row).intersects(dirtyRect) {
            for run in runs {
                let rect = NSRect(x: contentInset + CGFloat(run.column) * cellWidth, y: contentInset + CGFloat(row) * lineHeight, width: CGFloat(run.width) * cellWidth, height: lineHeight)
                let style = rendering(run.style)
                style.background.setFill(); rect.fill()
                (run.text as NSString).draw(at: rect.origin, withAttributes: style.attributes)
            }
        }
        if let (lo, hi) = normalizedSelection() {
            NSColor.selectedTextBackgroundColor.withAlphaComponent(0.4).setFill()
            for row in lo.0...hi.0 {
                let start = row == lo.0 ? lo.1 : 0
                let end = row == hi.0 ? hi.1 : viewport.size.cols
                NSRect(x: contentInset + CGFloat(start) * cellWidth, y: contentInset + CGFloat(row) * lineHeight, width: CGFloat(max(0, end - start)) * cellWidth, height: lineHeight).fill()
            }
        }
        let cursor = NSRect(x: contentInset + CGFloat(viewport.cursorCol) * cellWidth, y: contentInset + CGFloat(viewport.cursorRow) * lineHeight, width: cellWidth, height: lineHeight)
        if frameState == "running" && viewport.scrollbackOffset == 0 && !viewport.hideCursor && window?.firstResponder === self {
            NSColor.white.withAlphaComponent(0.4).setFill(); cursor.fill()
        }
        if hasMarkedText() && viewport.scrollbackOffset == 0 { marked.draw(at: cursor.origin) }
    }
    private func rowRect(_ row: Int) -> NSRect {
        NSRect(x: 0, y: contentInset + CGFloat(row) * lineHeight, width: bounds.width, height: lineHeight)
    }
    private func rendering(_ style: TerminalStyle) -> RenderStyle {
        if let cached = renderStyles[style] { return cached }
        var traits: NSFontTraitMask = []
        if style.bold { traits.insert(.boldFontMask) }
        if style.italic { traits.insert(.italicFontMask) }
        let fontKey = UInt(traits.rawValue)
        let variant = fontVariants[fontKey] ?? NSFontManager.shared.convert(font, toHaveTrait: traits)
        fontVariants[fontKey] = variant
        var attributes: [NSAttributedString.Key: Any] = [
            .font: variant,
            .foregroundColor: color(style.inverse ? style.background : style.foreground, foreground: true)]
        if style.underline { attributes[.underlineStyle] = NSUnderlineStyle.single.rawValue }
        let rendered = RenderStyle(background: color(style.inverse ? style.foreground : style.background, foreground: false), attributes: attributes)
        if renderStyles.count >= 256 { renderStyles.removeAll(keepingCapacity: true) }
        renderStyles[style] = rendered
        return rendered
    }
    private func color(_ color: TerminalColor, foreground: Bool) -> NSColor {
        if let rgb = color.rgb, rgb.count == 3 { return NSColor(calibratedRed: CGFloat(rgb[0]) / 255, green: CGFloat(rgb[1]) / 255, blue: CGFloat(rgb[2]) / 255, alpha: 1) }
        guard let index = color.index else { return foreground ? NSColor(calibratedWhite: 0.88, alpha: 1) : ZmuxPalette.terminal }
        let palette: [UInt32] = [0x1c1c1e,0xe06c75,0x98c379,0xe5c07b,0x61afef,0xc678dd,0x56b6c2,0xd9dce3,0x5c6370,0xff7b86,0xb0df8e,0xffd68b,0x83c3ff,0xdf98ff,0x78dae6,0xffffff]
        if index < 16 {
            let value = palette[index]
            return NSColor(calibratedRed: CGFloat((value >> 16) & 255) / 255, green: CGFloat((value >> 8) & 255) / 255, blue: CGFloat(value & 255) / 255, alpha: 1)
        }
        if index >= 232 { return NSColor(calibratedWhite: CGFloat(8 + (index - 232) * 10) / 255, alpha: 1) }
        let value = index - 16
        let levels: [CGFloat] = [0,95,135,175,215,255]
        return NSColor(calibratedRed: levels[value / 36] / 255, green: levels[(value / 6) % 6] / 255, blue: levels[value % 6] / 255, alpha: 1)
    }
    private func navigateHistory(_ lines: Int) {
        selection.clear(); needsDisplay = true
        onScroll?(lines)
    }
    override func scrollWheel(with event: NSEvent) {
        guard viewport != nil else { return }
        if event.phase == .began { wheelRemainder = 0 }
        wheelRemainder += event.hasPreciseScrollingDeltas ? event.scrollingDeltaY / lineHeight : event.scrollingDeltaY * 3
        let lines = Int(max(-1000, min(1000, wheelRemainder)))
        guard lines != 0 else { return }
        wheelRemainder -= CGFloat(lines)
        let (row, col) = gridPoint(event)
        let modifiers = (event.modifierFlags.contains(.option) ? 8 : 0) | (event.modifierFlags.contains(.control) ? 16 : 0)
        selection.clear(); needsDisplay = true
        onWheel?(lines, row, col, modifiers, event.modifierFlags.contains(.shift))
    }

    override func mouseDown(with event: NSEvent) {
        traceSelectionSmoke("mouseDown.entry", event: event)
        let before = selectionSmokeTraceState()
        if beginTerminalLink(event) {
            traceSelectionSmokeIfChanged("mouseDown", before: before, event: event, details: "path=pending-link")
            return
        }
        beginPointer(event, button: .left)
        traceSelectionSmokeIfChanged("mouseDown", before: before, event: event,
            details: "path=pointer route=\(pointerRouteDescription(pointerGestures[.left]?.route))")
    }
    override func mouseDragged(with event: NSEvent) {
        let before = selectionSmokeTraceState()
        var threshold: String = "not-applicable"
        if let pending = pendingTerminalLink {
            let exceeded = terminalLinkMovementExceedsThreshold(event, from: pending.startLocation)
            threshold = exceeded ? "exceeded" : "not-exceeded"
            guard exceeded else { return }
            pendingTerminalLink = nil
            beginPointer(pending.downEvent, button: .left, focus: false)
            dragPointer(event, button: .left)
            traceSelectionSmokeIfChanged("mouseDragged", before: before, event: event,
                details: "threshold=\(threshold) route=\(pointerRouteDescription(pointerGestures[.left]?.route))")
            return
        }
        dragPointer(event, button: .left)
        traceSelectionSmokeIfChanged("mouseDragged", before: before, event: event,
            details: "threshold=\(threshold) route=\(pointerRouteDescription(pointerGestures[.left]?.route))")
    }
    override func mouseUp(with event: NSEvent) {
        traceSelectionSmoke("mouseUp.entry", event: event)
        let before = selectionSmokeTraceState()
        var threshold = "not-applicable"
        var path = "pointer"
        if let pending = pendingTerminalLink {
            pendingTerminalLink = nil
            let exceeded = terminalLinkMovementExceedsThreshold(event, from: pending.startLocation)
            threshold = exceeded ? "exceeded" : "not-exceeded"
            if exceeded {
                // AppKit can coalesce away mouseDragged. Preserve the ordinary
                // pointer/selection gesture when mouse-up itself proves motion.
                path = "far-drag"
                beginPointer(pending.downEvent, button: .left, focus: false)
                dragPointer(event, button: .left)
                endPointer(event, button: .left)
                traceSelectionSmokeIfChanged("mouseUp", before: before, event: event,
                    details: "threshold=\(threshold) path=\(path)")
                return
            }
            if onOpenTerminalLink?(pending.hit) == true {
                lastHover = nil
                path = "opened-link"
                traceSelectionSmokeIfChanged("mouseUp", before: before, event: event,
                    details: "threshold=\(threshold) path=\(path)")
                return
            }
            // Admission can fail if the owner disappeared or its queue is full.
            // Re-run the original click through the normal selection/mouse route.
            path = "link-fallback"
            beginPointer(pending.downEvent, button: .left, focus: false)
        }
        endPointer(event, button: .left)
        traceSelectionSmokeIfChanged("mouseUp", before: before, event: event,
            details: "threshold=\(threshold) path=\(path)")
    }
    private func terminalLinkMovementExceedsThreshold(_ event: NSEvent, from start: NSPoint) -> Bool {
        let deltaX = event.locationInWindow.x - start.x
        let deltaY = event.locationInWindow.y - start.y
        return hypot(deltaX, deltaY) > Self.terminalLinkDragThreshold
    }
    override func rightMouseDown(with event: NSEvent) { beginPointer(event, button: .right) }
    override func rightMouseDragged(with event: NSEvent) { dragPointer(event, button: .right) }
    override func rightMouseUp(with event: NSEvent) { endPointer(event, button: .right) }
    override func otherMouseDown(with event: NSEvent) {
        if event.buttonNumber == 2 { beginPointer(event, button: .middle) } else { super.otherMouseDown(with: event) }
    }
    override func otherMouseDragged(with event: NSEvent) {
        if event.buttonNumber == 2 { dragPointer(event, button: .middle) } else { super.otherMouseDragged(with: event) }
    }
    override func otherMouseUp(with event: NSEvent) {
        if event.buttonNumber == 2 { endPointer(event, button: .middle) } else { super.otherMouseUp(with: event) }
    }
    override func mouseEntered(with event: NSEvent) { lastHover = nil }
    override func mouseExited(with event: NSEvent) { lastHover = nil }
    override func mouseMoved(with event: NSEvent) {
        guard frameState == "running", viewport?.pointerTracking == .some(.anyMotion), pointerGestures.isEmpty,
              !event.modifierFlags.contains(.shift) else { return }
        let sample = pointerSample(event)
        guard sample != lastHover else { return }
        if sendPointer(.motion, button: .none, sample: sample) { lastHover = sample }
    }
    private func beginTerminalLink(_ event: NSEvent) -> Bool {
        let relevantModifiers = event.modifierFlags.intersection([.command, .shift, .option, .control])
        guard relevantModifiers == .command, onOpenTerminalLink != nil,
              let viewport, let link = terminalLink(at: gridPoint(event), viewport: viewport) else { return false }
        window?.makeFirstResponder(self); onFocus?()
        pendingTerminalLink = PendingTerminalLink(downEvent: event, startLocation: event.locationInWindow,
                                                  hit: TerminalLinkHit(viewportGeneration: viewport.viewportGeneration,
                                                                       row: link.row, column: link.column,
                                                                       expectedURL: link.url))
        lastHover = nil
        return true
    }
    private func terminalLink(at point: (Int, Int), viewport: Viewport) -> (row: Int, column: Int, url: String)? {
        guard let span = viewport.links.first(where: {
            $0.row == point.0 && point.1 >= $0.startColumn && point.1 < $0.endColumn
        }), isHTTPURLCandidate(span.url) else { return nil }
        return (point.0, point.1, span.url)
    }
    private func isHTTPURLCandidate(_ value: String) -> Bool {
        let lowercased = value.lowercased()
        return lowercased.hasPrefix("http://") || lowercased.hasPrefix("https://")
    }
    private func beginPointer(_ event: NSEvent, button: TerminalPointerEvent.Button, focus: Bool = true) {
        let before = selectionSmokeTraceState()
        if focus { window?.makeFirstResponder(self); onFocus?() }
        guard let viewport else { return }
        // A route is latched at press, including Shift override and refused input.
        // Modifier/mode changes cannot turn the remainder into another gesture.
        if let previous = pointerGestures.removeValue(forKey: button), previous.route == .application {
            _ = sendPointer(.release, button: button, sample: previous.last)
        }
        let sample = pointerSample(event)
        let local = localPointerSelectionOnly || frameState != "running" || viewport.pointerTracking == nil || viewport.pointerTracking == .some(.none) || event.modifierFlags.contains(.shift)
        let route: PointerRoute
        if local {
            route = button == .left ? .selection : .refused
            if button == .left {
                let point = gridPoint(event)
                selection.begin(row: point.0, column: point.1, clicks: event.clickCount, viewport: viewport)
            }
        } else {
            selection.clear()
            route = sendPointer(.press, button: button, sample: sample) ? .application : .refused
        }
        pointerGestures[button] = PointerGesture(route: route, last: sample)
        lastHover = nil
        needsDisplay = true
        traceSelectionSmokeIfChanged("beginPointer", before: before, event: event,
            details: "button=\(button.rawValue) route=\(pointerRouteDescription(route)) local=\(local) clickCount=\(event.clickCount)")
    }
    private func dragPointer(_ event: NSEvent, button: TerminalPointerEvent.Button) {
        guard var gesture = pointerGestures[button] else { return }
        switch gesture.route {
        case .selection:
            guard let viewport else { return }
            let point = gridPoint(event)
            selection.extend(row: point.0, column: point.1, viewport: viewport)
            needsDisplay = true
        case .application:
            let sample = pointerSample(event)
            guard sample != gesture.last else { return }
            if sendPointer(.motion, button: button, sample: sample) {
                gesture.last = sample
                pointerGestures[button] = gesture
            }
        case .refused: break
        }
    }
    private func endPointer(_ event: NSEvent, button: TerminalPointerEvent.Button) {
        guard let gesture = pointerGestures.removeValue(forKey: button) else { return }
        if gesture.route == .application { _ = sendPointer(.release, button: button, sample: pointerSample(event)) }
        lastHover = nil
    }
    // Focus loss ends native ownership. Releases use the same admission/error
    // path; a full queue remains explicit failure, with no unbounded retry task.
    func cancelPointerGestures() {
        let before = selectionSmokeTraceState()
        for button in [TerminalPointerEvent.Button.left, .middle, .right] {
            if let gesture = pointerGestures.removeValue(forKey: button), gesture.route == .application {
                _ = sendPointer(.release, button: button, sample: gesture.last)
            }
        }
        lastHover = nil
        traceSelectionSmokeIfChanged("cancelPointerGestures", before: before, details: "cancelled=true")
    }
    private func pointerSample(_ event: NSEvent) -> PointerSample {
        let (row, col) = gridPoint(event)
        let flags = event.modifierFlags
        return PointerSample(row: row, col: min(col, max(0, (viewport?.size.cols ?? 1) - 1)),
            modifiers: (flags.contains(.shift) ? 4 : 0) | (flags.contains(.option) ? 8 : 0) | (flags.contains(.control) ? 16 : 0))
    }
    private func sendPointer(_ action: TerminalPointerEvent.Action, button: TerminalPointerEvent.Button, sample: PointerSample) -> Bool {
        onPointer?(TerminalPointerEvent(action: action, button: button, row: sample.row, col: sample.col, modifiers: sample.modifiers)) ?? false
    }
    private func gridPoint(_ event: NSEvent) -> (Int, Int) {
        let point = convert(event.locationInWindow, from: nil)
        return (max(0, min((viewport?.size.rows ?? 1) - 1, Int((point.y - contentInset) / lineHeight))), max(0, min(viewport?.size.cols ?? 1, Int((point.x - contentInset) / cellWidth))))
    }
    private func normalizedSelection() -> ((Int, Int), (Int, Int))? {
        guard let range = selection.range, !range.isEmpty else { return nil }
        return ((range.lower.row, range.lower.column), (range.upper.row, range.upper.column))
    }
    func copySelection(_ sender: Any?) {
        guard let viewport, let text = selection.text(viewport: viewport) else { return }
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(text, forType: .string)
    }
    override func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
        guard TerminalPasteboardContent.hasFilesOrImage(sender.draggingPasteboard) else { return [] }
        receivingFiles = true; needsDisplay = true
        return .copy
    }
    override func draggingExited(_ sender: (any NSDraggingInfo)?) { receivingFiles = false; needsDisplay = true }
    override func prepareForDragOperation(_ sender: any NSDraggingInfo) -> Bool { receivingFiles }
    override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
        receivingFiles = false; needsDisplay = true
        return paste(from: sender.draggingPasteboard, includeText: false)
    }
    override func concludeDragOperation(_ sender: (any NSDraggingInfo)?) { receivingFiles = false; needsDisplay = true }
    func pasteFromClipboard(_ sender: Any?) {
        _ = paste(from: .general)
    }
    @discardableResult
    func paste(from pasteboard: NSPasteboard, includeText: Bool = true) -> Bool {
        do {
            switch try TerminalPasteboardContent.read(pasteboard, includeText: includeText) {
            case .files(let paths):
                guard let onFiles else { return false }
                onFiles(paths)
                return true
            case .image(let data):
                return onImage?(data) ?? false
            case .text(let text):
                guard let onPaste else { return false }
                onPaste(text)
                return true
            case .empty:
                return false
            }
        } catch {
            onPasteError?(error.localizedDescription)
            return false
        }
    }
    func selectViewport(_ sender: Any?) {
        if let viewport { selection.selectAll(viewport: viewport); needsDisplay = true }
    }
    override func keyDown(with event: NSEvent) {
        if event.modifierFlags.contains(.command) && (event.keyCode == 115 || event.keyCode == 119) {
            navigateHistory(event.keyCode == 115 ? 1000 : 0); return
        }
        if event.modifierFlags.contains(.shift) && (event.keyCode == 116 || event.keyCode == 121) {
            navigateHistory((event.keyCode == 116 ? 1 : -1) * max(1, (viewport?.size.rows ?? 2) - 1)); return
        }
        if event.modifierFlags.contains(.command) { super.keyDown(with: event); return }
        if hasMarkedText() { interpretKeyEvents([event]); return }
        selection.clear(); needsDisplay = true
        if let semantic = TerminalKeyEncoder.event(keyCode: event.keyCode, modifiers: event.modifierFlags,
                                                   action: event.isARepeat ? .repeated : .press,
                                                   charactersIgnoringModifiers: event.charactersIgnoringModifiers) {
            if let onKey { onKey(semantic); semanticKeysDown.insert(event.keyCode) }
            return
        }
        if event.modifierFlags.contains(.control), let scalar = event.charactersIgnoringModifiers?.unicodeScalars.first, scalar.value < 128 {
            onInput?(String(UnicodeScalar(scalar.value & 31)!)); return
        }
        interpretKeyEvents([event])
    }
    override func keyUp(with event: NSEvent) {
        if semanticKeysDown.remove(event.keyCode) != nil,
           let semantic = TerminalKeyEncoder.event(keyCode: event.keyCode, modifiers: event.modifierFlags, action: .release) {
            onKey?(semantic)
            return
        }
        super.keyUp(with: event)
    }
    func insertText(_ string: Any, replacementRange: NSRange) {
        MainActor.preconditionIsolated()
        unmarkText()
        let text = (string as? NSAttributedString)?.string ?? (string as? String ?? "")
        if !text.isEmpty { onInput?(text) }
    }
    override func doCommand(by selector: Selector) {
        let commands: [String:String] = ["insertNewline:":"\r","deleteBackward:":"\u{7f}","insertTab:":"\t","cancelOperation:":"\u{1b}"]
        if let command = commands[NSStringFromSelector(selector)] { onInput?(command) }
    }
    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        MainActor.preconditionIsolated()
        let text = (string as? NSAttributedString)?.string ?? (string as? String ?? "")
        marked = NSAttributedString(string: text, attributes: [.font: font, .foregroundColor: NSColor.labelColor, .underlineStyle: NSUnderlineStyle.single.rawValue])
        markedSelection = selectedRange; needsDisplay = true
    }
    func unmarkText() { marked = NSAttributedString(string: ""); needsDisplay = true }
    func selectedRange() -> NSRange { markedSelection }
    func markedRange() -> NSRange { hasMarkedText() ? NSRange(location: 0, length: marked.length) : NSRange(location: NSNotFound, length: 0) }
    func hasMarkedText() -> Bool { marked.length > 0 }
    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [.font,.foregroundColor,.underlineStyle] }
    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? {
        guard hasMarkedText(), range.location != NSNotFound else { return nil }
        let intersection = NSIntersectionRange(range, NSRange(location: 0, length: marked.length))
        actualRange?.pointee = intersection
        return marked.attributedSubstring(from: intersection)
    }
    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        let rect = NSRect(x: contentInset + CGFloat(viewport?.cursorCol ?? 0) * cellWidth, y: contentInset + CGFloat(viewport?.cursorRow ?? 0) * lineHeight, width: cellWidth, height: lineHeight)
        return window?.convertToScreen(convert(rect, to: nil)) ?? .zero
    }
    func characterIndex(for point: NSPoint) -> Int { 0 }
}

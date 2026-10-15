import AppKit

@MainActor
// The macOS 15 SDK's legacy input protocol lacks actor annotations. AppKit calls it
// on the UI thread; preserve MainActor isolation and runtime checking at this boundary.
final class TerminalView: NSView, @preconcurrency NSTextInputClient {
    var pane: Pane
    var onFiles: (([String]) -> Void)?
    private var receivingFiles = false
    var onInput: ((String) -> Void)?
    var onScroll: ((Int) -> Void)?
    var onWheel: ((Int, Int, Int, Int, Bool) -> Void)?
    private var wheelRemainder: CGFloat = 0
    var onResize: ((Int, Int) -> Void)?
    var onFocus: (() -> Void)?
    private var viewport: Viewport?
    private var frameState = "Starting shell…"
    private var lastBytes: UInt64 = .max
    private var lastSize: (Int, Int) = (0, 0)
    private var marked = NSAttributedString(string: "")
    private var markedSelection = NSRange(location: 0, length: 0)
    private var selection: ((Int, Int), (Int, Int))?
    private let font = NSFont.monospacedSystemFont(ofSize: 12.5, weight: .regular)
    private let contentInset: CGFloat = 10
    private var cellWidth: CGFloat { ("M" as NSString).size(withAttributes: [.font: font]).width }
    private var lineHeight: CGFloat { ceil(font.ascender - font.descender + 3) }
    override var acceptsFirstResponder: Bool { true }
    override var isFlipped: Bool { true }

    init(pane: Pane) {
        self.pane = pane
        super.init(frame: .zero)
        registerForDraggedTypes([.fileURL])
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
    override func resignFirstResponder() -> Bool { needsDisplay = true; return true }
    var requestedSize: (Int, Int) { lastSize }
    func update(_ frame: Frame) {
        let oldState = frameState
        frameState = frame.failure ?? frame.snapshot?.failure ?? (frame.starting ? "Starting shell…" : frame.snapshot?.state ?? "Unavailable")
        if let snapshot = frame.snapshot {
            if viewport?.size.rows != snapshot.viewport.size.rows || viewport?.size.cols != snapshot.viewport.size.cols || viewport?.scrollbackOffset != snapshot.viewport.scrollbackOffset { selection = nil }
            let changed = lastBytes != snapshot.bytesRead || viewport?.scrollbackOffset != snapshot.viewport.scrollbackOffset || viewport?.size.rows != snapshot.viewport.size.rows || viewport?.size.cols != snapshot.viewport.size.cols || oldState != frameState
            viewport = snapshot.viewport
            lastBytes = snapshot.bytesRead
            setAccessibilityLabel(snapshot.viewport.title?.isEmpty == false ? snapshot.viewport.title : "Terminal")
            setAccessibilityValue(snapshot.viewport.contents)
            if changed { needsDisplay = true }
        } else { needsDisplay = true }
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
        for (row, runs) in viewport.lines.enumerated() {
            for run in runs {
                let rect = NSRect(x: contentInset + CGFloat(run.column) * cellWidth, y: contentInset + CGFloat(row) * lineHeight, width: CGFloat(run.width) * cellWidth, height: lineHeight)
                let foreground = color(run.style.inverse ? run.style.background : run.style.foreground, foreground: true)
                let background = color(run.style.inverse ? run.style.foreground : run.style.background, foreground: false)
                background.setFill(); rect.fill()
                var traits: NSFontTraitMask = []
                if run.style.bold { traits.insert(.boldFontMask) }
                if run.style.italic { traits.insert(.italicFontMask) }
                var attributes: [NSAttributedString.Key: Any] = [.font: NSFontManager.shared.convert(font, toHaveTrait: traits), .foregroundColor: foreground]
                if run.style.underline { attributes[.underlineStyle] = NSUnderlineStyle.single.rawValue }
                (run.text as NSString).draw(at: rect.origin, withAttributes: attributes)
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
        selection = nil; needsDisplay = true
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
        selection = nil; needsDisplay = true
        onWheel?(lines, row, col, modifiers, event.modifierFlags.contains(.shift))
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self); onFocus?()
        let point = gridPoint(event)
        selection = (point, point); needsDisplay = true
    }
    override func mouseDragged(with event: NSEvent) {
        if let old = selection { selection = (old.0, gridPoint(event)); needsDisplay = true }
    }
    private func gridPoint(_ event: NSEvent) -> (Int, Int) {
        let point = convert(event.locationInWindow, from: nil)
        return (max(0, min((viewport?.size.rows ?? 1) - 1, Int((point.y - contentInset) / lineHeight))), max(0, min(viewport?.size.cols ?? 1, Int((point.x - contentInset) / cellWidth))))
    }
    private func normalizedSelection() -> ((Int, Int), (Int, Int))? {
        guard let (a, b) = selection, a != b else { return nil }
        return a <= b ? (a, b) : (b, a)
    }
    func copySelection(_ sender: Any?) {
        guard let viewport, let (lo, hi) = normalizedSelection() else { return }
        var lines: [String] = []
        for row in lo.0...hi.0 {
            let lower = row == lo.0 ? lo.1 : 0
            let upper = row == hi.0 ? hi.1 : viewport.size.cols
            var text = ""
            for run in viewport.lines[row] {
                let start = max(lower, run.column), end = min(upper, run.column + run.width)
                if start < end {
                    text += run.text.isASCII ? String(run.text.dropFirst(start - run.column).prefix(end - start)) : run.text
                }
            }
            lines.append(text.replacingOccurrences(of: " +$", with: "", options: .regularExpression))
        }
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(lines.joined(separator: "\n"), forType: .string)
    }
    private func filePaths(_ pasteboard: NSPasteboard) -> [String] {
        (pasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []).filter { $0.isFileURL }.map { $0.path }
    }
    override func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
        guard sender.draggingPasteboard.canReadObject(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) else { return [] }
        receivingFiles = true; needsDisplay = true
        return .copy
    }
    override func draggingExited(_ sender: (any NSDraggingInfo)?) { receivingFiles = false; needsDisplay = true }
    override func prepareForDragOperation(_ sender: any NSDraggingInfo) -> Bool { receivingFiles }
    override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
        receivingFiles = false; needsDisplay = true
        let paths = filePaths(sender.draggingPasteboard)
        guard !paths.isEmpty, let onFiles else { return false }
        onFiles(paths)
        return true
    }
    override func concludeDragOperation(_ sender: (any NSDraggingInfo)?) { receivingFiles = false; needsDisplay = true }
    func pasteFromClipboard(_ sender: Any?) {
        let paths = filePaths(NSPasteboard.general)
        if !paths.isEmpty { onFiles?(paths); return }
        guard var text = NSPasteboard.general.string(forType: .string) else { return }
        if viewport?.bracketedPaste == true { text = "\u{1b}[200~" + text + "\u{1b}[201~" }
        else { text = text.replacingOccurrences(of: "\r\n", with: "\n").replacingOccurrences(of: "\n", with: "\r") }
        onInput?(text)
    }
    func selectViewport(_ sender: Any?) {
        if let viewport { selection = ((0,0),(viewport.size.rows-1,viewport.size.cols)); needsDisplay = true }
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
        selection = nil
        let arrows: [UInt16: String] = [123:"D",124:"C",125:"B",126:"A"]
        if let direction = arrows[event.keyCode] {
            var modifier = 1
            if event.modifierFlags.contains(.shift) { modifier += 1 }
            if event.modifierFlags.contains(.option) { modifier += 2 }
            if event.modifierFlags.contains(.control) { modifier += 4 }
            onInput?(modifier > 1 ? "\u{1b}[1;\(modifier)\(direction)" : "\u{1b}\(viewport?.applicationCursor == true ? "O" : "[")\(direction)")
            return
        }
        if event.modifierFlags.contains(.control), let scalar = event.charactersIgnoringModifiers?.unicodeScalars.first, scalar.value < 128 {
            onInput?(String(UnicodeScalar(scalar.value & 31)!)); return
        }
        let sequences: [UInt16:String] = [36:"\r",76:"\r",51:"\u{7f}",53:"\u{1b}",115:"\u{1b}[H",119:"\u{1b}[F",116:"\u{1b}[5~",121:"\u{1b}[6~",117:"\u{1b}[3~"]
        if event.keyCode == 48 { onInput?(event.modifierFlags.contains(.shift) ? "\u{1b}[Z" : "\t"); return }
        if let sequence = sequences[event.keyCode] { onInput?(sequence); return }
        interpretKeyEvents([event])
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
private extension String { var isASCII: Bool { unicodeScalars.allSatisfy { $0.isASCII } } }

import AppKit

/// Verify the public NSView repaint contract synchronously, between normal
/// coordinator updates. No timing threshold or test-only production counter.
@MainActor
enum TerminalDamageSmoke {
    static func validate(view: TerminalView, frame: Frame) throws {
        view.window?.contentView?.layoutSubtreeIfNeeded()
        guard view.window != nil, !view.visibleRect.isEmpty,
              !view.isHiddenOrHasHiddenAncestor, view.bounds.width > 0, view.bounds.height > 0 else {
            throw BridgeFailure(message: "Terminal damage fixture requires a laid-out visible terminal")
        }
        guard let original = frame.snapshot, original.viewport.size.cols > 1 else {
            throw BridgeFailure(message: "Terminal damage fixture requires a live grid")
        }
        defer { view.update(frame); view.needsDisplay = true }
        // AppKit can forward invalidation to a backing layer without retaining
        // NSView's dirty flag. Observe both public rendering owners.
        func isDirty() -> Bool { view.needsDisplay || (view.layer?.needsDisplay() ?? false) }
        func flush() {
            view.displayIfNeeded()
            view.layer?.displayIfNeeded()
            view.needsDisplay = false
        }
        view.update(frame)
        flush()
        let metadataOnly = Frame(sessionId: frame.sessionId, processId: frame.processId, starting: false, reconnectable: frame.reconnectable,
            snapshot: TerminalSnapshot(state: original.state, viewport: original.viewport,
                bytesRead: original.bytesRead + 1, exitCode: original.exitCode, failure: original.failure), failure: frame.failure)
        view.update(metadataOnly)
        guard !isDirty() else {
            throw BridgeFailure(message: "Metadata-only terminal output requested a pixel repaint")
        }
        var cursor = original.viewport
        cursor.cursorCol = (cursor.cursorCol + 1) % cursor.size.cols
        let moved = Frame(sessionId: frame.sessionId, processId: frame.processId, starting: false, reconnectable: frame.reconnectable,
            snapshot: TerminalSnapshot(state: original.state, viewport: cursor,
                bytesRead: original.bytesRead + 2, exitCode: original.exitCode, failure: original.failure), failure: frame.failure)
        view.update(moved)
        if !cursor.hideCursor, view.window?.firstResponder === view, !isDirty() {
            throw BridgeFailure(message: "Visible terminal cursor change lost its repaint")
        }
        flush()
        view.update(moved)
        guard !isDirty() else { throw BridgeFailure(message: "Repeated terminal frame requested a repaint") }
    }
}

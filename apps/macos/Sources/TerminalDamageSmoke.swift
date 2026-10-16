import AppKit

/// Verify the public NSView repaint contract synchronously, between normal
/// coordinator updates. No timing threshold or test-only production counter.
@MainActor
enum TerminalDamageSmoke {
    static func validate(view: TerminalView, frame: Frame) throws {
        guard let original = frame.snapshot, original.viewport.size.cols > 1 else {
            throw BridgeFailure(message: "Terminal damage fixture requires a live grid")
        }
        defer { view.update(frame); view.needsDisplay = true }
        view.update(frame)
        view.needsDisplay = false
        let metadataOnly = Frame(sessionId: frame.sessionId, processId: frame.processId, starting: false,
            snapshot: TerminalSnapshot(state: original.state, viewport: original.viewport,
                bytesRead: original.bytesRead + 1, exitCode: original.exitCode, failure: original.failure), failure: frame.failure)
        view.update(metadataOnly)
        guard !view.needsDisplay else {
            throw BridgeFailure(message: "Metadata-only terminal output requested a pixel repaint")
        }
        var cursor = original.viewport
        cursor.cursorCol = (cursor.cursorCol + 1) % cursor.size.cols
        let moved = Frame(sessionId: frame.sessionId, processId: frame.processId, starting: false,
            snapshot: TerminalSnapshot(state: original.state, viewport: cursor,
                bytesRead: original.bytesRead + 2, exitCode: original.exitCode, failure: original.failure), failure: frame.failure)
        view.update(moved)
        if !cursor.hideCursor, view.window?.firstResponder === view, !view.needsDisplay {
            throw BridgeFailure(message: "Visible terminal cursor change lost its repaint")
        }
        view.needsDisplay = false
        view.update(moved)
        guard !view.needsDisplay else { throw BridgeFailure(message: "Repeated terminal frame requested a repaint") }
    }
}

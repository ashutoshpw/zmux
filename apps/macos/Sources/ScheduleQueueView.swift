import AppKit

/// Keep columns inside the actual clip width, including split and scroller changes.
final class ScheduleQueueScrollView: NSScrollView {
    let table = NSTableView()

    override init(frame: NSRect) {
        super.init(frame: frame)
        table.addTableColumn(makeColumn("Message", identifier: "message", width: 150))
        table.addTableColumn(makeColumn("Remote target", identifier: "target", width: 100))
        table.addTableColumn(makeColumn("Scheduled", identifier: "deadline", width: 110))
        table.addTableColumn(makeColumn("State", identifier: "state", width: 90))
        // Automatic inset styling adds spacing outside the configured column widths.
        table.style = .fullWidth
        table.intercellSpacing = .zero
        table.autoresizingMask = [.width]
        table.columnAutoresizingStyle = .uniformColumnAutoresizingStyle
        table.headerView = NSTableHeaderView()
        table.rowHeight = 42
        table.usesAlternatingRowBackgroundColors = true
        table.allowsMultipleSelection = false
        table.setAccessibilityLabel("Scheduled message list")
        hasVerticalScroller = true
        hasHorizontalScroller = true
        autohidesScrollers = true
        documentView = table
        setAccessibilityLabel("Pending scheduled messages and history")
    }

    required init?(coder: NSCoder) { nil }

    private func makeColumn(_ title: String, identifier: String, width: CGFloat) -> NSTableColumn {
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(identifier))
        column.title = title
        column.width = width
        column.minWidth = 72
        column.resizingMask = [.userResizingMask, .autoresizingMask]
        return column
    }

    override func tile() {
        super.tile()
        guard let table = documentView as? NSTableView else { return }
        let width = contentView.bounds.width
        if width > 0, abs(table.frame.width - width) > 0.5 {
            table.setFrameSize(NSSize(width: width, height: table.frame.height))
        }
    }
}

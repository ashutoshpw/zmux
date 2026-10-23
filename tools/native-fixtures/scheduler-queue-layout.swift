import AppKit
import Foundation
import Darwin

@MainActor
private final class Rows: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    var count = 1
    func numberOfRows(in tableView: NSTableView) -> Int { count }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = NSTableCellView()
        let value: String
        switch tableColumn?.identifier.rawValue {
        case "message": value = "Scheduled reminder"
        case "target": value = "remote / pane %0"
        case "deadline": value = "2026-10-23 09:00"
        default: value = "cancelled · v5"
        }
        let text = NSTextField(labelWithString: value)
        text.font = .systemFont(ofSize: 11)
        text.frame = cell.bounds
        text.autoresizingMask = [.width, .height]
        cell.textField = text
        cell.addSubview(text)
        return cell
    }
}

/// Component-owner geometry at the measured failing viewport, independent of Rust/SSH.
@main @MainActor
struct SchedulerQueueLayout {
    static func main() {
        guard CommandLine.arguments.count == 3 else { exit(2) }
        let destination = URL(fileURLWithPath: CommandLine.arguments[1])
        let source = CommandLine.arguments[2]
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        app.finishLaunching()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 483, height: 527),
                              styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.appearance = NSAppearance(named: .darkAqua)
        let queue = ScheduleQueueScrollView(frame: .zero)
        let rows = Rows()
        queue.table.dataSource = rows
        queue.table.delegate = rows
        queue.autoresizingMask = [.width, .height]
        window.contentView = queue
        window.orderFrontRegardless()
        var observations: [[String: Any]] = []
        var failed = false
        let widths: [CGFloat] = [483, 470, 640, 470, 483]
        let scenarios: [(NSScroller.Style, Int)] = [(.overlay, 1), (.legacy, 30)]
        for (scrollerStyle, rowCount) in scenarios {
            queue.scrollerStyle = scrollerStyle
            rows.count = rowCount
            for width in widths {
                window.setContentSize(NSSize(width: width, height: 527))
                queue.table.reloadData()
                for _ in 0..<3 {
                    window.contentView?.layoutSubtreeIfNeeded()
                    queue.tile()
                    RunLoop.main.run(until: Date().addingTimeInterval(0.05))
                }
                let visible = queue.table.visibleRect
                let rects = queue.table.tableColumns.indices.map { queue.table.rect(ofColumn: $0) }
                let fits = rects.allSatisfy { $0.minX >= visible.minX - 1 && $0.maxX <= visible.maxX + 1 }
                observations.append([
                    "requested_width": width, "fits": fits, "rows": rowCount,
                    "scroller_style": scrollerStyle.rawValue,
                    "visible": String(describing: visible), "table": String(describing: queue.table.frame),
                    "columns": rects.map { String(describing: $0) },
                    "column_widths": queue.table.tableColumns.map(\.width),
                    "spacing": String(describing: queue.table.intercellSpacing),
                    "style": queue.table.style.rawValue
                ])
                if !fits { failed = true; break }
            }
            if failed { break }
        }
        // Use the application's real presentation owner, not a copied inspector layout.
        let layout = ScheduleMessagesView(frame: .zero)
        layout.autoresizingMask = [.width, .height]
        window.contentView = layout
        layout.table.dataSource = rows
        layout.table.delegate = rows
        layout.destinationLabel.stringValue = "ashutoshHzCode"
        layout.targetPicker.addItem(withTitle: "0 / zsh · %0 · ~/PROJECTS")
        layout.occurrencePicker.addItems(withTitles: ["Choose occurrence…", "First occurrence", "Second occurrence"])
        layout.occurrencePicker.isHidden = true
        layout.messageText.string = "Continue the test run and report any failing tests."
        layout.deadlineField.stringValue = "2026-10-23 13:45"
        layout.discardDraftButton.title = "Discard Changes"
        layout.saveButton.title = "Save Changes"
        layout.presentConnection(connected: true)
        layout.presentStatus("Remote scheduler refreshed. No local timer is active.")
        var inspectorObservations: [[String: Any]] = []
        for (style, count) in scenarios {
            layout.queueScroll.scrollerStyle = style
            rows.count = count
            for size in [NSSize(width: 1034, height: 636), NSSize(width: 940, height: 600),
                         NSSize(width: 800, height: 540), NSSize(width: 1034, height: 636)] {
                window.setContentSize(size)
                for relative in [true, false] {
                    layout.durationField.isHidden = !relative
                    layout.durationPresets.isHidden = !relative
                    layout.deadlineField.isHidden = relative
                    layout.resolvedDeadlineLabel.stringValue = relative
                        ? "Delay begins when the remote host accepts it."
                        : "2026-10-23 13:45 Europe/London (UTC+01:00)\nResolved instant: 2026-10-23T12:45:00Z"
                    layout.table.reloadData()
                    for _ in 0..<3 {
                        layout.layoutSubtreeIfNeeded()
                        layout.queueScroll.tile()
                        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
                    }
                    let buttons = [layout.refreshButton, layout.newButton, layout.inspectorButton,
                                   layout.setupButton, layout.saveButton, layout.cancelButton, layout.discardDraftButton]
                    let fits = buttons.allSatisfy { control in
                        let rect = control.convert(control.bounds, to: layout)
                        return rect.width > 0 && rect.height > 0 && layout.bounds.insetBy(dx: -1, dy: -1).contains(rect)
                    }
                    let columnsFit = layout.table.tableColumns.indices.allSatisfy {
                        let rect = layout.table.rect(ofColumn: $0)
                        return rect.maxX <= layout.table.visibleRect.maxX + 1
                    }
                    let noOverlap = !layout.queueScroll.convert(layout.queueScroll.bounds, to: layout)
                        .intersects(layout.messageText.enclosingScrollView!.convert(layout.messageText.enclosingScrollView!.bounds, to: layout))
                    let topAligned = layout.targetPicker.convert(layout.targetPicker.bounds, to: layout).minY
                        - layout.split.convert(layout.split.bounds, to: layout).minY < 120
                    let timeRect = layout.deadlineMode.convert(layout.deadlineMode.bounds, to: layout)
                    let messageRect = layout.messageText.enclosingScrollView!.convert(layout.messageText.enclosingScrollView!.bounds, to: layout)
                    let beforeTime = timeRect.minY - messageRect.maxY
                    let afterTime = layout.resolvedDeadlineLabel.convert(layout.resolvedDeadlineLabel.bounds, to: layout).minY - timeRect.maxY
                    let compactTime = (0..<65).contains(beforeTime) && (0..<65).contains(afterTime)
                    inspectorObservations.append([
                        "width": size.width, "height": size.height, "relative": relative,
                        "scroller_style": style.rawValue, "fits": fits && columnsFit && noOverlap && topAligned && compactTime,
                        "root": String(describing: layout.frame), "split": String(describing: layout.split.frame),
                        "save": String(describing: layout.saveButton.convert(layout.saveButton.bounds, to: layout)),
                        "columns_fit": columnsFit, "no_overlap": noOverlap, "top_aligned": topAligned,
                        "compact_time_controls": compactTime
                    ])
                    if !fits || !columnsFit || !noOverlap || !topAligned || !compactTime { failed = true }
                }
            }
        }
        let widthBefore = layout.split.arrangedSubviews.last!.frame.width
        layout.inspectorButton.performClick(nil)
        let collapsed = !layout.isInspectorVisible
        layout.inspectorButton.performClick(nil)
        layout.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        let widthRetained = abs(layout.split.arrangedSubviews.last!.frame.width - widthBefore) < 2
        let retained = layout.isInspectorVisible && layout.messageText.string == "Continue the test run and report any failing tests."
        layout.setupButton.performClick(nil)
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        let setupShown = layout.setupPopover.isShown && layout.setupCheckButton.window != nil
        layout.closeSetup()
        if !collapsed || !retained || !setupShown || !widthRetained { failed = true }
        var report: [String: Any] = ["source_sha": source, "state": failed ? "failed" : "passed", "observations": observations,
                                   "inspector_observations": inspectorObservations,
                                   "collapse_retains_editor": collapsed && retained, "collapse_retains_width": widthRetained,
                                   "setup_popover": setupShown]
        if let content = window.contentView,
           let bitmap = content.bitmapImageRepForCachingDisplay(in: content.bounds) {
            content.cacheDisplay(in: content.bounds, to: bitmap)
            if let png = bitmap.representation(using: .png, properties: [:]) {
                let screenshot = destination.deletingPathExtension().appendingPathExtension("png")
                do { try png.write(to: screenshot); report["screenshot"] = screenshot.lastPathComponent }
                catch { report["screenshot_error"] = String(describing: error) }
            }
        }
        do {
            let data = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
            try data.write(to: destination, options: .atomic)
            FileHandle.standardOutput.write(data)
        } catch {
            FileHandle.standardError.write(Data("Layout report failed: \(error)\n".utf8))
            failed = true
        }
        window.close()
        exit(failed ? 1 : 0)
    }
}

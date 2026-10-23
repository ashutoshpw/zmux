import AppKit
import Foundation
import Darwin

@MainActor
private final class Rows: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    func numberOfRows(in tableView: NSTableView) -> Int { 1 }
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
                "requested_width": width, "fits": fits,
                "visible": String(describing: visible), "table": String(describing: queue.table.frame),
                "columns": rects.map { String(describing: $0) },
                "column_widths": queue.table.tableColumns.map(\.width),
                "spacing": String(describing: queue.table.intercellSpacing),
                "style": queue.table.style.rawValue
            ])
            if !fits { failed = true; break }
        }
        var report: [String: Any] = ["source_sha": source, "state": failed ? "failed" : "passed", "observations": observations]
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

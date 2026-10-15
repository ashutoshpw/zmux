import AppKit

@MainActor
final class BrowserDownloadsWindowController: NSWindowController {
    private let rows = NSStackView()
    private let cancelDownload: @MainActor (BrowserDownloadRecord) -> Void
    private var presented: [BrowserDownloadRecord]?
    private var profileNames: [UUID: String] = [:]
    private var buttons: [NSButton: BrowserDownloadRecord] = [:]

    init(cancel: @escaping @MainActor (BrowserDownloadRecord) -> Void) {
        cancelDownload = cancel
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 560, height: 420),
                              styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = "Downloads"
        window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 420, height: 240)
        window.center()
        super.init(window: window)
        guard let content = window.contentView else { return }
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(scroll)
        rows.orientation = .vertical
        rows.alignment = .leading
        rows.spacing = 16
        rows.edgeInsets = NSEdgeInsets(top: 20, left: 20, bottom: 20, right: 20)
        rows.translatesAutoresizingMaskIntoConstraints = false
        scroll.documentView = rows
        NSLayoutConstraint.activate([
            scroll.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: content.topAnchor),
            scroll.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            rows.widthAnchor.constraint(equalTo: scroll.contentView.widthAnchor)
        ])
        update([])
    }
    required init?(coder: NSCoder) { nil }

    func update(_ records: [BrowserDownloadRecord], profiles: [UUID: String] = [:]) {
        guard presented != records || profileNames != profiles else { return }
        presented = records
        profileNames = profiles
        buttons.removeAll()
        rows.arrangedSubviews.forEach { rows.removeArrangedSubview($0); $0.removeFromSuperview() }
        if records.isEmpty {
            rows.addArrangedSubview(NSTextField(labelWithString: "No recent downloads."))
            return
        }
        for record in records.reversed() {
            let row = NSStackView()
            row.orientation = .horizontal
            row.alignment = .top
            row.spacing = 12
            let text = NSStackView()
            text.orientation = .vertical
            text.alignment = .leading
            text.spacing = 4
            let filename = NSTextField(labelWithString: record.filename)
            filename.font = .systemFont(ofSize: 13, weight: .medium)
            filename.lineBreakMode = .byTruncatingMiddle
            text.addArrangedSubview(filename)
            let state: String
            switch record.status {
            case "awaiting_destination": state = "Choose a destination"
            case "downloading": state = "Downloading"
            case "finalizing": state = "Saving"
            case "cancelling": state = "Cancelling"
            case "saved": state = "Saved"
            case "cancelled": state = "Cancelled"
            default: state = "Failed"
            }
            let profile = record.origin.profileId.map { profiles[$0] ?? "Browser profile" } ?? "Private browser"
            let bytes = ByteCountFormatter.string(fromByteCount: Int64(clamping: record.bytes), countStyle: .file)
            let detail = NSTextField(labelWithString: state + " · " + bytes + " · " + profile)
            detail.font = .systemFont(ofSize: 11)
            detail.textColor = .secondaryLabelColor
            text.addArrangedSubview(detail)
            if let error = record.error ?? (record.terminal && !record.cleanupComplete ? "Private staging cleanup needs attention." : nil) {
                let message = NSTextField(wrappingLabelWithString: error)
                message.textColor = .systemRed
                message.font = .systemFont(ofSize: 11)
                text.addArrangedSubview(message)
                message.widthAnchor.constraint(lessThanOrEqualToConstant: 400).isActive = true
            }
            row.addArrangedSubview(text)
            let cancel = NSButton(title: "Cancel", target: self, action: #selector(cancel(_:)))
            cancel.bezelStyle = .rounded
            cancel.setAccessibilityLabel("Cancel download " + record.filename)
            cancel.isEnabled = ["awaiting_destination", "downloading"].contains(record.status)
            buttons[cancel] = record
            row.addArrangedSubview(cancel)
            rows.addArrangedSubview(row)
            row.widthAnchor.constraint(equalTo: rows.widthAnchor, constant: -40).isActive = true
        }
    }
    @objc private func cancel(_ sender: NSButton) {
        guard let record = buttons[sender], sender.isEnabled else { return }
        cancelDownload(record)
    }
}

import AppKit

/// A status window keeps asynchronous cleanup visible without entering a modal
/// loop or offering to resume a desktop whose sessions are already stopping.
@MainActor
final class ShutdownPresentation {
    private var window: NSWindow?
    private let detail = NSTextField(wrappingLabelWithString: "")

    func show(_ message: String) {
        if window == nil {
            let host = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 520, height: 170),
                                styleMask: [.titled], backing: .buffered, defer: false)
            host.title = "zmux"
            host.isReleasedWhenClosed = false
            let title = NSTextField(labelWithString: "Closing sessions")
            title.font = .systemFont(ofSize: 17, weight: .semibold)
            let progress = NSProgressIndicator()
            progress.style = .spinning
            progress.startAnimation(nil)
            let heading = NSStackView(views: [progress, title])
            heading.spacing = 12
            detail.font = .systemFont(ofSize: 12)
            detail.textColor = .secondaryLabelColor
            detail.maximumNumberOfLines = 5
            let stack = NSStackView(views: [heading, detail])
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 16
            stack.translatesAutoresizingMaskIntoConstraints = false
            if let content = host.contentView {
                content.addSubview(stack)
                NSLayoutConstraint.activate([
                    stack.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 24),
                    stack.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -24),
                    stack.topAnchor.constraint(equalTo: content.topAnchor, constant: 24)
                ])
            }
            host.center()
            window = host
        }
        detail.stringValue = message
        if window?.isVisible != true { window?.orderFront(nil) }
    }

    func close() {
        window?.close()
        window = nil
    }
}

import AppKit

/// Saved output has no terminal session and never sends input or allocates a PTY.
@MainActor
final class CompletedTerminalHistoryView: BackgroundView {
    private let output = CompletedOutputTextView(frame: .zero)
    let completionDigest: String

    var outputResponder: NSResponder { output }
    func updateFocus(_ action: @escaping () -> Void) { output.onFocus = action }

    init(history: CompletedLocalZellijHistory, onFocus: @escaping () -> Void) {
        completionDigest = history.receipt.bindingDigest
        super.init(color: ZmuxPalette.terminal)
        let status = NSTextField(labelWithString: history.receipt.workloadExit.label)
        status.font = .systemFont(ofSize: 12, weight: .medium)
        status.textColor = .secondaryLabelColor
        let detail = NSTextField(labelWithString: history.viewport == nil
            ? "No saved output is available."
            : (history.viewport?.truncated == true ? "Saved output · truncated" : "Saved output"))
        detail.font = .systemFont(ofSize: 11)
        detail.textColor = .secondaryLabelColor
        let heading = NSStackView(views: [status, detail])
        heading.orientation = .vertical
        heading.alignment = .leading
        heading.spacing = 4

        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = true
        scroll.drawsBackground = false
        let text = output
        text.onFocus = onFocus
        text.isEditable = false
        text.isSelectable = true
        text.isRichText = false
        text.font = .monospacedSystemFont(ofSize: 13, weight: .regular)
        text.textColor = .labelColor
        text.drawsBackground = false
        text.textContainerInset = NSSize(width: 12, height: 10)
        text.isVerticallyResizable = true
        text.isHorizontallyResizable = true
        text.minSize = .zero
        text.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        text.textContainer?.containerSize = text.maxSize
        text.textContainer?.widthTracksTextView = false
        text.string = history.viewport?.contents ?? ""
        text.setAccessibilityLabel("Saved terminal output")
        text.sizeToFit()
        scroll.documentView = text

        for child in [heading, scroll] {
            child.translatesAutoresizingMaskIntoConstraints = false
            addSubview(child)
        }
        NSLayoutConstraint.activate([
            heading.topAnchor.constraint(equalTo: topAnchor, constant: 12),
            heading.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            heading.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -12),
            scroll.topAnchor.constraint(equalTo: heading.bottomAnchor, constant: 8),
            scroll.leadingAnchor.constraint(equalTo: leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: bottomAnchor)
        ])
    }

    required init?(coder: NSCoder) { nil }
}

@MainActor
private final class CompletedOutputTextView: NSTextView {
    var onFocus: (() -> Void)?
    override func becomeFirstResponder() -> Bool {
        let accepted = super.becomeFirstResponder()
        if accepted { onFocus?() }
        return accepted
    }
}

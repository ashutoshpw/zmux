import AppKit

/// One native viewer for one exact host session. All relay calls, including frame reads,
/// pass through a bounded FIFO; an uncertain mutation permanently makes this window read-only.
@MainActor
final class CloudTerminalViewerWindowController: NSWindowController, NSWindowDelegate {
    private enum Operation {
        case frame
        case input(String)
        case key(TerminalKeyEvent)
        case paste(String)
        case resize(Int, Int)
        case scroll(Int)

        var mutatesTerminal: Bool {
            if case .frame = self { return false }
            return true
        }
    }

    private static let operationCapacity = 32
    private static let maximumInputOperationBytes = 4_096
    private static let maximumPasteOperationBytes = 16_384
    private let session: CloudTerminalSessionDescriptor
    private let host: CloudTerminalHost
    private let onClose: @MainActor () -> Void
    private let terminal = TerminalView()
    private let status = NSTextField(wrappingLabelWithString: "Connecting…")
    private let reconnectButton = NSButton(title: "Refresh", target: nil, action: nil)
    private var pending: [Operation] = []
    private var pumping = false
    private var framePending = false
    private var readOnly = false
    private var detached = false
    private var pollTask: Task<Void, Never>?

    init(session: CloudTerminalSessionDescriptor, host: CloudTerminalHost,
         onClose: @escaping @MainActor () -> Void) {
        self.session = session
        self.host = host
        self.onClose = onClose
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 960, height: 640),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable],
                              backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let title = session.title.isEmpty ? "Terminal \(session.scope.sessionId)" : session.title
        window.title = "\(session.device.name) — \(title)"
        window.center()
        super.init(window: window)
        window.delegate = self
        buildContent()
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self, !self.detached else { return }
                self.enqueueFrame()
                do { try await Task.sleep(for: .milliseconds(500)) }
                catch { return }
            }
        }
    }

    required init?(coder: NSCoder) { nil }

    func windowWillClose(_ notification: Notification) {
        detached = true
        pollTask?.cancel(); pollTask = nil
        pending.removeAll(keepingCapacity: false)
        onClose()
    }

    private func buildContent() {
        guard let content = window?.contentView else { return }
        content.wantsLayer = true
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            stack.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),
            stack.topAnchor.constraint(equalTo: content.topAnchor, constant: 10),
            stack.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -10)
        ])

        let toolbar = NSStackView()
        toolbar.orientation = .horizontal
        toolbar.alignment = .centerY
        toolbar.spacing = 10
        status.font = .systemFont(ofSize: 12)
        status.textColor = .secondaryLabelColor
        toolbar.addArrangedSubview(status)
        reconnectButton.bezelStyle = .rounded
        reconnectButton.target = self
        reconnectButton.action = #selector(refreshFrame)
        reconnectButton.toolTip = "Refresh the remote terminal. An uncertain input outcome keeps this viewer read-only."
        updateReconnectButton()
        toolbar.addArrangedSubview(reconnectButton)
        stack.addArrangedSubview(toolbar)
        toolbar.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true

        terminal.translatesAutoresizingMaskIntoConstraints = false
        terminal.localPointerSelectionOnly = true
        terminal.onInput = { [weak self] text in self?.enqueue(.input(text)) }
        terminal.onKey = { [weak self] event in self?.enqueue(.key(event)) }
        terminal.onPaste = { [weak self] text in self?.enqueue(.paste(text)) }
        terminal.onPasteError = { [weak self] message in self?.status.stringValue = message }
        terminal.onResize = { [weak self] rows, cols in self?.enqueue(.resize(rows, cols)) }
        terminal.onScroll = { [weak self] lines in self?.enqueue(.scroll(lines)) }
        terminal.onWheel = { [weak self] lines, _, _, _, _ in self?.enqueue(.scroll(lines)) }
        stack.addArrangedSubview(terminal)
        NSLayoutConstraint.activate([
            terminal.widthAnchor.constraint(equalTo: stack.widthAnchor),
            terminal.heightAnchor.constraint(greaterThanOrEqualToConstant: 160)
        ])
        terminal.setContentHuggingPriority(.defaultLow, for: .vertical)
        terminal.setContentCompressionResistancePriority(.defaultLow, for: .vertical)
    }

    @objc private func refreshFrame() { enqueueFrame() }

    private func enqueueFrame() {
        guard !detached, !framePending else { return }
        guard pending.count + (pumping ? 1 : 0) < Self.operationCapacity else { return }
        framePending = true
        pending.append(.frame)
        pump()
    }

    private func enqueue(_ operation: Operation) {
        guard !detached else { return }
        guard operation.mutatesTerminal else { enqueueFrame(); return }
        guard !readOnly else {
            status.stringValue = "Read-only after an uncertain terminal change. Refresh output to inspect the session; input was not replayed."
            return
        }
        if case .input(let text) = operation {
            guard text.utf8.count <= Self.maximumInputOperationBytes else {
                status.stringValue = "Remote terminal input exceeded the 4096-byte operation limit and was not queued."
                return
            }
            if case .input(let previous)? = pending.last,
               previous.utf8.count + text.utf8.count <= Self.maximumInputOperationBytes {
                pending[pending.count - 1] = .input(previous + text)
                return
            }
        }
        if case .paste(let text) = operation {
            guard (1...Self.maximumPasteOperationBytes).contains(text.utf8.count) else {
                status.stringValue = "Remote terminal paste must contain 1–16384 UTF-8 bytes and was not queued."
                return
            }
        }
        if case .resize = operation, let last = pending.last, case .resize = last {
            pending[pending.count - 1] = operation
            return
        }
        guard pending.count + (pumping ? 1 : 0) < Self.operationCapacity else {
            status.stringValue = "Remote terminal queue is full; this operation was not queued."
            return
        }
        pending.append(operation)
        pump()
    }

    private func pump() {
        guard !pumping, !detached, !pending.isEmpty else { return }
        let operation = pending.removeFirst()
        pumping = true
        Task { [weak self] in
            guard let self else { return }
            do {
                switch operation {
                case .frame:
                    let frame = try await self.host.readFrame(for: self.session)
                    self.terminal.update(frame)
                    self.status.stringValue = self.readOnly
                        ? "Read-only · refresh shows output; terminal input is disabled after an uncertain change."
                        : "Connected · terminal input is enabled"
                case .input(let text): try await self.host.sendInput(text, to: self.session)
                case .key(let event): try await self.host.sendKey(event, to: self.session)
                case .paste(let text): try await self.host.sendPaste(text, to: self.session)
                case .resize(let rows, let cols): try await self.host.sendResize(rows: rows, cols: cols, to: self.session)
                case .scroll(let lines): try await self.host.sendScroll(lines, to: self.session)
                }
            } catch {
                if operation.mutatesTerminal {
                    let outcomeUnknown = (error as? CloudTerminalRPCFailure)?.outcomeUnknown
                        ?? (error as? CloudFailure)?.outcomeUnknown
                        ?? true
                    self.readOnly = outcomeUnknown
                    self.updateReconnectButton()
                    self.pending.removeAll { $0.mutatesTerminal }
                    if outcomeUnknown {
                        self.status.stringValue = "Read-only after a terminal change could not be confirmed. The operation was not retried. Refresh output and inspect the session before continuing.\n\(error.localizedDescription)"
                    } else {
                        self.status.stringValue = "The terminal operation was rejected and not retried. Queued input was cleared.\n\(error.localizedDescription)"
                    }
                } else {
                    self.status.stringValue = "Unable to refresh terminal output; trying again: \(error.localizedDescription)"
                }
            }
            if case .frame = operation { self.framePending = false }
            self.pumping = false
            self.pump()
        }
    }

    private func updateReconnectButton() {
        reconnectButton.title = readOnly ? "Refresh (Read-Only)" : "Refresh"
        reconnectButton.setAccessibilityLabel(readOnly ? "Refresh terminal output in read-only mode" : "Refresh terminal output")
    }
}

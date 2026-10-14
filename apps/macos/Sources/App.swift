import AppKit
import Darwin

private enum UICommand: Sendable {
    case action(String, UInt64?, String?, String?, UInt64?)
    case input(UInt64, String)
    case resize(UInt64, Int, Int)
}

@MainActor
final class AppController: NSObject {
    let window: NSWindow
    var cloudWindow: NSWindowController?
    private(set) var snapshot: DesktopSnapshot?

    private let bridge = NativeBridge()
    private let canvas = BackgroundView(color: ZmuxPalette.terminal)
    private let workspaceList = FlippedStackView()
    private let mainHeader = MainHeaderView()
    private var statusMessage = "Starting shell…"
    private lazy var cloudFooter = CloudFooterView(target: self, action: #selector(showCloudSettings))
    private var actionButtons: [IconButton] = []
    private let stream: AsyncStream<UICommand>
    private let continuation: AsyncStream<UICommand>.Continuation
    private var commandTask: Task<Void, Never>?
    private var pollingTask: Task<Void, Never>?
    private var layoutKey = ""
    private var modelRevision: UInt64 = .max
    private var terminals: [UInt64: TerminalView] = [:]
    private var workspaceRows: [WorkspaceRowView: UInt64] = [:]
    private var workspaceCount = 1
    private var didSetInitialTerminalFocus = false

    override init() {
        (stream, continuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingOldest(256))
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1120, height: 720),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        super.init()
        window.title = "Terminal"
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.titlebarSeparatorStyle = .none
        window.toolbarStyle = .unifiedCompact
        window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 680, height: 420)
        window.appearance = NSAppearance(named: .darkAqua)
        window.backgroundColor = ZmuxPalette.window
        window.center()
        buildLayout()
        buildMenus()
    }

    func start() async throws {
        try await bridge.start()
        commandTask = Task { [weak self, stream] in
            for await command in stream {
                guard let self, !Task.isCancelled else { break }
                do {
                    switch command {
                    case .action(let action, let pane, let axis, let title, let workspace):
                        try await self.bridge.action(action, pane: pane, axis: axis, title: title, workspace: workspace)
                    case .input(let session, let text):
                        try await self.bridge.input(session: session, text: text)
                    case .resize(let session, let rows, let cols):
                        try await self.bridge.resize(session: session, rows: rows, cols: cols)
                    }
                } catch {
                    self.showError(error.localizedDescription)
                }
            }
        }
        pollingTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { break }
                do {
                    if let snapshot = try await self.bridge.poll() { self.present(snapshot) }
                    try await Task.sleep(for: .milliseconds(32))
                } catch is CancellationError {
                    break
                } catch {
                    self.showError(error.localizedDescription)
                    do { try await Task.sleep(for: .milliseconds(250)) } catch { break }
                }
            }
        }
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
    }

    func shutdown() async throws {
        continuation.finish()
        commandTask?.cancel()
        pollingTask?.cancel()
        try await bridge.close()
    }

    func updateCloudStatus(_ label: String, connected: Bool) {
        cloudFooter.update(label: label, connected: connected)
    }

    private func enqueue(_ command: UICommand) {
        if case .dropped = continuation.yield(command) {
            showError("Command queue is full; the new command was not accepted")
        }
    }

    private func showError(_ message: String) {
        statusMessage = message
        mainHeader.update(title: mainHeader.titleLabel.stringValue, detail: message, error: true)
    }

    private func buildLayout() {
        let content = BackgroundView(color: ZmuxPalette.window)
        window.contentView = content

        let sidebarPanel = BackgroundView(color: ZmuxPalette.sidebar)
        let sidebarChrome = DraggableChromeView(color: ZmuxPalette.sidebar)
        let sidebarSeparator = SeparatorView()
        let rightSeparator = SeparatorView()
        let localHeading = NSTextField(labelWithString: "Local")
        localHeading.font = .systemFont(ofSize: 11, weight: .semibold)
        localHeading.textColor = ZmuxPalette.subdued

        let newWorkspaceButton = makeIconButton(symbol: "plus", label: "New workspace", action: #selector(newWorkspace))
        let cloudButton = makeIconButton(symbol: "icloud", label: "Cloud account", action: #selector(showCloudSettings))
        let sidebarActions = NSStackView(views: [newWorkspaceButton, cloudButton])
        sidebarActions.orientation = .horizontal
        sidebarActions.spacing = 3

        let scroll = NSScrollView()
        scroll.drawsBackground = false
        scroll.borderType = .noBorder
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = false
        scroll.autohidesScrollers = true
        workspaceList.translatesAutoresizingMaskIntoConstraints = false
        scroll.documentView = workspaceList
        workspaceList.orientation = .vertical
        workspaceList.alignment = .leading
        workspaceList.spacing = 4
        workspaceList.edgeInsets = NSEdgeInsets(top: 0, left: 8, bottom: 8, right: 8)

        let splitRightButton = makeIconButton(symbol: "rectangle.split.2x1", label: "Split right", action: #selector(splitRight))
        let splitDownButton = makeIconButton(symbol: "rectangle.split.1x2", label: "Split down", action: #selector(splitDown))
        let closePaneButton = makeIconButton(symbol: "xmark", label: "Close pane", action: #selector(closePane))
        for button in [splitRightButton, splitDownButton, closePaneButton] {
            mainHeader.actions.addArrangedSubview(button)
        }

        for view in [sidebarPanel, sidebarChrome, sidebarSeparator, rightSeparator, localHeading, sidebarActions, scroll, cloudFooter, mainHeader, canvas] {
            view.translatesAutoresizingMaskIntoConstraints = false
        }
        content.addSubview(sidebarPanel)
        sidebarPanel.addSubview(sidebarChrome)
        sidebarPanel.addSubview(localHeading)
        sidebarPanel.addSubview(sidebarActions)
        sidebarPanel.addSubview(scroll)
        sidebarPanel.addSubview(cloudFooter)
        sidebarPanel.addSubview(rightSeparator)
        sidebarChrome.addSubview(sidebarSeparator)
        content.addSubview(mainHeader)
        content.addSubview(canvas)

        for button in actionButtons {
            button.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                button.widthAnchor.constraint(equalToConstant: 28),
                button.heightAnchor.constraint(equalToConstant: 28)
            ])
        }

        NSLayoutConstraint.activate([
            sidebarPanel.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            sidebarPanel.topAnchor.constraint(equalTo: content.topAnchor),
            sidebarPanel.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            sidebarPanel.widthAnchor.constraint(equalToConstant: 224),

            sidebarChrome.leadingAnchor.constraint(equalTo: sidebarPanel.leadingAnchor),
            sidebarChrome.trailingAnchor.constraint(equalTo: sidebarPanel.trailingAnchor),
            sidebarChrome.topAnchor.constraint(equalTo: content.safeAreaLayoutGuide.topAnchor),
            sidebarChrome.heightAnchor.constraint(equalToConstant: 42),
            sidebarSeparator.leadingAnchor.constraint(equalTo: sidebarChrome.leadingAnchor),
            sidebarSeparator.trailingAnchor.constraint(equalTo: sidebarChrome.trailingAnchor),
            sidebarSeparator.bottomAnchor.constraint(equalTo: sidebarChrome.bottomAnchor),
            sidebarSeparator.heightAnchor.constraint(equalToConstant: 1),

            sidebarActions.trailingAnchor.constraint(equalTo: sidebarChrome.trailingAnchor, constant: -8),
            sidebarActions.centerYAnchor.constraint(equalTo: sidebarChrome.centerYAnchor),
            localHeading.leadingAnchor.constraint(equalTo: sidebarPanel.leadingAnchor, constant: 14),
            localHeading.topAnchor.constraint(equalTo: sidebarChrome.bottomAnchor, constant: 12),
            localHeading.trailingAnchor.constraint(lessThanOrEqualTo: sidebarPanel.trailingAnchor, constant: -12),

            scroll.leadingAnchor.constraint(equalTo: sidebarPanel.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: sidebarPanel.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: localHeading.bottomAnchor, constant: 7),
            scroll.bottomAnchor.constraint(equalTo: cloudFooter.topAnchor),
            workspaceList.leadingAnchor.constraint(equalTo: scroll.contentView.leadingAnchor),
            workspaceList.topAnchor.constraint(equalTo: scroll.contentView.topAnchor),
            workspaceList.widthAnchor.constraint(equalTo: scroll.contentView.widthAnchor),

            cloudFooter.leadingAnchor.constraint(equalTo: sidebarPanel.leadingAnchor),
            cloudFooter.trailingAnchor.constraint(equalTo: sidebarPanel.trailingAnchor),
            cloudFooter.bottomAnchor.constraint(equalTo: sidebarPanel.bottomAnchor),
            cloudFooter.heightAnchor.constraint(equalToConstant: 54),
            rightSeparator.trailingAnchor.constraint(equalTo: sidebarPanel.trailingAnchor),
            rightSeparator.topAnchor.constraint(equalTo: sidebarPanel.topAnchor),
            rightSeparator.bottomAnchor.constraint(equalTo: sidebarPanel.bottomAnchor),
            rightSeparator.widthAnchor.constraint(equalToConstant: 1),

            mainHeader.leadingAnchor.constraint(equalTo: sidebarPanel.trailingAnchor),
            mainHeader.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            mainHeader.topAnchor.constraint(equalTo: content.safeAreaLayoutGuide.topAnchor),
            mainHeader.heightAnchor.constraint(equalToConstant: 42),
            canvas.leadingAnchor.constraint(equalTo: sidebarPanel.trailingAnchor),
            canvas.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            canvas.topAnchor.constraint(equalTo: mainHeader.bottomAnchor),
            canvas.bottomAnchor.constraint(equalTo: content.bottomAnchor)
        ])
    }

    private func makeIconButton(symbol: String, label: String, action: Selector) -> IconButton {
        let button = IconButton(symbol: symbol, label: label, target: self, action: action)
        actionButtons.append(button)
        return button
    }

    private func buildMenus() {
        let menu = NSMenu()
        let appItem = NSMenuItem()
        menu.addItem(appItem)
        let appMenu = NSMenu(title: "zmux")
        let installItem = appMenu.addItem(withTitle: "Install Command Line Tool…", action: #selector(installCommandLineTool(_:)), keyEquivalent: "")
        installItem.target = self
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Quit zmux", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu

        let workspaceItem = NSMenuItem(title: "Workspace", action: nil, keyEquivalent: "")
        menu.addItem(workspaceItem)
        let workspaceMenu = NSMenu(title: "Workspace")
        let items: [(String, Selector, String)] = [
            ("New Workspace", #selector(newWorkspace), "n"),
            ("Split Right", #selector(splitRight), "d"),
            ("Split Down", #selector(splitDown), "D"),
            ("Rename Workspace", #selector(renameWorkspace), ""),
            ("Close Pane", #selector(closePane), "w")
        ]
        for (title, action, key) in items {
            let item = workspaceMenu.addItem(withTitle: title, action: action, keyEquivalent: key)
            item.target = self
        }
        workspaceItem.submenu = workspaceMenu

        let editItem = NSMenuItem(title: "Edit", action: nil, keyEquivalent: "")
        menu.addItem(editItem)
        let edit = NSMenu(title: "Edit")
        for (title, action, key) in [
            ("Copy", #selector(copySelection), "c"),
            ("Paste", #selector(pasteText), "v"),
            ("Select All", #selector(selectAllText), "a")
        ] {
            let item = edit.addItem(withTitle: title, action: action, keyEquivalent: key)
            item.target = self
        }
        editItem.submenu = edit
        NSApp.mainMenu = menu
    }

    private func present(_ snapshot: DesktopSnapshot) {
        self.snapshot = snapshot
        statusMessage = snapshot.lastError ?? "Ready"
        rebuildWorkspaceRowsIfNeeded(snapshot)
        updateWorkspaceRows(snapshot)
        updateHeader(snapshot)
        updateTerminalLayoutIfNeeded(snapshot)

        for frame in snapshot.frames { terminals[frame.sessionId]?.update(frame) }
        if let pane = snapshot.workspace?.root.panes.first(where: { $0.id == snapshot.workspace?.focusedPane }),
           let terminal = terminals[pane.sessionId] {
            if !didSetInitialTerminalFocus {
                window.makeFirstResponder(terminal)
                didSetInitialTerminalFocus = true
            } else if let responder = window.firstResponder as? TerminalView, responder !== terminal {
                window.makeFirstResponder(terminal)
            }
        }
    }

    private func rebuildWorkspaceRowsIfNeeded(_ snapshot: DesktopSnapshot) {
        guard modelRevision != snapshot.model.revision else { return }
        modelRevision = snapshot.model.revision
        workspaceList.arrangedSubviews.forEach {
            workspaceList.removeArrangedSubview($0)
            $0.removeFromSuperview()
        }
        workspaceRows.removeAll()
        for workspace in snapshot.model.workspaces {
            let row = WorkspaceRowView(workspaceID: workspace.id)
            row.target = self
            row.action = #selector(workspaceSelected(_:))
            row.menu = workspaceMenu(for: workspace)
            row.translatesAutoresizingMaskIntoConstraints = false
            workspaceList.addArrangedSubview(row)
            row.widthAnchor.constraint(equalTo: workspaceList.widthAnchor, constant: -16).isActive = true
            workspaceRows[row] = workspace.id
        }
        window.recalculateKeyViewLoop()
    }

    private func updateWorkspaceRows(_ snapshot: DesktopSnapshot) {
        for (row, workspaceID) in workspaceRows {
            guard let workspace = snapshot.model.workspaces.first(where: { $0.id == workspaceID }) else { continue }
            let isSelected = workspace.id == snapshot.model.focusedWorkspace
            let frames = isSelected ? snapshot.frames : []
            let isRunning = frames.contains { $0.snapshot?.state == "running" }
            let subtitle: String
            if isSelected, let workingDirectory = focusedViewport(in: snapshot)?.workingDirectory, !workingDirectory.isEmpty {
                subtitle = (workingDirectory as NSString).abbreviatingWithTildeInPath
            } else {
                let panes = workspace.root.panes.count
                subtitle = panes == 1 ? "Local shell" : "\(panes) local terminals"
            }
            row.update(title: workspace.name, subtitle: subtitle, isSelected: isSelected, isRunning: isRunning)
        }
    }

    private func updateHeader(_ snapshot: DesktopSnapshot) {
        let title = terminalTitle(in: snapshot)
        let detail: String
        let isError = snapshot.lastError != nil
        if let error = snapshot.lastError {
            detail = error
        } else if let workingDirectory = focusedViewport(in: snapshot)?.workingDirectory, !workingDirectory.isEmpty {
            detail = (workingDirectory as NSString).abbreviatingWithTildeInPath
        } else if let workspace = snapshot.workspace {
            let count = workspace.root.panes.count
            detail = count == 1 ? "Local shell" : "\(count) local terminals"
        } else {
            detail = "No terminal"
        }
        mainHeader.update(title: title, detail: detail, error: isError)
        window.title = title
    }

    private func terminalTitle(in snapshot: DesktopSnapshot) -> String {
        if let title = focusedViewport(in: snapshot)?.title?.trimmingCharacters(in: .whitespacesAndNewlines), !title.isEmpty {
            return title
        }
        if let name = snapshot.workspace?.name.trimmingCharacters(in: .whitespacesAndNewlines), !name.isEmpty {
            return name
        }
        if let shell = ProcessInfo.processInfo.environment["SHELL"] {
            let name = URL(fileURLWithPath: shell).lastPathComponent
            if !name.isEmpty { return name }
        }
        return "Shell"
    }

    private func focusedViewport(in snapshot: DesktopSnapshot) -> Viewport? {
        guard let workspace = snapshot.workspace,
              let pane = workspace.root.panes.first(where: { $0.id == workspace.focusedPane }) else { return nil }
        return snapshot.frames.first(where: { $0.sessionId == pane.sessionId })?.snapshot?.viewport
    }

    private func updateTerminalLayoutIfNeeded(_ snapshot: DesktopSnapshot) {
        let key = snapshot.workspace?.root.key ?? "empty"
        guard key != layoutKey else { return }
        layoutKey = key
        didSetInitialTerminalFocus = false
        canvas.subviews.forEach { $0.removeFromSuperview() }
        terminals.removeAll()
        if let workspace = snapshot.workspace {
            let root = paneView(workspace.root)
            root.frame = canvas.bounds
            root.autoresizingMask = [.width, .height]
            canvas.addSubview(root)
            initializeSplits(root)
        } else {
            let empty = NSStackView()
            empty.orientation = .vertical
            empty.alignment = .centerX
            empty.spacing = 8
            let icon = NSImageView(image: NSImage(systemSymbolName: "terminal", accessibilityDescription: nil) ?? NSImage())
            icon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 24, weight: .light)
            icon.contentTintColor = ZmuxPalette.subdued
            let label = NSTextField(labelWithString: "Create a workspace to open a shell")
            label.font = .systemFont(ofSize: 13)
            label.textColor = ZmuxPalette.subdued
            empty.addArrangedSubview(icon)
            empty.addArrangedSubview(label)
            empty.translatesAutoresizingMaskIntoConstraints = false
            canvas.addSubview(empty)
            NSLayoutConstraint.activate([
                empty.centerXAnchor.constraint(equalTo: canvas.centerXAnchor),
                empty.centerYAnchor.constraint(equalTo: canvas.centerYAnchor)
            ])
        }
        canvas.layoutSubtreeIfNeeded()
    }

    private func paneView(_ tree: PaneTree) -> NSView {
        switch tree {
        case .leaf(let pane):
            let view = TerminalView(pane: pane)
            view.onInput = { [weak self] text in self?.enqueue(.input(pane.sessionId, text)) }
            view.onResize = { [weak self] rows, cols in self?.enqueue(.resize(pane.sessionId, rows, cols)) }
            view.onFocus = { [weak self] in self?.enqueue(.action("focus_pane", pane.id, nil, nil, nil)) }
            terminals[pane.sessionId] = view
            return view
        case .split(let axis, let first, let second):
            let split = NSSplitView()
            split.isVertical = axis == "horizontal"
            split.dividerStyle = .thin
            split.addArrangedSubview(paneView(first))
            split.addArrangedSubview(paneView(second))
            split.adjustSubviews()
            return split
        }
    }

    private func initializeSplits(_ view: NSView) {
        if let split = view as? NSSplitView {
            split.adjustSubviews()
            split.setPosition((split.isVertical ? split.bounds.width : split.bounds.height) / 2, ofDividerAt: 0)
            split.subviews.forEach(initializeSplits)
        }
    }

    private func workspaceMenu(for workspace: Workspace) -> NSMenu {
        let menu = NSMenu()
        let rename = menu.addItem(withTitle: "Rename Workspace…", action: #selector(renameWorkspaceFromMenu(_:)), keyEquivalent: "")
        rename.target = self
        rename.representedObject = NSNumber(value: workspace.id)
        let close = menu.addItem(withTitle: "Close Workspace", action: #selector(closeWorkspaceFromMenu(_:)), keyEquivalent: "")
        close.target = self
        close.representedObject = NSNumber(value: workspace.id)
        return menu
    }

    @objc func newWorkspace() {
        workspaceCount += 1
        enqueue(.action("create_workspace", nil, nil, "Workspace \(workspaceCount)", nil))
    }

    @objc func splitRight() { split("horizontal") }
    @objc func splitDown() { split("vertical") }

    private func split(_ axis: String) {
        if let pane = snapshot?.workspace?.focusedPane {
            enqueue(.action("split_pane", pane, axis, nil, nil))
        }
    }

    @objc func closePane() {
        if let pane = snapshot?.workspace?.focusedPane {
            enqueue(.action("close_pane", pane, nil, nil, nil))
        }
    }

    @objc func workspaceSelected(_ sender: WorkspaceRowView) {
        if let id = workspaceRows[sender] { selectWorkspace(id) }
    }

    func selectWorkspace(_ id: UInt64) {
        if let workspace = snapshot?.model.workspaces.first(where: { $0.id == id }) {
            enqueue(.action("focus_pane", workspace.focusedPane, nil, nil, nil))
        }
    }

    @objc func renameWorkspace() {
        guard let id = snapshot?.workspace?.id else { return }
        promptToRename(workspaceID: id)
    }

    @objc private func renameWorkspaceFromMenu(_ sender: NSMenuItem) {
        guard let number = sender.representedObject as? NSNumber else { return }
        promptToRename(workspaceID: number.uint64Value)
    }

    private func promptToRename(workspaceID: UInt64) {
        guard let workspace = snapshot?.model.workspaces.first(where: { $0.id == workspaceID }) else { return }
        let alert = NSAlert()
        alert.messageText = "Rename workspace"
        alert.informativeText = "Choose a short name for this terminal workspace."
        let field = NSTextField(string: workspace.name)
        field.frame = NSRect(x: 0, y: 0, width: 280, height: 24)
        alert.accessoryView = field
        alert.addButton(withTitle: "Rename")
        alert.addButton(withTitle: "Cancel")
        if alert.runModal() == .alertFirstButtonReturn {
            enqueue(.action("rename_workspace", nil, nil, field.stringValue, workspace.id))
        }
    }

    @objc private func closeWorkspaceFromMenu(_ sender: NSMenuItem) {
        guard let number = sender.representedObject as? NSNumber,
              let workspace = snapshot?.model.workspaces.first(where: { $0.id == number.uint64Value }) else { return }
        for pane in workspace.root.panes {
            enqueue(.action("close_pane", pane.id, nil, nil, nil))
        }
    }

    @objc func showCloudSettings() {
        let controller = cloudWindow ?? CloudWindowController(owner: self)
        cloudWindow = controller
        controller.showWindow(nil)
        controller.window?.makeKeyAndOrderFront(nil)
        NSApp.activate()
    }

    private var focusedTerminal: TerminalView? {
        guard let snapshot,
              let pane = snapshot.workspace?.root.panes.first(where: { $0.id == snapshot.workspace?.focusedPane }) else { return nil }
        return terminals[pane.sessionId]
    }

    @objc func copySelection(_ sender: Any?) {
        if let terminal = window.firstResponder as? TerminalView {
            terminal.copySelection(sender)
        } else {
            NSApp.sendAction(NSSelectorFromString("copy:"), to: nil, from: sender)
        }
    }

    @objc func pasteText(_ sender: Any?) {
        if let terminal = window.firstResponder as? TerminalView {
            terminal.pasteFromClipboard(sender)
        } else {
            NSApp.sendAction(NSSelectorFromString("paste:"), to: nil, from: sender)
        }
    }

    @objc func selectAllText(_ sender: Any?) {
        if let terminal = window.firstResponder as? TerminalView {
            terminal.selectViewport(sender)
        } else {
            NSApp.sendAction(NSSelectorFromString("selectAll:"), to: nil, from: sender)
        }
    }

    func smokeTest(report: URL) async throws {
        try AppStartup.validateCloudCallbackContract()
        try await waitUntil { $0.frames.count == 1 && $0.frames.allSatisfy { $0.snapshot?.state == "running" } }
        guard let initialSnapshot = snapshot, let first = initialSnapshot.workspace else {
            throw BridgeFailure(message: "No initial workspace")
        }
        let expectedControls = Set(["New workspace", "Cloud account", "Split right", "Split down", "Close pane", "Cloud settings"])
        let accessibleControls = Set((actionButtons + [cloudFooter.settingsButton]).compactMap { $0.accessibilityLabel() })
        guard expectedControls.isSubset(of: accessibleControls) else {
            throw BridgeFailure(message: "Native icon controls are missing accessibility labels")
        }

        focusedTerminal?.insertText("printf '\\033[31mZMUX_NATIVE_SMOKE\\033[0m\\n'\r", replacementRange: NSRange(location: NSNotFound, length: 0))
        try await waitUntil { snapshot in
            snapshot.frames.contains { frame in
                frame.snapshot?.viewport.lines.contains { row in
                    row.contains { $0.text.contains("ZMUX_NATIVE_SMOKE") && $0.style.foreground.index == 1 }
                } == true
            }
        }
        focusedTerminal?.insertText("printf '\\033]2;build workspace\\007'\r", replacementRange: NSRange(location: NSNotFound, length: 0))
        try await waitUntil { $0.frames.first?.snapshot?.viewport.title == "build workspace" }
        guard window.title == "build workspace", mainHeader.titleLabel.stringValue == "build workspace" else {
            throw BridgeFailure(message: "PTY title was not reflected in native window chrome")
        }
        let cliURL = Bundle.main.resourceURL!.appendingPathComponent("bin/zmux")
        let cliData = try await Task.detached {
            let process = Process()
            process.executableURL = cliURL
            process.arguments = ["status"]
            let output = Pipe()
            process.standardOutput = output
            try process.run()
            let data = output.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            guard process.terminationStatus == 0 else { throw BridgeFailure(message: "Bundled CLI could not control the native app") }
            return data
        }.value
        guard let cliStatus = try JSONSerialization.jsonObject(with: cliData) as? [String: Any],
              cliStatus["workspace_count"] as? Int == 1,
              cliStatus["running_sessions"] as? Int == 1 else {
            throw BridgeFailure(message: "Bundled CLI did not observe the native terminal")
        }
        splitRight()
        try await waitUntil { $0.workspace?.root.panes.count == 2 && $0.frames.count == 2 && $0.frames.allSatisfy { $0.snapshot?.state == "running" } }
        window.setContentSize(NSSize(width: 1250, height: 820))
        window.contentView?.layoutSubtreeIfNeeded()
        try await waitUntil { [weak self] snapshot in
            guard let terminal = self?.focusedTerminal,
                  let frame = snapshot.frames.first(where: { $0.sessionId == terminal.pane.sessionId }),
                  let size = frame.snapshot?.viewport.size else { return false }
            return size.rows > 1 && size.cols > 1 && terminal.requestedSize == (size.rows, size.cols)
        }
        guard let size = focusedTerminal?.requestedSize else { throw BridgeFailure(message: "No resized native terminal") }
        focusedTerminal?.insertText("stty size; printf '\\nZMUX_SPLIT_OK\\n'\r", replacementRange: NSRange(location: NSNotFound, length: 0))
        try await waitUntil { snapshot in
            snapshot.frames.contains { frame in
                guard let contents = frame.snapshot?.viewport.contents else { return false }
                return contents.components(separatedBy: "\n").contains("\(size.0) \(size.1)")
                    && contents.components(separatedBy: "\n").contains("ZMUX_SPLIT_OK")
            }
        }
        newWorkspace()
        try await waitUntil { $0.model.workspaces.count == 2 && $0.frames.count == 1 && $0.frames.first?.snapshot?.state == "running" }
        selectWorkspace(first.id)
        try await waitUntil { $0.model.focusedWorkspace == first.id && $0.frames.count == 2 }
        closePane()
        try await waitUntil { $0.workspace?.root.panes.count == 1 && $0.frames.count == 1 }
        guard let view = window.contentView,
              let image = view.bitmapImageRepForCachingDisplay(in: view.bounds) else {
            throw BridgeFailure(message: "Native drawing surface unavailable")
        }
        view.displayIfNeeded()
        view.effectiveAppearance.performAsCurrentDrawingAppearance {
            view.cacheDisplay(in: view.bounds, to: image)
        }
        guard let png = image.representation(using: .png, properties: [:]), png.count > 4096 else {
            throw BridgeFailure(message: "Native render did not produce a usable PNG")
        }
        let directory = report.deletingLastPathComponent()
        try png.write(to: directory.appendingPathComponent("Zmux-native-smoke.png"), options: .atomic)
        let windowNumber = String(window.windowNumber)
        let capturePath = directory.appendingPathComponent("Zmux-native-window.png").path
        let captureStatus = try await Task.detached {
            let capture = Process()
            capture.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
            capture.arguments = ["-x", "-o", "-l", windowNumber, capturePath]
            try capture.run()
            capture.waitUntilExit()
            return capture.terminationStatus
        }.value
        let evidence: [String: Any] = [
            "success": true,
            "native_input": true,
            "ansi_color": true,
            "split_panes": true,
            "kernel_resize": true,
            "workspace_switch": true,
            "pane_close": true,
            "active_terminal_title": true,
            "native_cloud_callback": true,
            "bundled_cli_control": true,
            "accessible_icon_controls": true,
            "native_render_png_bytes": png.count,
            "window_capture_status": captureStatus,
            "source_sha": Bundle.main.object(forInfoDictionaryKey: "ZmuxSourceSHA") as? String ?? "local"
        ]
        try JSONSerialization.data(withJSONObject: evidence, options: [.prettyPrinted, .sortedKeys]).write(to: report, options: .atomic)
    }

    private func waitUntil(_ predicate: (DesktopSnapshot) -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(20)
        while ContinuousClock.now < deadline {
            if let snapshot, predicate(snapshot) { return }
            try await Task.sleep(for: .milliseconds(32))
        }
        throw BridgeFailure(message: "Native smoke condition timed out: \(statusMessage)")
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var controller: AppController?
    private var terminating = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        let controller = AppController()
        self.controller = controller
        Task {
            do {
                try await controller.start()
                if let index = CommandLine.arguments.firstIndex(of: "--smoke-test"), CommandLine.arguments.count > index + 1 {
                    let report = URL(fileURLWithPath: CommandLine.arguments[index + 1])
                    do {
                        try await controller.smokeTest(report: report)
                        try await controller.shutdown()
                        exit(0)
                    } catch {
                        FileHandle.standardError.write(Data("Native smoke failed: \(error.localizedDescription)\n".utf8))
                        do { try await controller.shutdown() } catch {
                            FileHandle.standardError.write(Data("Cleanup failed: \(error)\n".utf8))
                        }
                        exit(1)
                    }
                }
            } catch {
                FileHandle.standardError.write(Data("zmux startup failed: \(error)\n".utf8))
                if CommandLine.arguments.contains("--smoke-test") { exit(1) }
                let alert = NSAlert()
                alert.messageText = "Unable to start zmux"
                alert.informativeText = error.localizedDescription
                alert.runModal()
                NSApp.terminate(nil)
            }
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let controller else { return .terminateNow }
        if terminating { return .terminateLater }
        terminating = true
        Task {
            do { try await controller.shutdown() } catch {
                FileHandle.standardError.write(Data("zmux shutdown: \(error)\n".utf8))
            }
            sender.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }
}

@main
@MainActor
enum ZmuxMain {
    static func main() {
        AppStartup.prepare()
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        let delegate = AppDelegate()
        app.delegate = delegate
        withExtendedLifetime(delegate) { app.run() }
    }
}

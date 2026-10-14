import AppKit
import Darwin

private enum UICommand: Sendable {
    case action(String, UInt64?, String?, String?, UInt64?)
    case surface(String, UInt64?, UInt64?, Int?, UInt64?)
    case attention(String, UInt64?, UInt64?, Bool?)
    case input(UInt64, String)
    case resize(UInt64, Int, Int)
}

@MainActor
final class AppController: NSObject, NSMenuDelegate, NSMenuItemValidation, NSWindowDelegate {
    let window: NSWindow
    var cloudWindow: NSWindowController?
    private(set) var snapshot: DesktopSnapshot?

    private let bridge = NativeBridge()
    private let sidebarContainer = BackgroundView(color: ZmuxPalette.sidebar)
    private let sidebarPanel = BackgroundView(color: ZmuxPalette.sidebar)
    private var sidebarWidth: NSLayoutConstraint?
    private var sidebarCollapsed = UserDefaults.standard.bool(forKey: "sidebarCollapsed")
    private var sidebarScroll: NSScrollView?
    private var sidebarDocumentConstraints: [NSLayoutConstraint] = []
    private var sidebarModes: NSSegmentedControl?
    private lazy var clearNotificationsButton = IconButton(symbol: "trash", label: "Clear displayed notifications", target: self, action: #selector(clearNotifications))
    private var notificationList: NSStackView?
    private var notificationKey = ""
    private lazy var notificationButton = IconButton(symbol: "bell", label: "Notifications", target: self, action: #selector(showNotifications))
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
    private let recentlyClosedMenu = NSMenu(title: "Recently Closed")
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
        window.delegate = self
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
                    case .surface(let action, let surface, let pane, let index, let closed):
                        try await self.bridge.action(action, pane: pane, surface: surface, index: index, closed: closed)
                    case .attention(let operation, let id, let through, let active):
                        try await self.bridge.attention(operation, id: id, through: through, active: active)
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
        appActive(NSApp.isActive)
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

        let sidebarChrome = DraggableChromeView(color: ZmuxPalette.sidebar)
        let sidebarSeparator = SeparatorView()
        let rightSeparator = SeparatorView()
        let modes = NSSegmentedControl(images: [NSImage(systemSymbolName: "square.stack", accessibilityDescription: "Workspaces") ?? NSImage(), NSImage(systemSymbolName: "bell", accessibilityDescription: "Notifications") ?? NSImage()], trackingMode: .selectOne, target: self, action: #selector(sidebarModeChanged(_:)))
        modes.segmentStyle = .texturedRounded
        modes.selectedSegment = 0
        modes.setToolTip("Workspaces", forSegment: 0)
        modes.setToolTip("Notifications", forSegment: 1)
        modes.setAccessibilityLabel("Sidebar tabs")
        sidebarModes = modes

        let newWorkspaceButton = makeIconButton(symbol: "plus", label: "New workspace", action: #selector(newWorkspace))
        let cloudButton = makeIconButton(symbol: "icloud", label: "Cloud account", action: #selector(showCloudSettings))
        let sidebarActions = NSStackView(views: [newWorkspaceButton, clearNotificationsButton, cloudButton])
        sidebarActions.orientation = .horizontal
        sidebarActions.spacing = 3

        let scroll = NSScrollView()
        sidebarScroll = scroll
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
        let newTabButton = makeIconButton(symbol: "plus", label: "New terminal tab (⌘T)", action: #selector(newTerminal))
        let toggleSidebarButton = makeIconButton(symbol: "sidebar.left", label: "Toggle sidebar (⌘B)", action: #selector(toggleSidebar))
        for button in [toggleSidebarButton, notificationButton, newTabButton, splitRightButton, splitDownButton] {
            mainHeader.actions.addArrangedSubview(button)
        }

        for view in [sidebarContainer, sidebarPanel, sidebarChrome, sidebarSeparator, rightSeparator, modes, sidebarActions, scroll, cloudFooter, mainHeader, canvas] {
            view.translatesAutoresizingMaskIntoConstraints = false
        }
        content.addSubview(sidebarContainer)
        sidebarContainer.addSubview(sidebarPanel)
        sidebarContainer.layer?.masksToBounds = true
        sidebarPanel.addSubview(sidebarChrome)
        sidebarPanel.addSubview(modes)
        sidebarPanel.addSubview(sidebarActions)
        sidebarPanel.addSubview(scroll)
        sidebarPanel.addSubview(cloudFooter)
        sidebarPanel.addSubview(rightSeparator)
        sidebarChrome.addSubview(sidebarSeparator)
        content.addSubview(mainHeader)
        content.addSubview(canvas)

        for button in actionButtons + [notificationButton, clearNotificationsButton] {
            button.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                button.widthAnchor.constraint(equalToConstant: 28),
                button.heightAnchor.constraint(equalToConstant: 28)
            ])
        }

        NSLayoutConstraint.activate([
            sidebarContainer.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            sidebarContainer.topAnchor.constraint(equalTo: content.topAnchor),
            sidebarContainer.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            sidebarPanel.leadingAnchor.constraint(equalTo: sidebarContainer.leadingAnchor),
            sidebarPanel.topAnchor.constraint(equalTo: sidebarContainer.topAnchor),
            sidebarPanel.bottomAnchor.constraint(equalTo: sidebarContainer.bottomAnchor),
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
            modes.leadingAnchor.constraint(equalTo: sidebarPanel.leadingAnchor, constant: 10),
            modes.topAnchor.constraint(equalTo: sidebarChrome.bottomAnchor, constant: 8),
            modes.trailingAnchor.constraint(equalTo: sidebarPanel.trailingAnchor, constant: -10),
            modes.heightAnchor.constraint(equalToConstant: 25),

            scroll.leadingAnchor.constraint(equalTo: sidebarPanel.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: sidebarPanel.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: modes.bottomAnchor, constant: 7),
            scroll.bottomAnchor.constraint(equalTo: cloudFooter.topAnchor),

            cloudFooter.leadingAnchor.constraint(equalTo: sidebarPanel.leadingAnchor),
            cloudFooter.trailingAnchor.constraint(equalTo: sidebarPanel.trailingAnchor),
            cloudFooter.bottomAnchor.constraint(equalTo: sidebarPanel.bottomAnchor),
            cloudFooter.heightAnchor.constraint(equalToConstant: 54),
            rightSeparator.trailingAnchor.constraint(equalTo: sidebarPanel.trailingAnchor),
            rightSeparator.topAnchor.constraint(equalTo: sidebarPanel.topAnchor),
            rightSeparator.bottomAnchor.constraint(equalTo: sidebarPanel.bottomAnchor),
            rightSeparator.widthAnchor.constraint(equalToConstant: 1),

            mainHeader.leadingAnchor.constraint(equalTo: sidebarContainer.trailingAnchor),
            mainHeader.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            mainHeader.topAnchor.constraint(equalTo: content.safeAreaLayoutGuide.topAnchor),
            mainHeader.heightAnchor.constraint(equalToConstant: 42),
            canvas.leadingAnchor.constraint(equalTo: sidebarContainer.trailingAnchor),
            canvas.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            canvas.topAnchor.constraint(equalTo: mainHeader.bottomAnchor),
            canvas.bottomAnchor.constraint(equalTo: content.bottomAnchor)
        ])
        sidebarWidth = sidebarContainer.widthAnchor.constraint(equalToConstant: sidebarCollapsed ? 0 : 224)
        sidebarWidth?.isActive = true
        sidebarPanel.isHidden = sidebarCollapsed
        clearNotificationsButton.isHidden = true
        setSidebarDocument(workspaceList)
    }

    @objc func toggleSidebar() {
        sidebarCollapsed.toggle()
        UserDefaults.standard.set(sidebarCollapsed, forKey: "sidebarCollapsed")
        sidebarWidth?.constant = sidebarCollapsed ? 0 : 224
        sidebarPanel.isHidden = sidebarCollapsed
        window.contentView?.layoutSubtreeIfNeeded()
        if let terminal = focusedTerminal { window.makeFirstResponder(terminal) }
    }

    func appActive(_ active: Bool) { enqueue(.attention("app_active", nil, nil, active && window.isKeyWindow)) }
    func windowDidBecomeKey(_ notification: Notification) { appActive(NSApp.isActive) }
    func windowDidResignKey(_ notification: Notification) { appActive(false) }

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
            ("New Terminal Tab", #selector(newTerminal), "t"),
            ("Toggle Sidebar", #selector(toggleSidebar), "b"),
            ("Notifications", #selector(showNotifications), ""),
            ("Split Right", #selector(splitRight), "d"),
            ("Split Down", #selector(splitDown), "D"),
            ("Rename Workspace", #selector(renameWorkspace), ""),
            ("Rename Tab…", #selector(renameTab), ""),
            ("Close Tab", #selector(closeTab), "w"),
            ("Close Pane", #selector(closePane), "")
        ]
        for (title, action, key) in items {
            let item = workspaceMenu.addItem(withTitle: title, action: action, keyEquivalent: key)
            item.target = self
        }
        let next = workspaceMenu.addItem(withTitle: "Next Tab", action: #selector(nextTab), keyEquivalent: "]")
        next.keyEquivalentModifierMask = [.command, .shift]; next.target = self
        let previous = workspaceMenu.addItem(withTitle: "Previous Tab", action: #selector(previousTab), keyEquivalent: "[")
        previous.keyEquivalentModifierMask = [.command, .shift]; previous.target = self
        workspaceItem.submenu = workspaceMenu

        let historyItem = NSMenuItem(title: "History", action: nil, keyEquivalent: "")
        menu.addItem(historyItem)
        let history = NSMenu(title: "History")
        let reopen = history.addItem(withTitle: "Reopen Last Closed", action: #selector(reopenClosed), keyEquivalent: "t")
        reopen.keyEquivalentModifierMask = [.command, .shift]; reopen.target = self
        let recent = history.addItem(withTitle: "Recently Closed", action: nil, keyEquivalent: "")
        recent.submenu = recentlyClosedMenu
        recentlyClosedMenu.delegate = self
        historyItem.submenu = history

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
        let liveSessions = Set(snapshot.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.map { $0.sessionId })
        terminals = terminals.filter { liveSessions.contains($0.key) }
        self.snapshot = snapshot
        statusMessage = snapshot.lastError ?? "Ready"
        rebuildWorkspaceRowsIfNeeded(snapshot)
        updateWorkspaceRows(snapshot)
        updateHeader(snapshot)
        updateNotifications(snapshot)
        updateTerminalLayoutIfNeeded(snapshot)

        for frame in snapshot.frames { terminals[frame.sessionId]?.update(frame) }
        updateTabAttention(canvas, snapshot: snapshot)
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

    private func updateTabAttention(_ view: NSView, snapshot: DesktopSnapshot) {
        if let bar = view as? TerminalTabBar { bar.updateAttention(snapshot.notifications) }
        for child in view.subviews { updateTabAttention(child, snapshot: snapshot) }
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
            let surfaces = Set(workspace.root.panes.flatMap { $0.surfaces }.map { $0.id })
            let unread = snapshot.notifications.filter { !$0.read && surfaces.contains($0.surfaceId) }.count
            row.update(title: workspace.name, subtitle: subtitle, isSelected: isSelected, isRunning: isRunning, unread: unread)
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
        if let pane = snapshot.workspace?.root.panes.first(where: { $0.id == snapshot.workspace?.focusedPane }), pane.selected.title != "Terminal" { return pane.selected.title }
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
            let view = terminals[pane.sessionId] ?? TerminalView(pane: pane)
            view.pane = pane
            view.removeFromSuperview()
            view.onInput = { [weak self] text in self?.enqueue(.input(pane.sessionId, text)) }
            view.onResize = { [weak self] rows, cols in self?.enqueue(.resize(pane.sessionId, rows, cols)) }
            view.onFocus = { [weak self] in self?.enqueue(.action("focus_pane", pane.id, nil, nil, nil)) }
            terminals[pane.sessionId] = view
            let container = BackgroundView(color: ZmuxPalette.terminal)
            let tabs = TerminalTabBar(pane: pane, owner: self)
            tabs.translatesAutoresizingMaskIntoConstraints = false
            view.translatesAutoresizingMaskIntoConstraints = false
            container.addSubview(tabs)
            container.addSubview(view)
            NSLayoutConstraint.activate([
                tabs.leadingAnchor.constraint(equalTo: container.leadingAnchor),
                tabs.trailingAnchor.constraint(equalTo: container.trailingAnchor),
                tabs.topAnchor.constraint(equalTo: container.topAnchor),
                tabs.heightAnchor.constraint(equalToConstant: 32),
                view.leadingAnchor.constraint(equalTo: container.leadingAnchor),
                view.trailingAnchor.constraint(equalTo: container.trailingAnchor),
                view.topAnchor.constraint(equalTo: tabs.bottomAnchor),
                view.bottomAnchor.constraint(equalTo: container.bottomAnchor)
            ])
            return container
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

    private var focusedPane: Pane? {
        snapshot?.workspace?.root.panes.first { $0.id == snapshot?.workspace?.focusedPane }
    }

    func selectTab(_ id: UInt64) { enqueue(.surface("select_surface", id, nil, nil, nil)) }
    func closeTerminalTab(_ id: UInt64) { enqueue(.surface("close_surface", id, nil, nil, nil)) }
    func newTerminalInPane(_ paneID: UInt64) { enqueue(.surface("create_surface", nil, paneID, nil, nil)) }
    @objc func newTerminal() {
        if let pane = focusedPane { newTerminalInPane(pane.id) } else { newWorkspace() }
    }
    @objc func closeTab() { if let pane = focusedPane { closeTerminalTab(pane.selectedSurface) } }
    @objc func reopenClosed() { enqueue(.surface("reopen_closed", nil, nil, nil, nil)) }
    @objc private func reopenHistoryItem(_ sender: NSMenuItem) {
        guard let id = sender.representedObject as? NSNumber else { return }
        enqueue(.surface("reopen_closed", nil, nil, nil, id.uint64Value))
    }
    @objc func nextTab() { cycleTab(1) }
    @objc func previousTab() { cycleTab(-1) }
    private func cycleTab(_ delta: Int) {
        guard let pane = focusedPane, let index = pane.surfaces.firstIndex(where: { $0.id == pane.selectedSurface }) else { return }
        selectTab(pane.surfaces[(index + delta + pane.surfaces.count) % pane.surfaces.count].id)
    }
    @objc func renameTab() {
        if let surface = focusedPane?.selected { renameTerminalTab(surface.id) }
    }
    func renameTerminalTab(_ id: UInt64) {
        guard let surface = snapshot?.model.workspaces.flatMap({ $0.root.panes }).flatMap({ $0.surfaces }).first(where: { $0.id == id }) else { return }
        let alert = NSAlert()
        alert.messageText = "Rename terminal tab"
        let field = NSTextField(string: surface.title)
        field.frame = NSRect(x: 0, y: 0, width: 280, height: 24)
        alert.accessoryView = field
        alert.addButton(withTitle: "Rename"); alert.addButton(withTitle: "Cancel")
        if alert.runModal() == .alertFirstButtonReturn {
            Task { [weak self] in
                do { try await self?.bridge.action("rename_surface", title: field.stringValue, surface: id) }
                catch { self?.showError(error.localizedDescription) }
            }
        }
    }
    func closeOtherTabs(_ id: UInt64) {
        guard let pane = snapshot?.model.workspaces.flatMap({ $0.root.panes }).first(where: { $0.surfaces.contains(where: { $0.id == id }) }),
              confirmClose(title: "Close other tabs?", count: pane.surfaces.count - 1) else { return }
        enqueue(.surface("close_other_surfaces", id, nil, nil, nil))
    }
    private func confirmClose(title: String, count: Int) -> Bool {
        guard count > 0 else { return false }
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = "This closes \(count) terminal session(s). Reopening starts fresh shells; running jobs cannot be recovered."
        alert.addButton(withTitle: "Close"); alert.addButton(withTitle: "Cancel")
        return alert.runModal() == .alertFirstButtonReturn
    }
    func reorderTab(_ id: UInt64, in paneID: UInt64, to index: Int) {
        enqueue(.surface("move_surface", id, paneID, index, nil))
    }
    func menuNeedsUpdate(_ menu: NSMenu) {
        guard menu === recentlyClosedMenu else { return }
        menu.removeAllItems()
        for item in (snapshot?.model.closedItems ?? []).reversed() {
            let entry = menu.addItem(withTitle: "\(item.title) — \(item.workspaceName)" + (item.restoring ? " (restoring…)" : ""), action: #selector(reopenHistoryItem(_:)), keyEquivalent: "")
            entry.target = self; entry.representedObject = NSNumber(value: item.id); entry.isEnabled = !item.restoring
        }
        if menu.items.isEmpty { let empty = menu.addItem(withTitle: "No recently closed items", action: nil, keyEquivalent: ""); empty.isEnabled = false }
    }
    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(reopenClosed): return snapshot?.model.closedItems.last.map { !$0.restoring } ?? false
        case #selector(reopenHistoryItem(_:)):
            guard let id = item.representedObject as? NSNumber else { return false }
            return snapshot?.model.closedItems.contains { $0.id == id.uint64Value && !$0.restoring } ?? false
        case #selector(closeTab), #selector(closePane), #selector(renameTab), #selector(splitRight), #selector(splitDown): return focusedPane != nil
        case #selector(nextTab), #selector(previousTab): return (focusedPane?.surfaces.count ?? 0) > 1
        default: return true
        }
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
        guard confirmClose(title: "Close workspace?", count: workspace.root.panes.reduce(0) { $0 + $1.surfaces.count }) else { return }
        enqueue(.action("close_workspace", nil, nil, nil, workspace.id))
    }

    private func setSidebarDocument(_ view: NSView) {
        guard let scroll = sidebarScroll else { return }
        NSLayoutConstraint.deactivate(sidebarDocumentConstraints)
        view.translatesAutoresizingMaskIntoConstraints = false
        scroll.documentView = view
        sidebarDocumentConstraints = [view.leadingAnchor.constraint(equalTo: scroll.contentView.leadingAnchor),
            view.topAnchor.constraint(equalTo: scroll.contentView.topAnchor), view.widthAnchor.constraint(equalTo: scroll.contentView.widthAnchor)]
        NSLayoutConstraint.activate(sidebarDocumentConstraints)
    }
    @objc private func sidebarModeChanged(_ sender: NSSegmentedControl) {
        if sender.selectedSegment == 1 { showNotifications() }
        else { clearNotificationsButton.isHidden = true; setSidebarDocument(workspaceList) }
    }
    @objc func showNotifications() {
        if sidebarCollapsed { toggleSidebar() }
        sidebarModes?.selectedSegment = 1
        clearNotificationsButton.isHidden = false
        if notificationList == nil {
            let list = FlippedStackView()
            list.orientation = .vertical; list.alignment = .leading; list.spacing = 6
            list.edgeInsets = NSEdgeInsets(top: 0, left: 8, bottom: 12, right: 8)
            notificationList = list
        }
        if let list = notificationList { setSidebarDocument(list) }
        notificationKey = ""
        if let snapshot { updateNotifications(snapshot) }
    }
    private func updateNotifications(_ snapshot: DesktopSnapshot) {
        let unread = snapshot.notifications.filter { !$0.read }.count
        notificationButton.image = NSImage(systemSymbolName: unread > 0 ? "bell.badge.fill" : "bell", accessibilityDescription: "Notifications")
        notificationButton.contentTintColor = unread > 0 ? .systemOrange : .secondaryLabelColor
        notificationButton.toolTip = unread > 0 ? "\(unread) unread notifications" : "Notifications"
        notificationButton.setAccessibilityLabel("Notifications, \(unread) unread")
        guard let list = notificationList else { return }
        let key = snapshot.notifications.map { "\($0.id):\($0.read)" }.joined(separator: ",") + ":\(snapshot.model.revision):\(snapshot.notificationsDropped)"
        guard key != notificationKey else { return }
        notificationKey = key
        list.arrangedSubviews.forEach { list.removeArrangedSubview($0); $0.removeFromSuperview() }
        let live = Set(snapshot.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.map { $0.id })
        if snapshot.notifications.isEmpty {
            let label = NSTextField(wrappingLabelWithString: "No notifications. Terminal bells and explicit notifications appear here.")
            label.textColor = ZmuxPalette.subdued; list.addArrangedSubview(label)
            label.widthAnchor.constraint(equalTo: list.widthAnchor, constant: -24).isActive = true
        }
        for item in snapshot.notifications.reversed() {
            let row = NotificationRowView(item: item, available: live.contains(item.surfaceId), owner: self)
            list.addArrangedSubview(row)
            row.widthAnchor.constraint(equalTo: list.widthAnchor, constant: -24).isActive = true
        }
        if snapshot.notificationsDropped > 0 {
            let label = NSTextField(labelWithString: "\(snapshot.notificationsDropped) older signals omitted by history limits")
            label.font = .systemFont(ofSize: 10); label.textColor = ZmuxPalette.subdued; list.addArrangedSubview(label)
        }
    }
    func openNotification(_ id: UInt64) {
        enqueue(.attention("open", id, nil, nil))
        window.makeKeyAndOrderFront(nil)
    }
    func readNotification(_ id: UInt64) { enqueue(.attention("read", id, nil, nil)) }
    func dismissNotification(_ id: UInt64) { enqueue(.attention("dismiss", id, nil, nil)) }
    @objc private func clearNotifications() {
        if let through = snapshot?.notifications.last?.id { enqueue(.attention("clear", nil, through, nil)) }
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
        let expectedControls = Set(["New workspace", "Cloud account", "Split right", "Split down", "New terminal tab (⌘T)", "Toggle sidebar (⌘B)", "Cloud settings"])
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
        guard let originalPane = snapshot?.workspace?.root.panes.first,
              let originalPID = snapshot?.frames.first?.processId else { throw BridgeFailure(message: "Missing original tab") }
        try performShortcut("t", modifiers: [.command])
        try await waitUntil { $0.workspace?.root.panes.first?.surfaces.count == 2 && $0.frames.first?.snapshot?.state == "running" }
        guard snapshot?.workspace?.root.panes.count == 1, snapshot?.frames.first?.processId != originalPID else {
            throw BridgeFailure(message: "New terminal shortcut did not create an independent tab")
        }
        try performShortcut("w", modifiers: [.command])
        try await waitUntil { $0.workspace?.root.panes.first?.surfaces.count == 1 && $0.frames.first?.processId == originalPID }
        try performShortcut("t", modifiers: [.command, .shift])
        try await waitUntil { $0.workspace?.root.panes.first?.surfaces.count == 2 && $0.model.closedItems.isEmpty && $0.frames.first?.snapshot?.state == "running" }
        guard snapshot?.workspace?.focusedPane == originalPane.id else { throw BridgeFailure(message: "Reopen did not restore tab placement") }
        try performShortcut("w", modifiers: [.command])
        try await waitUntil { $0.workspace?.root.panes.first?.surfaces.count == 1 && $0.frames.first?.processId == originalPID }
        let wasCollapsed = sidebarCollapsed
        let priorWidth = canvas.bounds.width
        try performShortcut("b", modifiers: [.command])
        guard sidebarCollapsed != wasCollapsed, canvas.bounds.width != priorWidth else { throw BridgeFailure(message: "Sidebar toggle did not resize terminal space") }
        try performShortcut("b", modifiers: [.command])
        guard sidebarCollapsed == wasCollapsed else { throw BridgeFailure(message: "Sidebar visibility did not restore") }
        newTerminal()
        try await waitUntil { $0.workspace?.root.panes.first?.surfaces.count == 2 && $0.frames.first?.snapshot?.state == "running" }
        guard let attentionTab = snapshot?.workspace?.root.panes.first?.selectedSurface else { throw BridgeFailure(message: "No attention test tab") }
        enqueue(.input(originalPane.sessionId, "printf '\\033]777;notify;Agent ready;Review requested\\007'\r"))
        try await waitUntil { $0.notifications.contains { $0.surfaceId == originalPane.selectedSurface && !$0.read } }
        showNotifications()
        guard sidebarModes?.selectedSegment == 1, !(notificationList?.arrangedSubviews.isEmpty ?? true), !notificationButton.isHidden else { throw BridgeFailure(message: "Notification inbox is unavailable") }
        if let item = snapshot?.notifications.last(where: { $0.surfaceId == originalPane.selectedSurface }) {
            openNotification(item.id)
            try await waitUntil { $0.workspace?.root.panes.first?.selectedSurface == originalPane.selectedSurface && $0.notifications.first(where: { $0.id == item.id })?.read == true }
        }
        closeTerminalTab(attentionTab)
        try await waitUntil { $0.workspace?.root.panes.first?.surfaces.count == 1 && $0.frames.first?.processId == originalPID }
        sidebarModes?.selectedSegment = 0
        clearNotificationsButton.isHidden = true
        setSidebarDocument(workspaceList)
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
            "terminal_tab_shortcut": true,
            "close_tab_shortcut": true,
            "reopen_tab_shortcut": true,
            "sibling_process_preserved": true,
            "sidebar_toggle": true,
            "notification_inbox": true,
            "notification_target_routing": true,
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

    private func performShortcut(_ key: String, modifiers: NSEvent.ModifierFlags) throws {
        guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers,
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, characters: modifiers.contains(.shift) ? key.uppercased() : key,
            charactersIgnoringModifiers: key, isARepeat: false, keyCode: 0),
            NSApp.mainMenu?.performKeyEquivalent(with: event) == true else {
            throw BridgeFailure(message: "Native menu did not handle shortcut \(key)")
        }
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

    func applicationDidBecomeActive(_ notification: Notification) { controller?.appActive(true) }
    func applicationDidResignActive(_ notification: Notification) { controller?.appActive(false) }

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

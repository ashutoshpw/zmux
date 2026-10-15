import AppKit
import Darwin
import WebKit

private enum UICommand: Sendable {
    case action(String, UInt64?, String?, String?, UInt64?)
    case surface(String, UInt64?, UInt64?, Int?, UInt64?)
    case navigation(String, String, UInt64?, UInt64?, UInt64?, Bool?)
    case attention(String, UInt64?, UInt64?, Bool?)
    case browser(String, UInt64?, UInt64?, String, String?)
    case browserAutomation(UInt64, BrowserAction)
    case pasteFiles(UInt64, [String])
    case cancelUpload(UInt64)
    case input(UInt64, String)
    case resize(UInt64, Int, Int)
    case scroll(UInt64, Int)
    case wheel(UInt64, Int, Int, Int, Int, Bool)
}

@MainActor
final class AppController: NSObject, NSMenuDelegate, NSMenuItemValidation, NSWindowDelegate {
    let window: NSWindow
    var cloudWindow: NSWindowController?
    private(set) var snapshot: DesktopSnapshot?

    private let bridge = NativeBridge()
    private lazy var notificationDelivery = makeNotificationDelivery()
    private func makeNotificationDelivery() -> NotificationDelivery {
        NotificationDelivery(
            sendEvent: { [weak self] event in
                guard let self else { throw BridgeFailure(message: "Desktop is closed") }
                try await self.bridge.notificationEvent(event)
            },
            performAction: { [weak self] action in
                guard let self else { throw BridgeFailure(message: "Desktop is closed") }
                return try await self.bridge.notificationAction(action)
            },
            activate: { [weak self] in
                self?.window.makeKeyAndOrderFront(nil)
                NSApp.activate()
            },
            reportError: { [weak self] message in self?.showError(message) }
        )
    }
    private lazy var imagePastes = ClipboardImagePastes(bridge: bridge, reportError: { [weak self] message in
        self?.showError(message)
    }, statusChanged: { [weak self] in
        guard let self, let snapshot = self.snapshot else { return }
        self.updateHeader(snapshot)
    })
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
    private lazy var cancelUploadButton = IconButton(symbol: "xmark.circle", label: "Cancel active upload", target: self, action: #selector(cancelActiveUpload))
    private var statusMessage = "Starting shell…"
    private var presentationError: String?
    private lazy var cloudFooter = CloudFooterView(target: self, action: #selector(showCloudSettings))
    private var actionButtons: [IconButton] = []
    private let stream: AsyncStream<UICommand>
    private let continuation: AsyncStream<UICommand>.Continuation
    private var commandTask: Task<Void, Never>?
    private var pollingTask: Task<Void, Never>?
    private let browserEvents = AsyncStream<(UInt64, BrowserHostEvent?)>.makeStream(bufferingPolicy: .bufferingOldest(256))
    private var browserEventGeneration: UInt64 = 0
    private var browserEventsNeedRecovery = false
    private var browserEventTask: Task<Void, Never>?
    private var browserEpoch: UUID?
    private var layoutKey = ""
    private var splitFractions: [String: CGFloat] = [:]
    private var modelRevision: UInt64 = .max
    private var terminals: [UInt64: TerminalView] = [:]
    private var browsers: [UInt64: BrowserView] = [:]
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

    func start(persistent: Bool = true) async throws {
        try await bridge.start(persistent: persistent)
        browserEventTask = Task { [weak self] in
            guard let self else { return }
            for await (generation, event) in self.browserEvents.stream {
                if Task.isCancelled { break }
                if self.browserEventsNeedRecovery { await self.recoverBrowserHostEvents() }
                guard generation == self.browserEventGeneration, let event else { continue }
                do { try await self.bridge.browserEvent(event) }
                catch {
                    self.browserEventsNeedRecovery = true
                    self.showError("Browser host synchronization failed: " + error.localizedDescription)
                }
                if self.browserEventsNeedRecovery { await self.recoverBrowserHostEvents() }
            }
        }
        commandTask = Task { [weak self, stream] in
            for await command in stream {
                guard let self, !Task.isCancelled else { break }
                do {
                    switch command {
                    case .action(let action, let pane, let axis, let title, let workspace):
                        try await self.bridge.action(action, pane: pane, axis: axis, title: title, workspace: workspace)
                    case .surface(let action, let surface, let pane, let index, let closed):
                        try await self.bridge.action(action, pane: pane, surface: surface, index: index, closed: closed)
                    case .navigation(let action, let direction, let pane, let surface, let workspace, let focus):
                        try await self.bridge.action(action, pane: pane, workspace: workspace, surface: surface, direction: direction, focus: focus)
                    case .browser(let operation, let surface, let pane, let url, let title):
                        try await self.bridge.browser(operation, surface: surface, pane: pane, url: url, title: title)
                    case .browserAutomation(let surface, let action):
                        try await self.bridge.browserAction(surface: surface, action: action)
                    case .attention(let operation, let id, let through, let active):
                        try await self.bridge.attention(operation, id: id, through: through, active: active)
                    case .pasteFiles(let surface, let paths):
                        try await self.bridge.pasteFiles(surface: surface, paths: paths)
                    case .cancelUpload(let id):
                        try await self.bridge.cancelUpload(id: id)
                    case .input(let session, let text):
                        try await self.bridge.input(session: session, text: text)
                    case .wheel(let session, let lines, let row, let col, let modifiers, let history):
                        try await self.bridge.wheel(session: session, lines: lines, row: row, col: col, modifiers: modifiers, history: history)
                    case .scroll(let session, let lines):
                        try await self.bridge.scroll(session: session, lines: lines)
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
                    if self.browserEventsNeedRecovery {
                        // A wakeup, not a second sender: recovery must progress even
                        // when the last lifecycle event was the one that failed.
                        _ = self.browserEvents.continuation.yield((self.browserEventGeneration, nil))
                    }
                    if let snapshot = try await self.bridge.poll() { self.present(snapshot) }
                    if (self.snapshot?.browserPendingRequests ?? 0) > 0 {
                        let batch = try await self.bridge.drainBrowserRequests()
                        if batch.epoch == self.browserEpoch {
                            for request in batch.requests {
                                self.browsers[request.owner.surfaceId]?.executeAutomation(request)
                            }
                        }
                    }
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
        browserEvents.continuation.finish()
        browserEventTask?.cancel()
        notificationDelivery.stop()
        await imagePastes.stopAccepting()
        try await bridge.close()
        await imagePastes.desktopClosed()
    }

    func updateCloudStatus(_ label: String, connected: Bool) {
        cloudFooter.update(label: label, connected: connected)
    }

    private func enqueue(_ command: UICommand) {
        presentationError = nil
        if case .dropped = continuation.yield(command) {
            showError("Command queue is full; the new command was not accepted")
        }
    }

    private func showError(_ message: String) {
        presentationError = message
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
        let browserButton = makeIconButton(symbol: "globe", label: "New browser tab", action: #selector(newBrowser))
        let newTabButton = makeIconButton(symbol: "plus", label: "New terminal tab (⌘T)", action: #selector(newTerminal))
        let toggleSidebarButton = makeIconButton(symbol: "sidebar.left", label: "Toggle sidebar (⌘B)", action: #selector(toggleSidebar))
        for button in [toggleSidebarButton, notificationButton, newTabButton, browserButton, splitRightButton, splitDownButton] {
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
        mainHeader.actions.addArrangedSubview(cancelUploadButton)
        cancelUploadButton.isHidden = true
        cancelUploadButton.widthAnchor.constraint(equalToConstant: 28).isActive = true
        cancelUploadButton.heightAnchor.constraint(equalToConstant: 28).isActive = true
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

    func appActive(_ active: Bool) {
        enqueue(.attention("app_active", nil, nil, active && window.isKeyWindow))
        if active { notificationDelivery.refreshPermission() }
    }
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
            ("New Browser Tab", #selector(newBrowser), ""),
            ("Focus Browser Address", #selector(focusBrowserAddress), "l"),
            ("Toggle Sidebar", #selector(toggleSidebar), "b"),
            ("Notifications", #selector(showNotifications), ""),
            ("Enable Desktop Notifications…", #selector(enableDesktopNotifications), ""),
            ("Split Right", #selector(splitRight), "d"),
            ("Split Down", #selector(splitDown), "D"),
            ("Split Left", #selector(splitLeft), ""),
            ("Split Up", #selector(splitUp), ""),
            ("Rename Workspace", #selector(renameWorkspace), ""),
            ("Rename Tab…", #selector(renameTab), ""),
            ("Close Tab", #selector(closeTab), "w"),
            ("Close Pane", #selector(closePane), "")
        ]
        for (title, action, key) in items {
            let item = workspaceMenu.addItem(withTitle: title, action: action, keyEquivalent: key)
            item.target = self
        }
        // AppKit matches the resulting character, as it does for uppercase T/W.
        // Shift-brackets produce braces on the standard keyboard layout.
        let next = workspaceMenu.addItem(withTitle: "Next Tab", action: #selector(nextTab), keyEquivalent: "}")
        next.keyEquivalentModifierMask = [.command]; next.target = self
        let previous = workspaceMenu.addItem(withTitle: "Previous Tab", action: #selector(previousTab), keyEquivalent: "{")
        previous.keyEquivalentModifierMask = [.command]; previous.target = self
        let navigationItems: [(String, Selector, String, NSEvent.ModifierFlags)] = [
            ("Last Tab", #selector(lastTab), "", NSEvent.ModifierFlags.command),
            ("Next Workspace", #selector(nextWorkspace), "]", [.command, .control]),
            ("Previous Workspace", #selector(previousWorkspace), "[", [.command, .control]),
            ("Last Workspace", #selector(lastWorkspace), "", .command),
            ("Focus Pane Left", #selector(focusPaneLeft), String(UnicodeScalar(NSLeftArrowFunctionKey)!), [.command, .option]),
            ("Focus Pane Right", #selector(focusPaneRight), String(UnicodeScalar(NSRightArrowFunctionKey)!), [.command, .option]),
            ("Focus Pane Up", #selector(focusPaneUp), String(UnicodeScalar(NSUpArrowFunctionKey)!), [.command, .option]),
            ("Focus Pane Down", #selector(focusPaneDown), String(UnicodeScalar(NSDownArrowFunctionKey)!), [.command, .option]),
            ("Next Pane", #selector(nextPane), "", .command),
            ("Previous Pane", #selector(previousPane), "", .command),
            ("Last Pane", #selector(lastPane), "", .command)
        ]
        for (title, action, key, modifiers) in navigationItems {
            let item = workspaceMenu.addItem(withTitle: title, action: action, keyEquivalent: key)
            item.keyEquivalentModifierMask = modifiers; item.target = self
        }
        workspaceItem.submenu = workspaceMenu

        let historyItem = NSMenuItem(title: "History", action: nil, keyEquivalent: "")
        menu.addItem(historyItem)
        let history = NSMenu(title: "History")
        for (title, action, key) in [("Focus Back", #selector(focusBack), "["), ("Focus Forward", #selector(focusForward), "]")] {
            let item = history.addItem(withTitle: title, action: action, keyEquivalent: key)
            item.target = self
        }
        history.addItem(.separator())
        let reopen = history.addItem(withTitle: "Reopen Last Closed", action: #selector(reopenClosed), keyEquivalent: "T")
        reopen.keyEquivalentModifierMask = [.command]; reopen.target = self
        let recent = history.addItem(withTitle: "Recently Closed", action: nil, keyEquivalent: "")
        recent.submenu = recentlyClosedMenu
        recentlyClosedMenu.delegate = self
        historyItem.submenu = history

        let browserItem = NSMenuItem(title: "Browser", action: nil, keyEquivalent: "")
        let browserMenu = NSMenu(title: "Browser")
        for (title, action, key) in [
            ("Back", #selector(browserBack), ""), ("Forward", #selector(browserForward), ""),
            ("Reload", #selector(browserReload), "r"), ("Find in Page…", #selector(browserFind), "f"),
            ("Zoom In", #selector(browserZoomIn), "+"), ("Zoom Out", #selector(browserZoomOut), "-"),
            ("Actual Size", #selector(browserZoomReset), "0")
        ] {
            let item = browserMenu.addItem(withTitle: title, action: action, keyEquivalent: key); item.target = self
        }
        browserItem.submenu = browserMenu; menu.addItem(browserItem)
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
        let selectionChanged = self.snapshot?.model.focusedWorkspace != snapshot.model.focusedWorkspace
            || self.snapshot?.workspace?.focusedPane != snapshot.workspace?.focusedPane
            || focusedPane?.selectedSurface != snapshot.workspace?.root.panes.first(where: { $0.id == snapshot.workspace?.focusedPane })?.selectedSurface
        let layoutChanged = layoutKey != (snapshot.workspace?.root.key ?? "empty")
        let liveSessions = Set(snapshot.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.compactMap { $0.sessionId })
        terminals = terminals.filter { liveSessions.contains($0.key) }
        let liveBrowsers = Set(snapshot.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.filter { $0.kind == "browser" }.map { $0.id })
        browsers = browsers.filter { key, value in
            if liveBrowsers.contains(key) { return true }
            value.close(); return false
        }
        if browserEpoch != snapshot.browserEpoch {
            browsers.values.forEach { $0.close() }
            browsers.removeAll()
            browserEpoch = snapshot.browserEpoch
            layoutKey = ""
        }
        self.snapshot = snapshot
        // Keep the bounded browser set mounted even when its tabs are hidden so
        // automation targets never need to change the user's selection or focus.
        for surface in snapshot.model.workspaces.flatMap({ $0.root.panes }).flatMap({ $0.surfaces }) where surface.kind == "browser" {
            let view: BrowserView
            if let existing = browsers[surface.id] { view = existing }
            else {
                view = BrowserView(surface: surface, owner: self)
                view.frame = NSRect(x: 0, y: 0, width: min(1920, max(640, canvas.bounds.width)), height: min(1200, max(480, canvas.bounds.height)))
                view.layoutSubtreeIfNeeded()
            }
            browsers[surface.id] = view
            view.onAutomationAction = { [weak self] action in self?.enqueue(.browserAutomation(surface.id, action)) }
            view.bindAutomation(epoch: snapshot.browserEpoch, identityNamespace: snapshot.model.identityNamespace) { [weak self] event in
                guard let self else { return }
                if case .dropped = self.browserEvents.continuation.yield((self.browserEventGeneration, event)) {
                    self.browserEventsNeedRecovery = true
                    self.showError("Browser event queue is full; resynchronizing browser hosts")
                }
            }
        }
        imagePastes.update(snapshot)
        statusMessage = snapshot.lastError ?? "Ready"
        rebuildWorkspaceRowsIfNeeded(snapshot)
        updateWorkspaceRows(snapshot)
        updateHeader(snapshot)
        updateNotifications(snapshot)
        notificationDelivery.update(snapshot.nativeNotifications)
        updateTerminalLayoutIfNeeded(snapshot)
        let liveSplits = Set(snapshot.model.workspaces.flatMap { $0.root.splitKeys })
        splitFractions = splitFractions.filter { liveSplits.contains($0.key) }

        for frame in snapshot.frames { terminals[frame.sessionId]?.update(frame) }
        for surface in snapshot.model.workspaces.flatMap({ $0.root.panes }).flatMap({ $0.surfaces }) where surface.kind == "browser" {
            browsers[surface.id]?.update(surface)
        }
        updateTabAttention(canvas, snapshot: snapshot)
        if let pane = snapshot.workspace?.root.panes.first(where: { $0.id == snapshot.workspace?.focusedPane }),
           let session = pane.sessionId, let terminal = terminals[session] {
            if selectionChanged || !didSetInitialTerminalFocus {
                window.makeFirstResponder(terminal)
                didSetInitialTerminalFocus = true
            } else if let responder = window.firstResponder as? TerminalView, responder !== terminal {
                window.makeFirstResponder(terminal)
            }
        } else if selectionChanged && !layoutChanged, let pane = focusedPane, let browser = browsers[pane.selectedSurface] {
            window.makeFirstResponder(browser.webView)
        }
    }

    private func recoverBrowserHostEvents() async {
        browserEventsNeedRecovery = false
        browserEventGeneration &+= 1
        // Discard queued incremental events captured before this full-owner snapshot.
        // New events retain order after the snapshot while acknowledgements are awaited.
        let attachments = browsers.values.compactMap { $0.automationAttachment() }
        for attachment in attachments {
            if Task.isCancelled { return }
            do { try await bridge.browserEvent(attachment) }
            catch {
                browserEventsNeedRecovery = true
                showError("Browser synchronization is retrying: " + error.localizedDescription)
                return
            }
        }
    }

    private func updateTabAttention(_ view: NSView, snapshot: DesktopSnapshot) {
        if let bar = view as? TerminalTabBar { bar.updateAttention(snapshot.notifications, surfaces: snapshot.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }) }
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
                subtitle = workspace.root.panes.flatMap { $0.surfaces }.compactMap { $0.remote?.destination }.first.map { "SSH · \($0)" } ?? (panes == 1 ? "Local shell" : "\(panes) local terminals")
            }
            let unread = snapshot.notifications.filter { !$0.read && $0.identityNamespace == snapshot.model.identityNamespace && $0.workspaceId == workspace.id }.count
            row.update(title: workspace.name, subtitle: subtitle, isSelected: isSelected, isRunning: isRunning, unread: unread)
        }
    }

    private func updateHeader(_ snapshot: DesktopSnapshot) {
        let title = terminalTitle(in: snapshot)
        let detail: String
        let focusedSurface = focusedPane?.selectedSurface
        let activeUpload = snapshot.uploads.last { $0.delivery == "pending" }
        let latestUpload = snapshot.uploads.last { $0.surfaceId == focusedSurface }
        let uploadFailure = latestUpload.flatMap { $0.delivery == "failed" ? $0 : nil }
        cancelUploadButton.isHidden = activeUpload == nil
        if let activeUpload { cancelUploadButton.toolTip = "Cancel upload \(activeUpload.id)" }
        let isError = presentationError != nil || snapshot.lastError != nil || uploadFailure != nil || snapshot.persistence?.error != nil
        if let error = presentationError {
            detail = error
        } else if let error = snapshot.persistence?.error {
            detail = "Session recovery: \(error)"
        } else if snapshot.persistence?.startupPending == true || snapshot.persistence?.restoring == true {
            detail = "Restoring session…"
        } else if let activeUpload {
            detail = "Uploading \(ByteCountFormatter.string(fromByteCount: Int64(activeUpload.bytesSent), countStyle: .file)) of \(ByteCountFormatter.string(fromByteCount: Int64(activeUpload.totalBytes), countStyle: .file))"
        } else if imagePastes.preparingCount > 0 {
            detail = "Preparing image…"
        } else if let failure = uploadFailure {
            detail = failure.deliveryError ?? failure.error ?? "Upload failed"
        } else if let error = snapshot.lastError {
            detail = error
        } else if let pane = focusedPane, pane.selected.kind == "browser" {
            detail = pane.selected.url == "about:blank" ? "Private browser" : (pane.selected.url ?? "Private browser")
        } else if let workingDirectory = focusedViewport(in: snapshot)?.workingDirectory, !workingDirectory.isEmpty {
            detail = (workingDirectory as NSString).abbreviatingWithTildeInPath
        } else if let endpoint = focusedPane?.selected.remote {
            detail = "SSH · \(endpoint.destination)"
        } else if let workspace = snapshot.workspace {
            let count = workspace.root.panes.count
            detail = count == 1 ? "Local shell" : "\(count) local terminals"
        } else {
            detail = "No terminal"
        }
        mainHeader.update(title: title, detail: detail, error: isError)
        window.title = title
    }

    @objc private func cancelActiveUpload(_ sender: Any?) {
        if let upload = snapshot?.uploads.last(where: { $0.delivery == "pending" }) { enqueue(.cancelUpload(upload.id)) }
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
              let pane = workspace.root.panes.first(where: { $0.id == workspace.focusedPane }), let session = pane.sessionId else { return nil }
        return snapshot.frames.first(where: { $0.sessionId == session })?.snapshot?.viewport
    }

    private func updateTerminalLayoutIfNeeded(_ snapshot: DesktopSnapshot) {
        let key = snapshot.workspace?.root.key ?? "empty"
        guard key != layoutKey else { return }
        layoutKey = key
        didSetInitialTerminalFocus = false
        canvas.subviews.forEach { rememberSplits($0); $0.removeFromSuperview() }
        if let workspace = snapshot.workspace {
            let root = paneView(workspace.root)
            root.frame = canvas.bounds
            root.autoresizingMask = [.width, .height]
            canvas.addSubview(root)
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
        // Resolve AppKit constraints before setting divider positions; an earlier
        // position can be discarded when a new tab's native host is laid out.
        canvas.layoutSubtreeIfNeeded()
        canvas.subviews.forEach(initializeSplits)
        canvas.layoutSubtreeIfNeeded()
        if let pane = focusedPane, let browser = browsers[pane.selectedSurface] { browser.focusAddress() }
    }

    private func paneView(_ tree: PaneTree) -> NSView {
        switch tree {
        case .leaf(let pane):
            let view: NSView
            if let session = pane.sessionId {
                let terminal = terminals[session] ?? TerminalView(pane: pane)
                terminal.pane = pane
                terminal.onFiles = { [weak self] paths in self?.enqueue(.pasteFiles(pane.selectedSurface, paths)) }
                terminal.onImage = { [weak self] data in
                    guard let self else { return false }
                    self.presentationError = nil
                    return self.imagePastes.paste(data, surface: pane.selectedSurface, session: session)
                }
                terminal.onPasteError = { [weak self] message in self?.showError(message) }
                terminal.onInput = { [weak self] text in self?.enqueue(.input(session, text)) }
                terminal.onWheel = { [weak self] lines, row, col, modifiers, history in self?.enqueue(.wheel(session, lines, row, col, modifiers, history)) }
                terminal.onScroll = { [weak self] lines in self?.enqueue(.scroll(session, lines)) }
                terminal.onResize = { [weak self] rows, cols in self?.enqueue(.resize(session, rows, cols)) }
                terminal.onFocus = { [weak self] in self?.enqueue(.action("focus_pane", pane.id, nil, nil, nil)) }
                terminals[session] = terminal
                view = terminal
            } else {
                let browser = browsers[pane.selectedSurface] ?? BrowserView(surface: pane.selected, owner: self)
                browser.update(pane.selected)
                browsers[pane.selectedSurface] = browser
                view = browser
            }
            view.removeFromSuperview()
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
            return PaneSplitView(key: tree.geometryKey, vertical: axis == "horizontal",
                fraction: splitFractions[tree.geometryKey] ?? 0.5, first: paneView(first), second: paneView(second))
        }
    }

    private func rememberSplits(_ view: NSView) {
        if let split = view as? PaneSplitView { splitFractions[split.geometryKey] = split.fraction }
        view.subviews.forEach(rememberSplits)
    }
    private func initializeSplits(_ view: NSView) {
        (view as? PaneSplitView)?.layoutPanes()
        view.subviews.forEach(initializeSplits)
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

    private var focusedBrowser: BrowserView? {
        guard let pane = focusedPane else { return nil }
        return browsers[pane.selectedSurface]
    }
    @objc private func browserBack() { focusedBrowser?.requestNavigation(.back(snapshotAfter: false)) }
    @objc private func browserForward() { focusedBrowser?.requestNavigation(.forward(snapshotAfter: false)) }
    @objc private func browserReload() { focusedBrowser?.requestNavigation(.reload(snapshotAfter: false)) }
    @objc private func browserZoomIn() { focusedBrowser?.adjustZoom(0.1) }
    @objc private func browserZoomOut() { focusedBrowser?.adjustZoom(-0.1) }
    @objc private func browserZoomReset() { focusedBrowser?.resetZoom() }
    @objc private func browserFind() { focusedBrowser?.findInPage() }
    @objc func newBrowser() {
        guard let pane = focusedPane else { return }
        createBrowser(in: pane.id, url: "about:blank")
    }
    func createBrowser(in pane: UInt64, url: String) { enqueue(.browser("create_browser", nil, pane, url, nil)) }
    func navigateBrowser(_ id: UInt64, url: String) { enqueue(.browser("navigate_browser", id, nil, url, nil)) }
    func observeBrowser(_ id: UInt64, url: String, title: String) { enqueue(.browser("observe_browser", id, nil, url, title)) }
    @objc func focusBrowserAddress() {
        if let pane = focusedPane, let browser = browsers[pane.selectedSurface] { browser.focusAddress() }
        else { newBrowser() }
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
    @objc func nextTab() { navigateTab("next") }
    @objc func previousTab() { navigateTab("previous") }
    @objc func lastTab() { navigateTab("last") }
    private func navigateTab(_ direction: String) {
        guard let pane = focusedPane else { return }
        enqueue(.navigation("navigate_surface", direction, nil, pane.selectedSurface, nil, nil))
    }
    @objc func nextWorkspace() { navigateWorkspace("next") }
    @objc func previousWorkspace() { navigateWorkspace("previous") }
    @objc func lastWorkspace() { navigateWorkspace("last") }
    private func navigateWorkspace(_ direction: String) {
        guard let workspace = snapshot?.workspace else { return }
        enqueue(.navigation("navigate_workspace", direction, nil, nil, workspace.id, nil))
    }
    @objc func nextPane() { navigatePane("next") }
    @objc func previousPane() { navigatePane("previous") }
    @objc func lastPane() { navigatePane("last") }
    private func navigatePane(_ direction: String) {
        guard let pane = focusedPane else { return }
        enqueue(.navigation("navigate_pane", direction, pane.id, nil, nil, nil))
    }
    @objc func focusPaneLeft() { focusPane("left") }
    @objc func focusPaneRight() { focusPane("right") }
    @objc func focusPaneUp() { focusPane("up") }
    @objc func focusPaneDown() { focusPane("down") }
    private func focusPane(_ direction: String) {
        guard let pane = focusedPane else { return }
        enqueue(.navigation("focus_pane_direction", direction, pane.id, nil, nil, nil))
    }
    // Focus history spans the desktop model; never attach the current pane/workspace as a scope.
    @objc func focusBack() { enqueue(.navigation("navigate_focus", "back", nil, nil, nil, nil)) }
    @objc func focusForward() { enqueue(.navigation("navigate_focus", "forward", nil, nil, nil, nil)) }
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
        case #selector(closeTab), #selector(closePane), #selector(renameTab), #selector(splitRight), #selector(splitDown), #selector(splitLeft), #selector(splitUp): return focusedPane != nil
        case #selector(browserBack): return focusedBrowser?.webView.canGoBack == true
        case #selector(browserForward): return focusedBrowser?.webView.canGoForward == true
        case #selector(browserReload), #selector(browserFind), #selector(browserZoomIn), #selector(browserZoomOut), #selector(browserZoomReset): return focusedBrowser != nil
        case #selector(newBrowser), #selector(focusBrowserAddress): return focusedPane != nil
        case #selector(nextTab), #selector(previousTab), #selector(lastTab): return (focusedPane?.surfaces.count ?? 0) > 1
        case #selector(nextWorkspace), #selector(previousWorkspace), #selector(lastWorkspace): return (snapshot?.model.workspaces.count ?? 0) > 1
        case #selector(nextPane), #selector(previousPane), #selector(lastPane), #selector(focusPaneLeft), #selector(focusPaneRight), #selector(focusPaneUp), #selector(focusPaneDown): return (snapshot?.workspace?.root.panes.count ?? 0) > 1
        case #selector(focusBack), #selector(focusForward): return snapshot?.model.focusedWorkspace != nil
        default: return true
        }
    }

    @objc func splitRight() { split("right") }
    @objc func splitDown() { split("down") }
    @objc func splitLeft() { split("left") }
    @objc func splitUp() { split("up") }

    private func split(_ direction: String) {
        if let pane = snapshot?.workspace?.focusedPane {
            enqueue(.navigation("split_pane_directional", direction, pane, nil, nil, true))
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
        if snapshot?.model.workspaces.contains(where: { $0.id == id }) == true {
            enqueue(.action("select_workspace", nil, nil, nil, id))
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
        NSApp.dockTile.badgeLabel = unread > 0 ? String(unread) : nil
        notificationButton.image = NSImage(systemSymbolName: unread > 0 ? "bell.badge.fill" : "bell", accessibilityDescription: "Notifications")
        notificationButton.contentTintColor = unread > 0 ? .systemOrange : .secondaryLabelColor
        notificationButton.toolTip = unread > 0 ? "\(unread) unread notifications" : "Notifications"
        notificationButton.setAccessibilityLabel("Notifications, \(unread) unread")
        guard let list = notificationList else { return }
        let key = snapshot.notifications.map { "\($0.id):\($0.read)" }.joined(separator: ",") + ":\(snapshot.model.revision):\(snapshot.notificationsDropped):\(snapshot.nativeNotifications.epoch)"
        guard key != notificationKey else { return }
        notificationKey = key
        list.arrangedSubviews.forEach { list.removeArrangedSubview($0); $0.removeFromSuperview() }
        if snapshot.notifications.isEmpty {
            let label = NSTextField(wrappingLabelWithString: "No notifications. Terminal bells and explicit notifications appear here.")
            label.textColor = ZmuxPalette.subdued; list.addArrangedSubview(label)
            label.widthAnchor.constraint(equalTo: list.widthAnchor, constant: -24).isActive = true
        }
        for item in snapshot.notifications.reversed() {
            let available = item.target?.isAvailable(in: snapshot) ?? false
            let workspaceName = snapshot.model.workspaces.first { $0.id == item.workspaceId }?.name
            let row = NotificationRowView(item: item, workspaceName: workspaceName, available: available, owner: self)
            list.addArrangedSubview(row)
            row.widthAnchor.constraint(equalTo: list.widthAnchor, constant: -24).isActive = true
        }
        if snapshot.notificationsDropped > 0 {
            let label = NSTextField(labelWithString: "\(snapshot.notificationsDropped) older signals omitted by history limits")
            label.font = .systemFont(ofSize: 10); label.textColor = ZmuxPalette.subdued; list.addArrangedSubview(label)
        }
    }
    func openNotification(_ id: UInt64) {
        guard let target = snapshot?.notifications.first(where: { $0.id == id })?.target else {
            showError("Notification target is no longer available")
            return
        }
        notificationDelivery.openInbox(target)
    }
    @objc private func enableDesktopNotifications() { notificationDelivery.enable() }
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
              let pane = snapshot.workspace?.root.panes.first(where: { $0.id == snapshot.workspace?.focusedPane }), let session = pane.sessionId else { return nil }
        return terminals[session]
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
        try await smokeHistoryNavigation()
        try await smokeClipboardImagePaste()
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
        guard let originalPane = snapshot?.workspace?.root.panes.first, let originalSession = originalPane.sessionId,
              let originalPID = snapshot?.frames.first?.processId else { throw BridgeFailure(message: "Missing original tab") }
        try performShortcut("t", modifiers: [.command])
        try await waitUntil { $0.workspace?.root.panes.first?.surfaces.count == 2 && $0.frames.first?.snapshot?.state == "running" }
        guard snapshot?.workspace?.root.panes.count == 1, snapshot?.frames.first?.processId != originalPID else {
            throw BridgeFailure(message: "New terminal shortcut did not create an independent tab")
        }
        guard let secondTab = focusedPane?.selectedSurface else { throw BridgeFailure(message: "Missing navigation tab") }
        try performShortcut("[", modifiers: [.command, .shift])
        try await waitUntil { $0.workspace?.root.panes.first?.selectedSurface == originalPane.selectedSurface }
        try performShortcut("]", modifiers: [.command, .shift])
        try await waitUntil { $0.workspace?.root.panes.first?.selectedSurface == secondTab }
        lastTab()
        try await waitUntil { $0.workspace?.root.panes.first?.selectedSurface == originalPane.selectedSurface }
        try performShortcut("[", modifiers: [.command])
        try await waitUntil { $0.workspace?.root.panes.first?.selectedSurface == secondTab }
        try performShortcut("]", modifiers: [.command])
        try await waitUntil { $0.workspace?.root.panes.first?.selectedSurface == originalPane.selectedSurface }
        nextTab()
        try await waitUntil { $0.workspace?.root.panes.first?.selectedSurface == secondTab }
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
        enqueue(.input(originalSession, "printf '\\033]777;notify;Agent ready;Review requested\\007'\r"))
        try await waitUntil { $0.notifications.contains { $0.surfaceId == originalPane.selectedSurface && !$0.read } }
        selectTab(originalPane.selectedSurface)
        try await waitUntil { $0.workspace?.root.panes.first?.selectedSurface == originalPane.selectedSurface && $0.notifications.filter { $0.surfaceId == originalPane.selectedSurface }.allSatisfy { $0.read } }
        selectTab(attentionTab)
        try await waitUntil { $0.workspace?.root.panes.first?.selectedSurface == attentionTab }
        enqueue(.input(originalSession, "printf '\\033]777;notify;Agent ready;Second review\\007'\r"))
        try await waitUntil { $0.notifications.contains { $0.surfaceId == originalPane.selectedSurface && !$0.read && $0.body == "Second review" } }
        showNotifications()
        guard sidebarModes?.selectedSegment == 1, !(notificationList?.arrangedSubviews.isEmpty ?? true), !notificationButton.isHidden else { throw BridgeFailure(message: "Notification inbox is unavailable") }
        _ = try await nativeCLI(["notify", "--surface", String(originalPane.selectedSurface), "--title", "CLI review", "--subtitle", "CLI follow up", "--body", "Review queued", "--desktop", "false"])
        try await waitUntil { value in
            value.notifications.contains { $0.surfaceId == originalPane.selectedSurface && $0.title == "CLI review" && $0.subtitle == "CLI follow up" && !$0.read }
        }
        guard let metadata = snapshot?.notifications.last(where: { $0.title == "CLI review" }),
              snapshot?.nativeNotifications.desired.contains(where: { $0.target.notificationId == metadata.id }) == false,
              notificationList?.arrangedSubviews.contains(where: { row in
                  row.subviews.compactMap { $0 as? NSStackView }.flatMap { $0.arrangedSubviews }
                      .compactMap { $0 as? NSTextField }.contains { $0.stringValue == "CLI follow up" }
              }) == true else {
            throw BridgeFailure(message: "Notification metadata did not reach the inbox or disabled desktop delivery")
        }
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
        guard let rightPane = focusedPane else { throw BridgeFailure(message: "No right pane for navigation") }
        try performShortcut(String(UnicodeScalar(NSLeftArrowFunctionKey)!), modifiers: [.command, .option])
        try await waitUntil { $0.workspace?.focusedPane == originalPane.id }
        guard window.firstResponder === focusedTerminal else { throw BridgeFailure(message: "Directional focus did not reach the native terminal") }
        try performShortcut(String(UnicodeScalar(NSRightArrowFunctionKey)!), modifiers: [.command, .option])
        try await waitUntil { $0.workspace?.focusedPane == rightPane.id }
        splitUp()
        try await waitUntil { $0.workspace?.root.panes.count == 3 && $0.frames.allSatisfy { $0.snapshot?.state == "running" } }
        guard let upperPane = focusedPane, let upperSession = upperPane.sessionId,
              let rightSession = rightPane.sessionId,
              let rightPID = snapshot?.frames.first(where: { $0.sessionId == rightSession })?.processId,
              let splitWorkspaceCount = snapshot?.model.workspaces.count else { throw BridgeFailure(message: "No upward split or surviving process") }
        try performShortcut(String(UnicodeScalar(NSDownArrowFunctionKey)!), modifiers: [.command, .option])
        try await waitUntil { $0.workspace?.focusedPane == rightPane.id }
        try performShortcut(String(UnicodeScalar(NSUpArrowFunctionKey)!), modifiers: [.command, .option])
        try await waitUntil { $0.workspace?.focusedPane == upperPane.id }
        closePane()
        try await waitUntil { $0.workspace?.root.panes.count == 2 }
        try performShortcut("t", modifiers: [.command, .shift])
        try await waitUntil {
            $0.workspace?.root.panes.count == 3 && $0.model.closedItems.last?.restoring != true
                && $0.frames.count == 3 && $0.frames.allSatisfy { $0.snapshot?.state == "running" }
        }
        guard let restoredWorkspace = snapshot?.workspace,
              restoredWorkspace.id == first.id, snapshot?.model.workspaces.count == splitWorkspaceCount,
              case .split(let outerAxis, .leaf(let left), .split(let innerAxis, .leaf(let restoredUpper), .leaf(let lower))) = restoredWorkspace.root,
              outerAxis == "horizontal", innerAxis == "vertical",
              left.id == originalPane.id, lower.id == rightPane.id,
              restoredUpper.id != upperPane.id, let restoredSession = restoredUpper.sessionId,
              restoredSession != upperSession, restoredWorkspace.focusedPane == restoredUpper.id,
              snapshot?.frames.first(where: { $0.sessionId == originalSession })?.processId == originalPID,
              snapshot?.frames.first(where: { $0.sessionId == rightSession })?.processId == rightPID else {
            throw BridgeFailure(message: "Reopening a pane lost its original split placement or replaced a surviving process")
        }
        closePane()
        try await waitUntil { $0.workspace?.root.panes.count == 2 && $0.frames.count == 2 }
        focusPaneRight()
        try await waitUntil { $0.workspace?.focusedPane == rightPane.id }
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
        guard let secondWorkspace = snapshot?.workspace?.id else { throw BridgeFailure(message: "Missing second workspace") }
        try await smokeWorkspaceNotification(workspaceID: first.id, foregroundWorkspace: secondWorkspace)
        try performShortcut("[", modifiers: [.command, .control])
        try await waitUntil { $0.model.focusedWorkspace == first.id && $0.frames.count == 2 }
        try performShortcut("]", modifiers: [.command, .control])
        try await waitUntil { $0.model.focusedWorkspace == secondWorkspace }
        lastWorkspace()
        try await waitUntil { $0.model.focusedWorkspace == first.id }
        try performShortcut("[", modifiers: [.command])
        try await waitUntil { $0.model.focusedWorkspace == secondWorkspace }
        try performShortcut("]", modifiers: [.command])
        try await waitUntil { $0.model.focusedWorkspace == first.id && $0.frames.count == 2 }
        closePane()
        try await waitUntil { $0.workspace?.root.panes.count == 1 && $0.frames.count == 1 }
        splitRight()
        try await waitUntil { $0.workspace?.root.panes.count == 2 && $0.frames.count == 2 && $0.frames.allSatisfy { $0.snapshot?.state == "running" } }
        newBrowser()
        try await waitUntil { $0.workspace?.root.panes.contains { $0.selected.kind == "browser" } == true }
        guard let browserPane = focusedPane, let browser = browsers[browserPane.selectedSurface] else { throw BridgeFailure(message: "Embedded browser was not created") }
        browser.webView.loadHTMLString("<!doctype html><title>zmux browser fixture</title><style>body{background:#1c1c1e;color:#eee;font:16px -apple-system;padding:32px}input{font:inherit;padding:8px;border-radius:6px}</style><h1>Browser ready</h1><input id='entry' aria-label='Entry'>", baseURL: nil)
        try await waitUntil { [weak self] _ in self?.browsers[browserPane.selectedSurface]?.webView.title == "zmux browser fixture" && self?.browsers[browserPane.selectedSurface]?.webView.isLoading == false }
        let browserResult = try await browser.webView.evaluateJavaScript("document.querySelector('#entry').value = 'native input'; document.querySelector('h1').textContent + ':' + document.querySelector('#entry').value")
        guard browserResult as? String == "Browser ready:native input" else { throw BridgeFailure(message: "Embedded WebKit did not interact with the document") }
        if let terminalTab = browserPane.surfaces.first(where: { $0.kind == "terminal" }) {
            (canvas.subviews.first as? PaneSplitView)?.setFraction(0.6)
            selectTab(terminalTab.id)
            try await waitUntil { $0.workspace?.root.panes.first(where: { $0.id == browserPane.id })?.selectedSurface == terminalTab.id }
            selectTab(browserPane.selectedSurface)
            try await waitUntil { $0.workspace?.root.panes.first(where: { $0.id == browserPane.id })?.selectedSurface == browserPane.selectedSurface }
            let preserved = try await browser.webView.evaluateJavaScript("document.querySelector('#entry').value")
            guard preserved as? String == "native input" else { throw BridgeFailure(message: "Hidden browser state was reset") }
            guard let split = canvas.subviews.first as? PaneSplitView, abs(split.fraction - 0.6) < 0.01,
                  browser.bounds.width >= canvas.bounds.width * 0.38, browser.bounds.width <= canvas.bounds.width * 0.42 else {
                throw BridgeFailure(message: "User split ratio was lost during tab switching")
            }
            split.setFraction(0.5)
        }
        window.contentView?.layoutSubtreeIfNeeded()
        guard browser.bounds.width >= canvas.bounds.width * 0.3,
              browser.bounds.width <= canvas.bounds.width * 0.7 else {
            throw BridgeFailure(message: "Browser split is cramped: \(browser.bounds.width) of \(canvas.bounds.width)")
        }
        try performShortcut(String(UnicodeScalar(NSLeftArrowFunctionKey)!), modifiers: [.command, .option])
        try await waitUntil { $0.workspace?.focusedPane != browserPane.id }
        guard window.firstResponder === focusedTerminal else { throw BridgeFailure(message: "Browser-to-terminal navigation retained browser input ownership") }
        try performShortcut(String(UnicodeScalar(NSRightArrowFunctionKey)!), modifiers: [.command, .option])
        try await waitUntil { $0.workspace?.focusedPane == browserPane.id }
        guard window.firstResponder === browser.webView else { throw BridgeFailure(message: "Terminal-to-browser navigation did not focus WebKit") }
        try await smokeBrowserAutomation(report: report, surface: browserPane.selectedSurface, pane: browserPane, terminalSession: originalSession)
        let pagePNG: Data = try await withCheckedThrowingContinuation { continuation in
            browser.webView.takeSnapshot(with: nil) { image, error in
                if let error { continuation.resume(throwing: error); return }
                guard let image, let tiff = image.tiffRepresentation,
                      let bitmap = NSBitmapImageRep(data: tiff),
                      let png = bitmap.representation(using: .png, properties: [:]), png.count > 1024 else {
                    continuation.resume(throwing: BridgeFailure(message: "WebKit did not produce usable page pixels")); return
                }
                continuation.resume(returning: png)
            }
        }
        try pagePNG.write(to: report.deletingLastPathComponent().appendingPathComponent("zmux-browser-smoke.png"), options: .atomic)
        enqueue(.input(originalSession, "PS1='❯ '; printf '\\033[2J\\033[H\\033[32mnative terminal ready\\033[0m\\n'\r"))
        try await waitUntil { $0.frames.contains { $0.snapshot?.viewport.contents.components(separatedBy: "\n").contains("native terminal ready") == true } }
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
        try await RemoteClipboardImageSmoke.validate(configuration: report.deletingLastPathComponent().appendingPathComponent("native-ssh-fixture.json"), snapshot: { self.snapshot }, terminal: { self.terminals[$0] })
        let evidence: [String: Any] = [
            "success": true,
            "notification_permission_ordering": true,
            "notification_cli_metadata": true,
            "notification_workspace_target": true,
            "browser_cli_automation": true,
            "browser_never_selected_viewport": true,
            "browser_csp_eval_once": true,
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
            "tab_navigation_shared_actions": true,
            "workspace_navigation_shortcuts": true,
            "directional_pane_focus_shortcuts": true,
            "browser_directional_focus": true,
            "directional_split_up": true,
            "pane_reopen_original_split_anchor": true,
            "global_focus_history_shortcuts": true,
            "sidebar_toggle": true,
            "terminal_wheel_scrollback": true,
            "terminal_keyboard_scrollback": true,
            "terminal_input_returns_live": true,
            "notification_focus_acknowledgement": true,
            "notification_inbox": true,
            "notification_target_routing": true,
            "embedded_webkit": true,
            "browser_hidden_state": true,
            "balanced_browser_split": true,
            "split_ratio_retained": true,
            "browser_render_png_bytes": pagePNG.count,
            "active_terminal_title": true,
            "clipboard_image_preparation": true,
            "clipboard_image_pasteboard": true,
            "clipboard_image_local_paste": true,
            "clipboard_image_remote_transfer": true,
            "clipboard_image_remote_shell_path": true,
            "clipboard_image_remote_source_lifetime": true,
            "clipboard_image_remote_source_cleanup": true,
            "clipboard_image_remote_managed_connection": true,
            "clipboard_image_remote_focus_retained": true,
            "native_cloud_callback": true,
            "bundled_cli_control": true,
            "accessible_icon_controls": true,
            "native_render_png_bytes": png.count,
            "window_capture_status": captureStatus,
            "source_sha": Bundle.main.object(forInfoDictionaryKey: "ZmuxSourceSHA") as? String ?? "local"
        ]
        try JSONSerialization.data(withJSONObject: evidence, options: [.prettyPrinted, .sortedKeys]).write(to: report, options: .atomic)
    }

    private func nativeCLI(_ arguments: [String], expectFailure: Bool = false, timeout: Int = 5000, externalCaller: Bool = false) async throws -> Data {
        let executable = Bundle.main.resourceURL!.appendingPathComponent("bin/zmux")
        return try await Task.detached {
            let process = Process()
            process.executableURL = executable
            process.arguments = ["--timeout", String(timeout)] + arguments
            if externalCaller {
                var environment = ProcessInfo.processInfo.environment
                for key in ["ZMUX_WORKSPACE_ID", "ZMUX_SURFACE_ID", "ZMUX_TAB_ID"] {
                    environment.removeValue(forKey: key)
                }
                process.environment = environment
            }
            let output = Pipe()
            process.standardOutput = output
            process.standardError = output
            try process.run()
            let data = output.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            guard (process.terminationStatus != 0) == expectFailure else {
                throw BridgeFailure(message: "Bundled browser CLI failed its expected outcome: " + String(decoding: data, as: UTF8.self))
            }
            return data
        }.value
    }
    private func browserCLIValue(_ arguments: [String]) async throws -> Any {
        let data = try await nativeCLI(arguments)
        guard let result = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let value = result["value"] else {
            throw BridgeFailure(message: "Browser CLI did not return a typed completion")
        }
        return value
    }
    private func smokeWorkspaceNotification(workspaceID: UInt64, foregroundWorkspace: UInt64) async throws {
        guard let retained = snapshot?.model.workspaces.first(where: { $0.id == workspaceID }),
              snapshot?.model.focusedWorkspace == foregroundWorkspace else {
            throw BridgeFailure(message: "Workspace notification fixture lacks a background workspace")
        }
        let selectedTabs = Dictionary(uniqueKeysWithValues: retained.root.panes.map { ($0.id, $0.selectedSurface) })
        let title = "CLI workspace review"
        _ = try await nativeCLI(["notify", "--workspace", String(workspaceID), "--title", title, "--body", "Open the workspace without changing its selected tab", "--desktop", "false"], externalCaller: true)
        try await waitUntil { $0.notifications.contains { $0.title == title && $0.surfaceId == nil && !$0.read } }
        guard let item = snapshot?.notifications.last(where: { $0.title == title }),
              let target = item.target, case .workspace(let destination) = target.destination,
              destination == workspaceID, item.workspaceId == workspaceID,
              let current = snapshot, target.isAvailable(in: current),
              current.model.focusedWorkspace == foregroundWorkspace,
              !current.nativeNotifications.desired.contains(where: { $0.target.notificationId == item.id }) else {
            throw BridgeFailure(message: "CLI workspace notification borrowed a surface or changed focus")
        }
        showNotifications()
        guard notificationList?.arrangedSubviews.contains(where: { row in
            let children = row.subviews.compactMap { $0 as? NSStackView }.flatMap { $0.arrangedSubviews }
            let fields = children.compactMap { $0 as? NSTextField }
            return fields.contains { $0.stringValue.hasSuffix(title) }
                && fields.contains { $0.stringValue.hasPrefix("Workspace · \(retained.name) · ") }
                && children.compactMap { $0 as? NSButton }.contains { $0.toolTip == "Open workspace" && $0.isEnabled }
        }) == true else {
            throw BridgeFailure(message: "Workspace notification inbox label or open action is unavailable")
        }
        openNotification(item.id)
        try await waitUntil { value in
            guard let workspace = value.workspace, workspace.id == workspaceID,
                  workspace.focusedPane == retained.focusedPane,
                  value.notifications.first(where: { $0.id == item.id })?.read == true else { return false }
            return Dictionary(uniqueKeysWithValues: workspace.root.panes.map { ($0.id, $0.selectedSurface) }) == selectedTabs
        }
        selectWorkspace(foregroundWorkspace)
        try await waitUntil { $0.model.focusedWorkspace == foregroundWorkspace }
        sidebarModes?.selectedSegment = 0
        clearNotificationsButton.isHidden = true
        setSidebarDocument(workspaceList)
    }
    private func smokeBrowserAutomation(report: URL, surface: UInt64, pane: Pane, terminalSession: UInt64) async throws {
        let data = try Data(contentsOf: report.deletingLastPathComponent().appendingPathComponent("native-browser-fixture.json"))
        guard let fixture = try JSONSerialization.jsonObject(with: data) as? [String: String], let origin = fixture["origin"] else {
            throw BridgeFailure(message: "Missing browser HTTP fixture")
        }
        let target = ["--surface", String(surface)]
        let originalWorkspace = snapshot?.model.focusedWorkspace
        let hiddenData = try await nativeCLI(["browser", "open", "--no-focus", origin + "/first"])
        guard let created = try JSONSerialization.jsonObject(with: hiddenData) as? [String: Any],
              let surfaces = created["created_surfaces"] as? [[String: Any]],
              let hiddenID = (surfaces.first?["surface_id"] as? NSNumber)?.uint64Value else {
            throw BridgeFailure(message: "No-focus browser creation omitted its target receipt")
        }
        try await waitUntil { [weak self] _ in self?.browsers[hiddenID]?.webView.title == "First fixture" && self?.browsers[hiddenID]?.webView.isLoading == false }
        guard browsers[hiddenID]?.superview == nil,
              snapshot?.model.focusedWorkspace == originalWorkspace,
              focusedPane?.selectedSurface == surface,
              let box = try await browserCLIValue(["browser", "get", "box", "#entry", "--surface", String(hiddenID)]) as? [String: Any],
              let width = box["width"] as? Double, width > 0,
              let hiddenTree = try await browserCLIValue(["browser", "snapshot", "--interactive", "--surface", String(hiddenID)]) as? [String: Any],
              let hiddenNodes = hiddenTree["nodes"] as? [[String: Any]],
              hiddenNodes.contains(where: { $0["role"] as? String == "button" }) else {
            throw BridgeFailure(message: "Never-selected browser lacks a viewport or stole focus")
        }
        _ = try await nativeCLI(["close-surface", "--surface", String(hiddenID)])
        _ = try await nativeCLI(["browser", "goto"] + target + [origin + "/first"])
        try await waitUntil { [weak self] _ in self?.browsers[surface]?.webView.title == "First fixture" && self?.browsers[surface]?.webView.isLoading == false }
        guard try await browserCLIValue(["browser", "get", "title"] + target) as? String == "First fixture",
              try await browserCLIValue(["browser", "get", "value", "#entry"] + target) as? String == "initial" else {
            throw BridgeFailure(message: "Browser CLI reads did not inspect the live page")
        }
        let evaluated = try await browserCLIValue(["browser", "eval", "({answer: 6 * 7, label: document.title})"] + target) as? [String: Any]
        guard evaluated?["answer"] as? Int == 42, evaluated?["label"] as? String == "First fixture" else {
            throw BridgeFailure(message: "Browser CLI eval lost typed page results")
        }
        let scriptFailure = try await nativeCLI(["browser", "eval", "throw new Error('expected native fixture failure')"] + target, expectFailure: true)
        guard String(decoding: scriptFailure, as: UTF8.self).contains("javascript_failed") else { throw BridgeFailure(message: "Script failure had an unrelated cause") }
        let fakePolicyFailure = try await nativeCLI(["browser", "eval", "document.body.dataset.zmuxEvalCount = String(Number(document.body.dataset.zmuxEvalCount || 0) + 1); throw new EvalError('unsafe-eval Content Security Policy refused')"] + target, expectFailure: true)
        guard String(decoding: fakePolicyFailure, as: UTF8.self).contains("javascript_failed"),
              try await browserCLIValue(["browser", "get", "attr", "body", "data-zmux-eval-count"] + target) as? String == "1" else {
            throw BridgeFailure(message: "Page script was replayed after a CSP-shaped user exception")
        }
        guard let tree = try await browserCLIValue(["browser", "snapshot", "--interactive", "--compact", "--selector", "main", "--max-depth", "3"] + target) as? [String: Any],
              let nodes = tree["nodes"] as? [[String: Any]],
              let reference = nodes.first(where: { $0["role"] as? String == "button" })?["ref"] as? String,
              try await browserCLIValue(["browser", "get", "text", reference] + target) as? String == "Continue" else {
            throw BridgeFailure(message: "Browser CLI snapshot refs did not resolve the live button")
        }
        if let terminal = pane.surfaces.first(where: { $0.kind == "terminal" }) {
            selectTab(terminal.id)
            try await waitUntil { $0.workspace?.root.panes.first(where: { $0.id == pane.id })?.selectedSurface == terminal.id }
            guard try await browserCLIValue(["browser", "get", "title"] + target) as? String == "First fixture",
                  snapshot?.workspace?.root.panes.first(where: { $0.id == pane.id })?.selectedSurface == terminal.id else {
                throw BridgeFailure(message: "Hidden browser CLI changed focus or lost its document")
            }
            selectTab(surface)
            try await waitUntil { $0.workspace?.root.panes.first(where: { $0.id == pane.id })?.selectedSurface == surface }
        }
        _ = try await nativeCLI(["browser", "goto"] + target + [origin + "/second"])
        try await waitUntil { [weak self] _ in self?.browsers[surface]?.webView.title == "Second fixture" && self?.browsers[surface]?.webView.isLoading == false }
        let staleFailure = try await nativeCLI(["browser", "get", "text", reference] + target, expectFailure: true)
        guard String(decoding: staleFailure, as: UTF8.self).contains("stale_reference") else { throw BridgeFailure(message: "Stale reference failed for an unrelated reason") }
        guard let back = try await browserCLIValue(["browser", "back", "--snapshot-after"] + target) as? [String: Any], back["title"] as? String == "First fixture" else {
            throw BridgeFailure(message: "Back snapshot was captured before navigation completed")
        }
        _ = try await nativeCLI(["browser", "forward", "--snapshot-after"] + target)
        _ = try await nativeCLI(["browser", "reload", "--snapshot-after"] + target)
        guard try await browserCLIValue(["browser", "get", "title"] + target) as? String == "Second fixture" else {
            throw BridgeFailure(message: "Browser history/reload lost the current page")
        }
        var pendingFinished = false
        let pending = Task { @MainActor in
            defer { pendingFinished = true }
            return try await nativeCLI(["browser", "eval", "globalThis.zmuxPendingFixture = new Promise(() => {document.title = 'Pending fixture';})"] + target, expectFailure: true, timeout: 3000)
        }
        defer { pending.cancel() }
        try await waitUntil { [weak self] _ in self?.browsers[surface]?.webView.title == "Pending fixture" }
        enqueue(.input(terminalSession, "printf '\\nZMUX_BROWSER_PENDING_INPUT\\n'\r"))
        try await waitUntil { $0.frames.contains { $0.snapshot?.viewport.contents.components(separatedBy: "\n").contains("ZMUX_BROWSER_PENDING_INPUT") == true } }
        guard !pendingFinished else { throw BridgeFailure(message: "Evaluation ended before terminal responsiveness was observed") }
        let timeoutFailure = try await pending.value
        guard String(decoding: timeoutFailure, as: UTF8.self).contains("browser_timeout") else { throw BridgeFailure(message: "Pending evaluation failed for an unrelated reason: \(String(decoding: timeoutFailure.prefix(2048), as: UTF8.self))") }
        guard try await browserCLIValue(["browser", "get", "title"] + target) as? String == "Pending fixture" else {
            throw BridgeFailure(message: "Browser did not recover after an evaluation timeout")
        }
        _ = try await nativeCLI(["browser", "goto"] + target + [origin + "/csp"])
        try await waitUntil { [weak self] _ in self?.browsers[surface]?.webView.title == "CSP fixture" && self?.browsers[surface]?.webView.isLoading == false }
        let cspData = try await nativeCLI(["browser", "eval", "({title:document.title, value:42})"] + target)
        guard let cspResult = try JSONSerialization.jsonObject(with: cspData) as? [String: Any],
              cspResult["content_world"] as? String == "isolated",
              let cspValue = cspResult["value"] as? [String: Any], cspValue["value"] as? Int == 42,
              cspValue["title"] as? String == "CSP fixture" else {
            throw BridgeFailure(message: "CSP evaluation did not report the isolated execution world")
        }
        let noReplay = try await nativeCLI(["browser", "eval", "globalThis.zmuxCspCount = (globalThis.zmuxCspCount || 0) + 1; throw new EvalError('unsafe-eval Content Security Policy')"] + target, expectFailure: true)
        guard String(decoding: noReplay, as: UTF8.self).contains("javascript_failed"),
              try await browserCLIValue(["browser", "eval", "globalThis.zmuxCspCount"] + target) as? Int == 1 else {
            throw BridgeFailure(message: "User script was replayed after a CSP-shaped exception")
        }
    }

    private func smokeClipboardImagePaste() async throws {
        guard let pane = focusedPane, let terminal = focusedTerminal else {
            throw BridgeFailure(message: "No terminal for clipboard image smoke")
        }
        let png = try await ClipboardImageSmoke.validate(pane: pane)
        let pasteboard = NSPasteboard.withUniqueName()
        defer { pasteboard.releaseGlobally() }
        pasteboard.clearContents()
        guard pasteboard.setData(png, forType: .png),
              pasteboard.setString("IMAGE_TEXT_MUST_NOT_BE_PASTED", forType: .string) else {
            throw BridgeFailure(message: "Could not populate the image smoke pasteboard")
        }
        // Exercise the actual view -> asynchronous preparation -> attributable Rust
        // insertion path. The shell independently verifies that the inserted path exists.
        terminal.insertText("test -f ", replacementRange: NSRange(location: NSNotFound, length: 0))
        try await waitUntil { $0.frames.first?.snapshot?.viewport.contents.contains("test -f") == true }
        guard terminal.paste(from: pasteboard) else { throw BridgeFailure(message: "Native image paste was not accepted") }
        try await waitUntil { $0.frames.first?.snapshot?.viewport.contents.contains("ClipboardImages") == true }
        terminal.insertText(" && printf '\\nZMUX_IMAGE_%s\\n' READABLE\r", replacementRange: NSRange(location: NSNotFound, length: 0))
        try await waitUntil { $0.frames.first?.snapshot?.viewport.contents.components(separatedBy: "\n").contains("ZMUX_IMAGE_READABLE") == true }
        guard snapshot?.frames.first?.snapshot?.viewport.contents.contains("IMAGE_TEXT_MUST_NOT_BE_PASTED") == false else {
            throw BridgeFailure(message: "Image paste also inserted the text representation")
        }
    }

    private func smokeHistoryNavigation() async throws {
        guard let terminal = focusedTerminal else { throw BridgeFailure(message: "No terminal for history check") }
        terminal.insertText("i=0; while [ $i -lt 180 ]; do printf 'SCROLL_%03d\\n' $i; i=$((i+1)); done\r", replacementRange: NSRange(location: NSNotFound, length: 0))
        try await waitUntil { $0.frames.first?.snapshot?.viewport.contents.components(separatedBy: "\n").contains("SCROLL_179") == true }
        guard let wheel = CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 1, wheel1: 5, wheel2: 0, wheel3: 0),
              let event = NSEvent(cgEvent: wheel) else { throw BridgeFailure(message: "Could not create wheel event") }
        terminal.scrollWheel(with: event)
        try await waitUntil { $0.frames.first?.snapshot?.viewport.scrollbackOffset ?? 0 > 0 }
        guard let contents = snapshot?.frames.first?.snapshot?.viewport.contents,
              terminal.accessibilityValue() as? String == contents else {
            throw BridgeFailure(message: "Scrolled history did not reach the native view")
        }
        guard let top = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.command], timestamp: 0,
                                         windowNumber: window.windowNumber, context: nil, characters: "", charactersIgnoringModifiers: "", isARepeat: false, keyCode: 115),
              let bottom = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.command], timestamp: 0,
                                            windowNumber: window.windowNumber, context: nil, characters: "", charactersIgnoringModifiers: "", isARepeat: false, keyCode: 119) else {
            throw BridgeFailure(message: "Could not create terminal history keys")
        }
        terminal.keyDown(with: top)
        try await waitUntil { snapshot in
            guard let viewport = snapshot.frames.first?.snapshot?.viewport else { return false }
            return viewport.scrollbackOffset > 0 && viewport.scrollbackOffset == viewport.scrollbackLines
        }
        terminal.keyDown(with: bottom)
        try await waitUntil { $0.frames.first?.snapshot?.viewport.scrollbackOffset == 0 }
        terminal.scrollWheel(with: event)
        try await waitUntil { $0.frames.first?.snapshot?.viewport.scrollbackOffset ?? 0 > 0 }
        terminal.insertText("printf '\\nSCROLL_%s\\n' LIVE\r", replacementRange: NSRange(location: NSNotFound, length: 0))
        try await waitUntil { $0.frames.first?.snapshot?.viewport.scrollbackOffset == 0 && $0.frames.first?.snapshot?.viewport.contents.components(separatedBy: "\n").contains("SCROLL_LIVE") == true }
    }

    private func performShortcut(_ key: String, modifiers: NSEvent.ModifierFlags) throws {
        // AppKit removes command/option/control from charactersIgnoringModifiers, but
        // retains Shift. Supplying lowercase here would synthesize an unshifted shortcut.
        let characters = modifiers.contains(.shift) ? (["[": "{", "]": "}"][key] ?? key.uppercased()) : key
        let keyCodes: [String: UInt16] = [
            "t": 17, "w": 13, "b": 11, "[": 33, "]": 30,
            String(UnicodeScalar(NSLeftArrowFunctionKey)!): 123,
            String(UnicodeScalar(NSRightArrowFunctionKey)!): 124,
            String(UnicodeScalar(NSDownArrowFunctionKey)!): 125,
            String(UnicodeScalar(NSUpArrowFunctionKey)!): 126
        ]
        let keyCode = keyCodes[key] ?? 0
        // A synthetic menu invocation does not run AppKit's normal event-dispatch menu
        // update. Refresh dynamic availability, including newly added close history.
        NSApp.mainMenu?.items.forEach { $0.submenu?.update() }
        NSApp.mainMenu?.update()
        guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers,
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, characters: characters,
            charactersIgnoringModifiers: characters, isARepeat: false, keyCode: keyCode),
            NSApp.mainMenu?.performKeyEquivalent(with: event) == true else {
            throw BridgeFailure(message: "Native menu did not handle shortcut \(key)")
        }
    }

    private func waitUntil(line: UInt = #line, _ predicate: (DesktopSnapshot) -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(20)
        while ContinuousClock.now < deadline {
            if let snapshot, predicate(snapshot) { return }
            try await Task.sleep(for: .milliseconds(32))
        }
        let state = snapshot.map { value in
            let panes = value.model.workspaces.flatMap { $0.root.panes }.map { "pane=\($0.id),selected=\($0.selectedSurface),tabs=" + $0.surfaces.map { "\($0.id):\($0.kind)" }.joined(separator: ",") }.joined(separator: "; ")
            let frames = value.frames.map { "session=\($0.sessionId),pid=\($0.processId.map(String.init) ?? "nil"),state=\($0.snapshot?.state ?? "starting")" }.joined(separator: "; ")
            return "model=\(value.model.revision),focused=\(value.model.focusedWorkspace.map(String.init) ?? "nil"),history=\(value.model.closedItems.count),notifications=\(value.notifications.count), \(panes), \(frames)"
        } ?? "no snapshot"
        throw BridgeFailure(message: "Native smoke condition at line \(line) timed out: \(statusMessage); \(state)")
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
                if CommandLine.arguments.contains("--smoke-test") {
                    // This isolated context must release its control socket before the
                    // normal app context starts; never create two owners for one path.
                    try await NotificationPermissionSmoke.validate()
                }
                try await controller.start(persistent: !CommandLine.arguments.contains("--smoke-test"))
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
                // Teardown has already stopped sessions. Do not offer a misleading
                // Cancel/Retry that would return the user to a dead desktop.
                let alert = NSAlert()
                alert.alertStyle = .warning
                alert.messageText = "zmux could not confirm a clean shutdown"
                alert.informativeText = "The latest session layout may not have been saved. Terminal shutdown has already started.\n\n" + error.localizedDescription
                alert.addButton(withTitle: "Quit")
                alert.runModal()
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

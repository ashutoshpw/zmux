import AppKit
import Darwin
import WebKit

/// Assertion progress only; process exit and shared shutdown remain separate gates.
@MainActor
private final class NativeSmokeStages {
    private struct Evidence: Encodable, Sendable {
        let scope = "native_smoke_assertions"
        let sourceSHA: String
        let appPID: Int32
        var state = "running"
        var currentStage: String?
        var completedStages: [String] = []
        var failure: String?
    }
    private let output: URL
    private var evidence = Evidence(
        sourceSHA: Bundle.main.object(forInfoDictionaryKey: "ZmuxSourceSHA") as? String ?? "local",
        appPID: getpid())

    init(report: URL) { output = report.appendingPathExtension("stages.json") }

    func begin(_ stage: String) async throws {
        guard evidence.currentStage == nil, evidence.completedStages.count < 16 else {
            throw BridgeFailure(message: "Native assertion stages exceeded their bound or overlapped")
        }
        evidence.currentStage = stage
        try await save()
    }
    func complete(_ stage: String) async throws {
        guard evidence.currentStage == stage else {
            throw BridgeFailure(message: "Native assertion stage completion does not match its owner")
        }
        evidence.completedStages.append(stage)
        evidence.currentStage = nil
        try await save()
    }
    func finish() async throws {
        evidence.state = "assertions_complete"
        try await save()
    }
    func fail(_ error: any Error) async {
        evidence.state = "failed"
        evidence.failure = String(String(describing: error).prefix(2048))
        do { try await save() }
        catch { FileHandle.standardError.write(Data(("Native assertion stage receipt failed: " + error.localizedDescription + "\n").utf8)) }
    }
    private func save() async throws {
        let value = evidence
        let output = output
        try await Task.detached {
            let data = try JSONEncoder().encode(value)
            guard data.count <= 16384 else { throw BridgeFailure(message: "Native assertion stage receipt exceeds its bound") }
            try data.write(to: output, options: .atomic)
        }.value
    }
}

/// One main window's presentation. Shared sessions and resource lifetimes belong
/// to its injected coordinator, including views retained while this host is hidden.
@MainActor
final class MainWindowHost: NSObject, NSMenuDelegate, NSMenuItemValidation, NSWindowDelegate {
    let window: SidebarFocusWindow
    var cloudWindow: NSWindowController?
    private(set) var snapshot: DesktopSnapshot?

    private unowned let coordinator: ApplicationCoordinator
    private(set) var identity: MainWindowIdentity?
    private(set) var nativeToken: WindowHostToken?
    private(set) var windowUUID: UUID?
    var nativeReady = false
    private var authorizedClose = false
    private var bridge: NativeBridge { coordinator.bridge }
    private var browserDownloads: BrowserDownloads { coordinator.browserDownloads }
    private var notificationDelivery: NotificationDelivery { coordinator.notificationDelivery }
    private var imagePastes: ClipboardImagePastes { coordinator.imagePastes }
    private var terminals: [UInt64: TerminalView] { coordinator.terminals }
    private var completedHistories: [UInt64: CompletedTerminalHistoryView] { coordinator.completedHistories }
    private var browsers: [UInt64: BrowserView] { coordinator.browsers }
    var browserViewportSize: NSSize { canvas.bounds.size }
    private let sidebarContainer = BackgroundView(color: ZmuxPalette.sidebar)
    private let sidebarPanel = BackgroundView(color: ZmuxPalette.sidebar)
    private var sidebarLayout = SidebarLayout()
    private var sidebarWidthKey = "sidebarWidth"
    private var sidebarCollapsedKey = "sidebarCollapsed"
    private let sidebarDivider = SidebarDividerView()
    private var sidebarWidth: NSLayoutConstraint?
    private var sidebarPanelWidth: NSLayoutConstraint?
    private var sidebarCollapsed = UserDefaults.standard.bool(forKey: "sidebarCollapsed")
    private var sidebarScroll: NSScrollView?
    private var sidebarDocumentConstraints: [NSLayoutConstraint] = []
    private var sidebarModes: NSSegmentedControl?
    private lazy var clearNotificationsButton = IconButton(symbol: "trash", label: "Clear displayed notifications", target: self, action: #selector(clearNotifications))
    private lazy var jumpToUnreadButton = IconButton(symbol: "arrow.up.forward.circle", label: "Jump to latest unread (⌘⇧U)", target: self, action: #selector(jumpToUnread))
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
    private var layoutKey = ""
    private var modelRevision: UInt64 = .max
    private var workspaceRows: [WorkspaceRowView: UInt64] = [:]
    private var workspaceRowsNamespace: UUID?
    private var metadataPopover: NSPopover?
    private var metadataInspector: WorkspaceMetadataViewController?
    private let recentlyClosedMenu = NSMenu(title: "Recently Closed")
    private let browserProfilesMenu = NSMenu(title: "Browser Profiles")
    private var browserProfilesWindow: BrowserProfilesWindowController?
    private var workspaceCount = 1
    private var didSetInitialTerminalFocus = false
    private var detachedContentResponder = false
    private var pendingBrowserFocus: UInt64?

    init(coordinator: ApplicationCoordinator) {
        self.coordinator = coordinator
        window = SidebarFocusWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1120, height: 720),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        super.init()
        window.delegate = self
        window.onWorkspaceNumber = { [weak self] digit in self?.selectWorkspaceNumber(digit) }
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
    }

    func bind(_ token: WindowHostToken, windowUUID: UUID, inheritLegacy: Bool) {
        nativeToken = token
        self.windowUUID = windowUUID
        let uuid = token.identityNamespace.uuidString.lowercased() + "." + windowUUID.uuidString.lowercased()
        sidebarWidthKey = "window." + uuid + ".sidebarWidth"
        sidebarCollapsedKey = "window." + uuid + ".sidebarCollapsed"
        sidebarLayout = SidebarLayout(key: sidebarWidthKey, inheritLegacy: inheritLegacy)
        let saved = UserDefaults.standard.object(forKey: sidebarCollapsedKey) as? Bool
        sidebarCollapsed = saved ?? (inheritLegacy && UserDefaults.standard.bool(forKey: "sidebarCollapsed"))
        applySidebarLayout()
    }
    private lazy var windowMenu = makeMenus()
    func installMenus() {
        // A menu belongs to one tree. Key-window changes must switch the retained
        // tree, not attach its persistent submenus to newly constructed parents.
        if NSApp.mainMenu !== windowMenu { NSApp.mainMenu = windowMenu }
    }
    func closeAuthorized() { authorizedClose = true; nativeReady = false; window.close(); finishPresentation() }
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard !authorizedClose else { return true }
        if let identity { coordinator.enqueue(.window("close", identity.windowId, nil)) }
        return false
    }
    func detachLayoutIfNeeded(_ snapshot: DesktopSnapshot, identity: MainWindowIdentity) {
        guard layoutKey != presentationKey(for: snapshot) || self.identity != identity || self.snapshot?.browserEpoch != snapshot.browserEpoch else { return }
        releaseContentResponder()
        canvas.subviews.forEach { rememberSplits($0); $0.removeFromSuperview() }
        layoutKey = ""
        didSetInitialTerminalFocus = false
    }
    private func releaseContentResponder() {
        // Capture before detaching shared views, including the window's field
        // editor, which need not be a descendant of the browser's own view.
        let browserOwned = browsers.values.contains { $0.rememberOwnedFocus(in: window) }
        let contentOwned = (window.firstResponder as? NSView)?.isDescendant(of: canvas) == true
        detachedContentResponder = browserOwned || contentOwned
        if detachedContentResponder { window.makeFirstResponder(nil) }
    }
    func detachContent() {
        releaseContentResponder()
        canvas.subviews.forEach { rememberSplits($0); $0.removeFromSuperview() }
        layoutKey = ""
    }

    func finishPresentation() {
        metadataPopover?.performClose(nil)
        metadataPopover = nil; metadataInspector = nil
    }
    func refreshSharedStatus(_ snapshot: DesktopSnapshot) {
        guard let current = self.snapshot, current.revision == snapshot.revision else { return }
        updateHeader(current)
    }

    func updateCloudStatus(_ label: String, connected: Bool) {
        cloudFooter.update(label: label, connected: connected)
    }

    private func enqueue(_ command: UICommand) {
        coordinator.enqueue(command)
    }

    func clearErrorForCommand() { presentationError = nil }

    func showError(_ message: String) {
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
        let sidebarActions = NSStackView(views: [newWorkspaceButton, jumpToUnreadButton, clearNotificationsButton, cloudButton])
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

        for view in [sidebarContainer, sidebarPanel, sidebarChrome, sidebarSeparator, rightSeparator, modes, sidebarActions, scroll, cloudFooter, mainHeader, canvas, sidebarDivider] {
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
        content.addSubview(sidebarDivider)
        sidebarDivider.onResize = { [weak self] width in
            guard let self, !self.sidebarCollapsed else { return }
            self.sidebarLayout.resize(to: width, availableWidth: self.window.contentView?.bounds.width ?? 0)
            self.applySidebarLayout()
        }

        for button in actionButtons + [notificationButton, jumpToUnreadButton, clearNotificationsButton] {
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
            sidebarDivider.centerXAnchor.constraint(equalTo: sidebarContainer.trailingAnchor),
            sidebarDivider.topAnchor.constraint(equalTo: sidebarChrome.topAnchor),
            sidebarDivider.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            sidebarDivider.widthAnchor.constraint(equalToConstant: 8),

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
        sidebarPanelWidth = sidebarPanel.widthAnchor.constraint(equalToConstant: 224)
        sidebarPanelWidth?.isActive = true
        sidebarWidth = sidebarContainer.widthAnchor.constraint(equalToConstant: 0)
        sidebarWidth?.isActive = true
        applySidebarLayout()
        setNotificationActionsVisible(false)
        setSidebarDocument(workspaceList)
        window.autorecalculatesKeyViewLoop = true
    }

    @objc func toggleSidebar() {
        sidebarCollapsed.toggle()
        UserDefaults.standard.set(sidebarCollapsed, forKey: sidebarCollapsedKey)
        if sidebarCollapsed, let responder = window.firstResponder as? NSView,
           responder === sidebarDivider || responder.isDescendant(of: sidebarPanel) {
            // A hidden sidebar cannot keep keyboard focus. Preserve content
            // responders otherwise, including a browser address field/editor.
            if let terminal = focusedTerminal { window.makeFirstResponder(terminal) }
            else if let surface = focusedPane?.selectedSurface, let history = completedHistories[surface] {
                window.makeFirstResponder(history.outputResponder)
            }
            else if let pane = focusedPane, let browser = browsers[pane.selectedSurface] {
                window.makeFirstResponder(browser.webView)
            }
            else { window.makeFirstResponder(nil) }
        }
        applySidebarLayout()
    }

    private func applySidebarLayout() {
        guard let content = window.contentView else { return }
        let width = sidebarLayout.visibleWidth(availableWidth: content.bounds.width)
        sidebarPanelWidth?.constant = width
        sidebarWidth?.constant = sidebarCollapsed ? 0 : width
        sidebarPanel.isHidden = sidebarCollapsed
        sidebarDivider.isHidden = sidebarCollapsed
        sidebarDivider.update(width: width)
        content.layoutSubtreeIfNeeded()
    }

    func windowDidResize(_ notification: Notification) { applySidebarLayout() }

    func appActive(_ active: Bool) {
        coordinator.appActive(active)
    }
    func windowDidBecomeKey(_ notification: Notification) { coordinator.windowKeyChanged(self, key: true) }
    func windowDidResignKey(_ notification: Notification) { coordinator.windowKeyChanged(self, key: false) }

    private func makeIconButton(symbol: String, label: String, action: Selector) -> IconButton {
        let button = IconButton(symbol: symbol, label: label, target: self, action: action)
        actionButtons.append(button)
        return button
    }

    private func makeMenus() -> NSMenu {
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
            ("New Window", #selector(newMainWindow), ""),
            ("Close Window", #selector(closeMainWindow), ""),
            ("New Workspace", #selector(newWorkspace), "n"),
            ("New Terminal Tab", #selector(newTerminal), "t"),
            ("New Browser Tab", #selector(newBrowser), ""),
            ("Focus Browser Address", #selector(focusBrowserAddress), "l"),
            ("Toggle Sidebar", #selector(toggleSidebar), "b"),
            ("Notifications", #selector(showNotifications), ""),
            ("Jump to Latest Unread", #selector(jumpToUnread), "U"),
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
        let profilesItem = NSMenuItem(title: "New Browser Tab with Profile", action: nil, keyEquivalent: "")
        profilesItem.submenu = browserProfilesMenu
        browserProfilesMenu.delegate = self
        workspaceMenu.insertItem(profilesItem, at: 3)
        // AppKit matches the resulting character, as it does for uppercase T/W.
        // Shift-brackets produce braces on the standard keyboard layout.
        let next = workspaceMenu.addItem(withTitle: "Next Tab", action: #selector(nextTab), keyEquivalent: "}")
        next.keyEquivalentModifierMask = [.command]; next.target = self
        let previous = workspaceMenu.addItem(withTitle: "Previous Tab", action: #selector(previousTab), keyEquivalent: "{")
        previous.keyEquivalentModifierMask = [.command]; previous.target = self
        let navigationItems: [(String, Selector, String, NSEvent.ModifierFlags)] = [
            ("Last Tab", #selector(lastTab), "", NSEvent.ModifierFlags.command),
            ("Move Tab Left", #selector(moveTabLeft), "{", [.command, .option]),
            ("Move Tab Right", #selector(moveTabRight), "}", [.command, .option]),
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
        workspaceMenu.addItem(.separator())
        for digit in 1...9 {
            let title = digit == 9 ? "Last Workspace in Sidebar" : "Workspace \(digit)"
            let item = workspaceMenu.addItem(withTitle: title, action: #selector(selectWorkspaceNumberFromMenu(_:)), keyEquivalent: String(digit))
            item.keyEquivalentModifierMask = [.command]
            item.target = self
            item.tag = digit
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
            ("Manage Profiles…", #selector(showBrowserProfiles(_:)), ""),
            ("Downloads…", #selector(showBrowserDownloads(_:)), ""),
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
        return menu
    }

    func present(_ snapshot: DesktopSnapshot, identity: MainWindowIdentity?) -> (selectionChanged: Bool, layoutChanged: Bool) {
        let selectionChanged = self.snapshot?.model.focusedWorkspace != snapshot.model.focusedWorkspace
            || self.snapshot?.workspace?.focusedPane != snapshot.workspace?.focusedPane
            || focusedPane?.selectedSurface != snapshot.workspace?.root.panes.first(where: { $0.id == snapshot.workspace?.focusedPane })?.selectedSurface
        let layoutChanged = layoutKey != presentationKey(for: snapshot)
        if self.snapshot?.browserEpoch != snapshot.browserEpoch || self.identity != identity { layoutKey = "" }
        self.identity = identity
        self.snapshot = snapshot
        browserProfilesWindow?.update(snapshot.browserProfiles)
        statusMessage = snapshot.lastError ?? "Ready"
        rebuildWorkspaceRowsIfNeeded(snapshot)
        updateWorkspaceRows(snapshot)
        updateHeader(snapshot)
        updateNotifications(snapshot)
        updateTerminalLayoutIfNeeded(snapshot)

        updateTabAttention(canvas, snapshot: snapshot)
        return (selectionChanged, layoutChanged)
    }

    func completePresentation(selectionChanged: Bool, layoutChanged: Bool) {
        defer { detachedContentResponder = false }
        guard let snapshot else { return }
        if let pane = snapshot.workspace?.root.panes.first(where: { $0.id == snapshot.workspace?.focusedPane }),
           let session = pane.sessionId, let terminal = terminals[session] {
            pendingBrowserFocus = nil
            if selectionChanged || !didSetInitialTerminalFocus {
                window.makeFirstResponder(terminal)
                didSetInitialTerminalFocus = true
            } else if let responder = window.firstResponder as? TerminalView, responder !== terminal {
                window.makeFirstResponder(terminal)
            }
        } else if let pane = focusedPane, let history = completedHistories[pane.selectedSurface] {
            pendingBrowserFocus = nil
            if selectionChanged || !didSetInitialTerminalFocus {
                window.makeFirstResponder(history.outputResponder)
                didSetInitialTerminalFocus = true
            }
        } else if let pane = focusedPane, let browser = browsers[pane.selectedSurface] {
            if selectionChanged || (layoutChanged && detachedContentResponder) {
                pendingBrowserFocus = pane.selectedSurface
            }
            if pendingBrowserFocus == pane.selectedSurface, window.isKeyWindow,
               browser.restorePreferredFocus() {
                pendingBrowserFocus = nil
            }
        } else { pendingBrowserFocus = nil }
    }

    private func updateTabAttention(_ view: NSView, snapshot: DesktopSnapshot) {
        if let bar = view as? TerminalTabBar { bar.updateAttention(snapshot.notifications, surfaces: snapshot.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }) }
        for child in view.subviews { updateTabAttention(child, snapshot: snapshot) }
    }

    private func rebuildWorkspaceRowsIfNeeded(_ snapshot: DesktopSnapshot) {
        guard modelRevision != snapshot.model.revision || workspaceRowsNamespace != snapshot.model.identityNamespace else { return }
        modelRevision = snapshot.model.revision
        workspaceRowsNamespace = snapshot.model.identityNamespace
        workspaceList.arrangedSubviews.forEach {
            workspaceList.removeArrangedSubview($0)
            $0.removeFromSuperview()
        }
        workspaceRows.removeAll()
        for workspace in snapshot.model.workspaces {
            let row = WorkspaceRowView(workspaceID: workspace.id)
            row.target = self
            row.action = #selector(workspaceSelected(_:))
            let namespace = snapshot.model.identityNamespace
            let workspaceID = workspace.id
            row.onMetadata = { [weak self] source in
                self?.showWorkspaceMetadata(workspaceID, namespace: namespace, from: source)
            }
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
            row.update(title: workspace.name, subtitle: subtitle, isSelected: isSelected, isRunning: isRunning, unread: unread,
                       metadata: snapshot.workspaceMetadata?.first { $0.workspaceId == workspaceID })
        }
        if let inspector = metadataInspector {
            guard metadataPopover?.isShown == true, inspector.identityNamespace == snapshot.model.identityNamespace,
                  let workspace = snapshot.model.workspaces.first(where: { $0.id == inspector.workspaceID }) else {
                metadataPopover?.performClose(nil)
                metadataPopover = nil; metadataInspector = nil
                return
            }
            inspector.update(snapshot.workspaceMetadata?.first { $0.workspaceId == inspector.workspaceID } ?? .empty(inspector.workspaceID), name: workspace.name)
        }
    }

    private func showWorkspaceMetadata(_ workspaceID: UInt64, namespace: UUID, from source: NSView) {
        guard source.window === window, let snapshot, snapshot.model.identityNamespace == namespace,
              let workspace = snapshot.model.workspaces.first(where: { $0.id == workspaceID }) else { return }
        metadataPopover?.performClose(nil)
        let inspector = WorkspaceMetadataViewController(workspaceID: workspaceID, identityNamespace: namespace)
        inspector.update(snapshot.workspaceMetadata?.first { $0.workspaceId == workspaceID } ?? .empty(workspaceID), name: workspace.name)
        let popover = NSPopover()
        popover.contentViewController = inspector
        popover.contentSize = NSSize(width: 420, height: 460)
        popover.behavior = .transient
        metadataInspector = inspector; metadataPopover = popover
        popover.show(relativeTo: source.bounds, of: source, preferredEdge: .maxX)
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
        } else if let completion = focusedPane?.selected.localZellijCompletion {
            detail = completion.receipt.statusLabel
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

    private func presentationKey(for snapshot: DesktopSnapshot) -> String {
        // A selected persistent browser can acquire its native store after the
        // model selection is published. Replace its pending placeholder when
        // that exact native host becomes ready, or when its host is replaced.
        let hosts = snapshot.workspace?.root.panes.filter { $0.selected.kind == "browser" }.map { pane in
            let host = browsers[pane.selectedSurface]?.hostID.uuidString ?? "pending"
            return "\(pane.id):\(pane.selectedSurface):\(host)"
        }.joined(separator: ",") ?? ""
        return (snapshot.workspace?.root.key ?? "empty") + "|browser-hosts:" + hosts
    }

    private func updateTerminalLayoutIfNeeded(_ snapshot: DesktopSnapshot) {
        let key = presentationKey(for: snapshot)
        guard key != layoutKey else { return }
        layoutKey = key
        didSetInitialTerminalFocus = false
        canvas.subviews.forEach { rememberSplits($0); $0.removeFromSuperview() }
        if let workspace = snapshot.workspace {
            let root = paneView(workspace.root, namespace: snapshot.model.identityNamespace)
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
    }

    private func paneView(_ tree: PaneTree, namespace: UUID) -> NSView {
        switch tree {
        case .leaf(let pane):
            let view: NSView
            if let session = pane.sessionId {
                let terminal = coordinator.terminal(session: session, pane: pane)
                terminal.onFiles = { [weak self] paths in self?.enqueue(.pasteFiles(pane.selectedSurface, paths)) }
                terminal.onImage = { [weak self] data in
                    guard let self else { return false }
                    self.presentationError = nil
                    return self.imagePastes.paste(data, surface: pane.selectedSurface, session: session)
                }
                terminal.onPasteError = { [weak self] message in self?.showError(message) }
                terminal.onInput = { [weak self] text in self?.enqueue(.input(session, text)) }
                terminal.onKey = { [weak self] event in self?.enqueue(.key(session, event)) }
                terminal.onPaste = { [weak self] text in self?.enqueue(.paste(session, text)) }
                terminal.onWheel = { [weak self] lines, row, col, modifiers, history in self?.enqueue(.wheel(session, lines, row, col, modifiers, history)) }
                terminal.onPointer = { [weak self] event in self?.coordinator.enqueue(.pointer(session, event)) ?? false }
                terminal.onScroll = { [weak self] lines in self?.enqueue(.scroll(session, lines)) }
                terminal.onResize = { [weak self] rows, cols in self?.enqueue(.resize(session, rows, cols)) }
                terminal.onFocus = { [weak self] in self?.enqueue(.action("focus_pane", pane.id, nil, nil, nil)) }
                view = terminal
            } else if let completion = pane.selected.localZellijCompletion {
                let focus: () -> Void = { [weak self] in
                    self?.enqueue(.action("focus_pane", pane.id, nil, nil, nil))
                }
                view = coordinator.completedHistory(surface: pane.selectedSurface, history: completion, onFocus: focus)
            } else {
                // Reconciliation owns profile validation, datastore selection and
                // automation binding. Presentation must never bypass that owner.
                if let browser = browsers[pane.selectedSurface], browser.profileID == pane.selected.browserProfileId {
                    browser.update(pane.selected)
                    view = browser
                } else {
                    let unavailable = NSTextField(wrappingLabelWithString: "This browser could not be opened. Check its profile and try again.")
                    unavailable.textColor = .secondaryLabelColor
                    unavailable.alignment = .center
                    view = unavailable
                }
            }
            view.removeFromSuperview()
            let container = BackgroundView(color: ZmuxPalette.terminal)
            let tabs = TerminalTabBar(pane: pane, namespace: namespace, owner: self)
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
            let workspace = snapshot?.workspace?.id
            let split = PaneSplitView(key: tree.geometryKey, vertical: axis == "horizontal",
                fraction: workspace.map { coordinator.splitLayouts.fraction(workspace: $0, split: tree.geometryKey) } ?? 0.5,
                first: paneView(first, namespace: namespace), second: paneView(second, namespace: namespace))
            split.onFractionChanged = { [weak self] fraction in
                guard let self, let workspace else { return }
                self.coordinator.splitLayouts.remember(workspace: workspace, split: tree.geometryKey, fraction: fraction)
            }
            return split
        }
    }

    private func rememberSplits(_ view: NSView) {
        if let split = view as? PaneSplitView, let workspace = snapshot?.workspace?.id {
            coordinator.splitLayouts.remember(workspace: workspace, split: split.geometryKey, fraction: split.fraction)
        }
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
        if let shared = coordinator.snapshot,
           let workspaceUUID = shared.workspaceIdentities.first(where: { $0.workspaceId == workspace.id })?.uuid {
            if let owner = shared.model.windows.first(where: { $0.workspaceIds.contains(workspace.id) }),
               let index = owner.workspaceIds.firstIndex(of: workspace.id),
               let windowUUID = coordinator.mainWindowHosts[MainWindowIdentity(namespace: shared.model.identityNamespace, windowId: owner.id)]?.windowUUID {
                let arrange = NSMenu(title: "Reorder Workspace")
                arrange.autoenablesItems = false
                for (title, destination) in [("Move Up", index - 1), ("Move Down", index + 1), ("Move to Top", 0), ("Move to Bottom", owner.workspaceIds.count - 1)] {
                    let item = arrange.addItem(withTitle: title, action: #selector(reorderWorkspaceFromMenu(_:)), keyEquivalent: "")
                    item.target = self
                    item.isEnabled = destination >= 0 && destination < owner.workspaceIds.count && destination != index
                    item.representedObject = WorkspaceOrderTarget(namespace: shared.model.identityNamespace, workspace: workspaceUUID, window: windowUUID, index: destination, revision: shared.model.revision)
                }
                menu.addItem(withTitle: "Reorder Workspace", action: nil, keyEquivalent: "").submenu = arrange
            }
            let move = NSMenu(title: "Move to Window")
            for (index, window) in shared.model.windows.enumerated() where !window.workspaceIds.contains(workspace.id) {
                guard let destination = coordinator.mainWindowHosts[MainWindowIdentity(namespace: shared.model.identityNamespace, windowId: window.id)]?.windowUUID else { continue }
                for focus in [true, false] {
                    let title = "Window \(index + 1)" + (focus ? "" : " Without Focusing")
                    let item = move.addItem(withTitle: title, action: #selector(moveWorkspaceFromMenu(_:)), keyEquivalent: "")
                    item.target = self
                    item.representedObject = WorkspaceMoveTarget(namespace: shared.model.identityNamespace, workspace: workspaceUUID, destination: destination, focus: focus)
                }
            }
            if !move.items.isEmpty { menu.addItem(withTitle: "Move to Window", action: nil, keyEquivalent: "").submenu = move }
        }
        let close = menu.addItem(withTitle: "Close Workspace", action: #selector(closeWorkspaceFromMenu(_:)), keyEquivalent: "")
        close.target = self
        close.representedObject = NSNumber(value: workspace.id)
        return menu
    }

    @objc func newWorkspace() {
        guard let identity else { showError("Window startup is still pending"); return }
        workspaceCount += 1
        enqueue(.workspaceCreate(identity.windowId, "Workspace \(workspaceCount)"))
    }
    @objc private func reorderWorkspaceFromMenu(_ sender: NSMenuItem) {
        guard let target = sender.representedObject as? WorkspaceOrderTarget,
              coordinator.snapshot?.model.identityNamespace == target.namespace else { return }
        enqueue(.workspaceOrder(target.workspace, target.window, target.index, target.revision))
    }
    @objc private func moveWorkspaceFromMenu(_ sender: NSMenuItem) {
        guard let target = sender.representedObject as? WorkspaceMoveTarget,
              coordinator.snapshot?.model.identityNamespace == target.namespace else { return }
        enqueue(.workspaceMove(target.workspace, target.destination, target.focus))
    }
    @objc private func newMainWindow() { enqueue(.window("new", nil, true)) }
    @objc private func closeMainWindow() { if let identity { enqueue(.window("close", identity.windowId, nil)) } }

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
    @objc private func browserZoomIn() { focusedBrowser?.requestZoom(.in) }
    @objc private func browserZoomOut() { focusedBrowser?.requestZoom(.out) }
    @objc private func browserZoomReset() { focusedBrowser?.requestZoom(.reset) }
    @objc private func browserFind() { focusedBrowser?.findInPage() }
    @objc func newBrowser() {
        guard let pane = focusedPane else { return }
        createBrowser(in: pane.id, url: "about:blank")
    }
    @objc private func newBrowserWithProfile(_ item: NSMenuItem) {
        guard let pane = focusedPane, let profile = profileForMenuItem(item) else {
            showError("Browser profile is unavailable")
            return
        }
        createBrowser(in: pane.id, url: "about:blank", profile: profile)
    }
    @objc private func showBrowserDownloads(_ sender: Any?) { browserDownloads.showWindow(sender) }
    @objc private func showBrowserProfiles(_ sender: Any?) {
        if browserProfilesWindow == nil {
            let bridge = self.bridge
            browserProfilesWindow = BrowserProfilesWindowController(create: { name in
                try await bridge.createBrowserProfile(name: name)
            }, rename: { id, name in
                try await bridge.renameBrowserProfile(id: id, name: name)
            }, reconcile: { profile, intent in
                try await bridge.reconcileBrowserProfileData(profile: profile, intent: intent)
            }, changeData: { id, delete in
                try await bridge.changeBrowserProfileData(id: id, delete: delete)
            }, open: { [weak self] profile in
                guard let self, let pane = self.focusedPane,
                      let state = self.snapshot?.browserProfiles, state.enabled,
                      !state.loading, state.error == nil,
                      state.profiles.contains(where: { $0.id == profile }) else {
                    throw BridgeFailure(message: "The selected browser profile is unavailable.")
                }
                self.createBrowser(in: pane.id, url: "about:blank", profile: profile)
                self.window.makeKeyAndOrderFront(nil)
            })
        }
        if let state = snapshot?.browserProfiles { browserProfilesWindow?.update(state) }
        browserProfilesWindow?.showWindow(sender)
    }
    private func profileForMenuItem(_ item: NSMenuItem) -> UUID? {
        guard let id = item.representedObject as? String, let profile = UUID(uuidString: id),
              let state = snapshot?.browserProfiles, !state.loading, state.error == nil,
              state.profiles.contains(where: { $0.id == profile }) else { return nil }
        return profile
    }
    func createBrowser(in pane: UInt64, url: String, profile: UUID? = nil) { enqueue(.browser("create_browser", nil, pane, url, nil, profile)) }
    func navigateBrowser(_ id: UInt64, url: String) { enqueue(.browser("navigate_browser", id, nil, url, nil, nil)) }
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
    @objc func reopenClosed() { if let identity { enqueue(.windowReopen(identity.windowId, nil)) } }
    @objc private func reopenHistoryItem(_ sender: NSMenuItem) {
        guard let id = sender.representedObject as? NSNumber else { return }
        if let identity { enqueue(.windowReopen(identity.windowId, id.uint64Value)) }
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
    @objc func focusBack() { if let identity { enqueue(.windowHistory(identity.windowId, "back")) } }
    @objc func focusForward() { if let identity { enqueue(.windowHistory(identity.windowId, "forward")) } }
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
    @objc func moveTabLeft() { moveFocusedTab(offset: -1) }
    @objc func moveTabRight() { moveFocusedTab(offset: 1) }
    private func moveFocusedTab(offset: Int) {
        guard let snapshot, let pane = focusedPane else { return }
        moveTab(pane.selectedSurface, in: pane.id, namespace: snapshot.model.identityNamespace, offset: offset)
    }
    func moveTab(_ id: UInt64, in paneID: UInt64, namespace: UUID, offset: Int) {
        guard surfaceDragRevision(id, in: paneID, namespace: namespace) != nil else { return }
        enqueue(.surfaceOrder(namespace, id, paneID, offset, true, nil))
    }
    func surfaceDragRevision(_ id: UInt64, in paneID: UInt64, namespace: UUID) -> UInt64? {
        guard let snapshot, snapshot.model.identityNamespace == namespace,
              snapshot.workspace?.root.panes.contains(where: { $0.id == paneID && $0.surfaces.contains(where: { $0.id == id }) }) == true else { return nil }
        return snapshot.model.revision
    }
    func reorderTab(_ id: UInt64, from sourcePane: UInt64, in paneID: UInt64, namespace: UUID, revision: UInt64, to index: Int) {
        guard surfaceDragRevision(id, in: sourcePane, namespace: namespace) == revision else { return }
        if sourcePane == paneID {
            enqueue(.surfaceOrder(namespace, id, paneID, index, false, revision))
        } else {
            enqueue(.surface("move_surface", id, paneID, index, nil))
        }
    }
    func menuNeedsUpdate(_ menu: NSMenu) {
        if menu === browserProfilesMenu {
            menu.removeAllItems()
            if let state = snapshot?.browserProfiles, !state.loading, state.error == nil {
                for profile in state.profiles {
                    let item = menu.addItem(withTitle: profile.name, action: #selector(newBrowserWithProfile(_:)), keyEquivalent: "")
                    item.target = self
                    item.representedObject = profile.id.uuidString
                    item.toolTip = "Open a browser tab using this profile’s cookies and website data"
                }
            }
            if menu.items.isEmpty {
                let title = snapshot?.browserProfiles.loading == true ? "Loading profiles…" : "No browser profiles available"
                let item = menu.addItem(withTitle: title, action: nil, keyEquivalent: "")
                item.isEnabled = false
            }
            return
        }
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
        case #selector(newBrowserWithProfile(_:)):
            return focusedPane != nil && profileForMenuItem(item) != nil
        case #selector(moveTabLeft), #selector(moveTabRight), #selector(nextTab), #selector(previousTab), #selector(lastTab): return (focusedPane?.surfaces.count ?? 0) > 1
        case #selector(nextWorkspace), #selector(previousWorkspace), #selector(lastWorkspace): return (snapshot?.model.workspaces.count ?? 0) > 1
        case #selector(nextPane), #selector(previousPane), #selector(lastPane), #selector(focusPaneLeft), #selector(focusPaneRight), #selector(focusPaneUp), #selector(focusPaneDown): return (snapshot?.workspace?.root.panes.count ?? 0) > 1
        case #selector(focusBack), #selector(focusForward): return snapshot?.model.focusedWorkspace != nil
        case #selector(jumpToUnread): return canJumpToUnread
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

    private func selectWorkspaceNumber(_ digit: Int) {
        guard (1...9).contains(digit), nativeReady, NSApp.keyWindow === window, let windowUUID,
              let identity, coordinator.snapshot?.model.identityNamespace == identity.namespace else { return }
        enqueue(.workspaceNumber(windowUUID, digit))
    }
    @objc private func selectWorkspaceNumberFromMenu(_ sender: NSMenuItem) {
        selectWorkspaceNumber(sender.tag)
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
        else { setNotificationActionsVisible(false); setSidebarDocument(workspaceList) }
    }
    @objc func showNotifications() {
        if sidebarCollapsed { toggleSidebar() }
        sidebarModes?.selectedSegment = 1
        setNotificationActionsVisible(true)
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
        jumpToUnreadButton.isEnabled = canJumpToUnread
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
            let available = item.target?.isAvailable(in: coordinator.snapshot ?? snapshot) ?? false
            let workspaceName = (coordinator.snapshot ?? snapshot).model.workspaces.first { $0.id == item.workspaceId }?.name
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
    private var canJumpToUnread: Bool {
        guard let snapshot else { return false }
        return snapshot.notifications.contains { !$0.read && ($0.target?.isAvailable(in: coordinator.snapshot ?? snapshot) ?? false) }
    }
    @objc private func jumpToUnread() { notificationDelivery.jumpToUnread() }
    private func setNotificationActionsVisible(_ visible: Bool) {
        clearNotificationsButton.isHidden = !visible
        jumpToUnreadButton.isHidden = !visible
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
        let stages = NativeSmokeStages(report: report)
        do {
            try await runSmokeTest(report: report, stages: stages)
            try await stages.finish()
        } catch {
            await stages.fail(error)
            throw error
        }
    }

    private func runSmokeTest(report: URL, stages: NativeSmokeStages) async throws {
        try await stages.begin("core_native_and_browser_setup")
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
        do {
            let wasCollapsed = sidebarCollapsed
            let savedSidebarWidth = sidebarLayout.preferredWidth
            let savedWidthPreference = UserDefaults.standard.object(forKey: sidebarWidthKey)
            let savedVisibilityPreference = UserDefaults.standard.object(forKey: sidebarCollapsedKey)
            let savedWindowFrame = window.frame
            let savedResponder = window.firstResponder
            let savedWorkspace = snapshot?.model.focusedWorkspace
            let savedPane = snapshot?.workspace?.focusedPane
            let savedTabs = snapshot?.workspace?.root.panes.map { $0.selectedSurface }
            defer {
                window.setFrame(savedWindowFrame, display: true)
                sidebarLayout.resize(to: savedSidebarWidth, availableWidth: .greatestFiniteMagnitude)
                sidebarCollapsed = wasCollapsed
                if let savedWidthPreference { UserDefaults.standard.set(savedWidthPreference, forKey: sidebarWidthKey) }
                else { UserDefaults.standard.removeObject(forKey: sidebarWidthKey) }
                if let savedVisibilityPreference { UserDefaults.standard.set(savedVisibilityPreference, forKey: sidebarCollapsedKey) }
                else { UserDefaults.standard.removeObject(forKey: sidebarCollapsedKey) }
                applySidebarLayout()
                window.makeFirstResponder(savedResponder)
            }
            if sidebarCollapsed { toggleSidebar() }
            for _ in 0..<32 { _ = sidebarDivider.accessibilityPerformDecrement() }
            guard abs(sidebarContainer.bounds.width - 200) < 1 else {
                throw BridgeFailure(message: "Sidebar accessibility resize did not reach its minimum width")
            }
            let point = sidebarDivider.convert(NSPoint(x: sidebarDivider.bounds.midX, y: sidebarDivider.bounds.midY), to: nil)
            guard let down = NSEvent.mouseEvent(with: .leftMouseDown, location: point, modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1),
                  let drag = NSEvent.mouseEvent(with: .leftMouseDragged, location: NSPoint(x: point.x + 64, y: point.y), modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1),
                  let up = NSEvent.mouseEvent(with: .leftMouseUp, location: NSPoint(x: point.x + 64, y: point.y), modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 0) else {
                throw BridgeFailure(message: "Unable to create native sidebar drag events")
            }
            @MainActor func responderDescription(_ responder: NSResponder?) -> String {
                guard let responder else { return "nil" }
                return "\(String(reflecting: type(of: responder)))@\(ObjectIdentifier(responder))"
            }
            @MainActor func sidebarHitTarget(_ location: NSPoint) -> String {
                guard let content = window.contentView else { return "no-content-view" }
                let pointInParent = content.superview?.convert(location, from: nil) ?? location
                return responderDescription(content.hitTest(pointInParent))
            }
            let downHit = sidebarHitTarget(point)
            let beforeMouseResponder = responderDescription(window.firstResponder)
            window.sendEvent(down)
            let afterDownResponder = responderDescription(window.firstResponder)
            window.sendEvent(drag)
            let afterDragResponder = responderDescription(window.firstResponder)
            window.sendEvent(up)
            guard abs(sidebarContainer.bounds.width - 264) < 1, window.firstResponder === savedResponder else {
                let diagnostics = [
                    "width=\(sidebarContainer.bounds.width), expected=264, preference=\(sidebarLayout.preferredWidth)",
                    "responders before=\(beforeMouseResponder), down=\(afterDownResponder), drag=\(afterDragResponder), up=\(responderDescription(window.firstResponder)), expected=\(responderDescription(savedResponder))",
                    "downPoint=\(point), downHit=\(downHit), dragPoint=\(drag.locationInWindow), dragHit=\(sidebarHitTarget(drag.locationInWindow))",
                    "dividerFrame=\(sidebarDivider.frame), dividerBounds=\(sidebarDivider.bounds), hidden=\(sidebarDivider.isHiddenOrHasHiddenAncestor)",
                    "windowFrame=\(window.frame), contentBounds=\(String(describing: window.contentView?.bounds)), key=\(window.isKeyWindow), currentEvent=\(String(describing: NSApp.currentEvent?.type))"
                ].joined(separator: "; ")
                throw BridgeFailure(message: "Sidebar dragging did not resize while retaining input focus: " + diagnostics)
            }
            window.recalculateKeyViewLoop()
            for _ in 0..<256 {
                if window.firstResponder === sidebarDivider { break }
                window.selectNextKeyView(nil)
            }
            guard window.firstResponder === sidebarDivider,
                  let left = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil, characters: String(UnicodeScalar(NSLeftArrowFunctionKey)!), charactersIgnoringModifiers: String(UnicodeScalar(NSLeftArrowFunctionKey)!), isARepeat: false, keyCode: 123) else {
                throw BridgeFailure(message: "Sidebar divider could not receive keyboard focus")
            }
            window.sendEvent(left)
            guard abs(sidebarContainer.bounds.width - 248) < 1, window.firstResponder === sidebarDivider else {
                throw BridgeFailure(message: "Sidebar keyboard resize failed: width=\(sidebarContainer.bounds.width), expected=248; responder=\(responderDescription(window.firstResponder))")
            }
            guard sidebarDivider.accessibilityPerformIncrement(), abs(sidebarContainer.bounds.width - 264) < 1 else {
                throw BridgeFailure(message: "Sidebar accessibility increment failed: width=\(sidebarContainer.bounds.width), expected=264")
            }
            let reloadedSidebarWidth = SidebarLayout(key: sidebarWidthKey, inheritLegacy: false).preferredWidth
            guard reloadedSidebarWidth == 264 else {
                throw BridgeFailure(message: "Sidebar persisted width failed: width=\(reloadedSidebarWidth), expected=264")
            }
            let priorCanvasWidth = canvas.bounds.width
            try performShortcut("b", modifiers: [.command])
            guard sidebarCollapsed, sidebarContainer.bounds.width == 0, canvas.bounds.width > priorCanvasWidth,
                  window.firstResponder === savedResponder else {
                throw BridgeFailure(message: "Sidebar collapse did not return its space and keyboard focus to the selected terminal")
            }
            try performShortcut("b", modifiers: [.command])
            guard !sidebarCollapsed, abs(sidebarContainer.bounds.width - 264) < 1, window.firstResponder === savedResponder else {
                throw BridgeFailure(message: "Sidebar expansion forgot the resized width or changed content focus")
            }
            for _ in 0..<32 { _ = sidebarDivider.accessibilityPerformIncrement() }
            guard abs(sidebarContainer.bounds.width - 480) < 1 else {
                throw BridgeFailure(message: "Sidebar accessibility resize did not respect its maximum width")
            }
            window.setContentSize(NSSize(width: 680, height: window.contentView?.bounds.height ?? 720))
            guard sidebarContainer.bounds.width < 480, canvas.bounds.width >= 400,
                  SidebarLayout(key: sidebarWidthKey, inheritLegacy: false).preferredWidth == 480 else {
                throw BridgeFailure(message: "A narrow window overwrote the sidebar preference or squeezed terminal content")
            }
            window.setFrame(savedWindowFrame, display: true)
            guard abs(sidebarContainer.bounds.width - 480) < 1, window.firstResponder === savedResponder,
                  snapshot?.model.focusedWorkspace == savedWorkspace,
                  snapshot?.workspace?.focusedPane == savedPane,
                  snapshot?.workspace?.root.panes.map({ $0.selectedSurface }) == savedTabs else {
                throw BridgeFailure(message: "Sidebar resizing lost its width, input focus, or workspace target")
            }
        }
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
        try await smokeNotificationReply(surface: originalPane.selectedSurface, session: originalSession)
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
        setNotificationActionsVisible(false)
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
        try await smokeWorkspaceMetadata(workspaceID: first.id, foregroundWorkspace: secondWorkspace)
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
        // A new blank tab initially owns address focus. This journey tests a
        // document-focus roundtrip, so establish that native intent first.
        @MainActor func browserFocusDiagnostics() -> String {
            let responder = window.firstResponder.map { String(reflecting: type(of: $0)) } ?? "nil"
            return "responder=\(responder), key=\(window.isKeyWindow), browserWindowMatches=\(browser.window === window), browserHidden=\(browser.isHiddenOrHasHiddenAncestor)"
        }
        guard window.makeFirstResponder(browser.webView), window.firstResponder === browser.webView else {
            throw BridgeFailure(message: "Loaded browser fixture could not establish native document focus: " + browserFocusDiagnostics())
        }
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
        guard window.firstResponder === browser.webView else {
            throw BridgeFailure(message: "Terminal-to-browser navigation did not focus WebKit: " + browserFocusDiagnostics())
        }
        try await stages.complete("core_native_and_browser_setup")
        try await stages.begin("browser_automation")
        let origin = try await smokeBrowserAutomation(report: report, surface: browserPane.selectedSurface, pane: browserPane, terminalSession: originalSession)
        try await stages.complete("browser_automation")
        try await stages.begin("browser_render")
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
        try await stages.complete("browser_render")
        try await stages.begin("native_render")
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
        try await stages.complete("native_render")
        try await stages.begin("remote_clipboard")
        try await RemoteClipboardImageSmoke.validate(configuration: report.deletingLastPathComponent().appendingPathComponent("native-ssh-fixture.json"), snapshot: { self.snapshot }, terminal: { self.terminals[$0] })
        try await stages.complete("remote_clipboard")
        try await stages.begin("local_tmux")
        try await LocalTmuxSmoke.validate(report: report.deletingLastPathComponent().appendingPathComponent("zmux-local-tmux-smoke.png"), cli: { try await self.nativeCLI($0, timeout: 15000, externalCaller: true) }, snapshot: { self.snapshot }, terminal: { self.terminals[$0] })
        try await stages.complete("local_tmux")
        try await stages.begin("terminal_special_keys")
        try await smokeTerminalSpecialKeys()
        try await stages.complete("terminal_special_keys")
        try await stages.begin("browser_download_checks")
        let downloadSaveVerified = try await BrowserDownloadSmoke.validate(origin: origin, window: window, report: report, cli: { try await self.nativeCLI($0, expectFailure: $1, externalCaller: true) }, snapshot: { self.snapshot }, browser: { self.browsers[$0] })
        guard let downloadsItem = NSApp.mainMenu?.items.flatMap({ $0.submenu?.items ?? [] }).first(where: { $0.action == #selector(showBrowserDownloads(_:)) }),
              let downloadsMenu = downloadsItem.menu else { throw BridgeFailure(message: "Downloads menu is unavailable") }
        downloadsMenu.performActionForItem(at: downloadsMenu.index(of: downloadsItem))
        guard let downloadsWindow = NSApp.windows.first(where: { $0.title == "Downloads" && $0.isVisible }) else { throw BridgeFailure(message: "Downloads menu did not open its native history") }
        downloadsWindow.close()
        try await stages.complete("browser_download_checks")
        let evidence: [String: Any] = [
            "success": true,
            "notification_permission_ordering": true,
            "notification_cli_metadata": true,
            "notification_workspace_target": true,
            "notification_jump_to_unread": true,
            "notification_reply_pty": true,
            "workspace_metadata": true,
            "workspace_pull_request_metadata": true,
            "workspace_todo_metadata": true,
            "remote_file_url_symlink": true,
            "remote_file_url_directory_refusal": true,
            "browser_cli_automation": true,
            "browser_locators_waits": true,
            "browser_profiles_isolation_reopen": true,
            "browser_profile_menu": true,
            "browser_profile_management": true,
            "browser_page_zoom": true,
            "browser_fill": true,
            "browser_click": true,
            "browser_interactions": true,
            "browser_edit_scroll": true,
            "browser_keyboard": true,
            "browser_downloads_native": downloadSaveVerified,
            "browser_downloads_cancel_native": true,
            "native_save_dialog_accessibility": downloadSaveVerified ? "passed" : "skipped_user_deferred",
            "local_tmux_cli_native_reopen": true,
            "browser_never_selected_viewport": true,
            "browser_csp_eval_once": true,
            "browser_title_observation": true,
            "native_input": true,
            "terminal_special_keys": true,
            "ansi_color": true,
            "split_panes": true,
            "kernel_resize": true,
            "workspace_switch": true,
            "workspace_order": true,
            "surface_order": true,
            "workspace_number": true,
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
            "sidebar_resize": true,
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

    func nativeCLI(_ arguments: [String], expectFailure: Bool = false, timeout: Int = 5000, externalCaller: Bool = false) async throws -> Data {
        let executable = Bundle.main.resourceURL!.appendingPathComponent("bin/zmux")
        return try await Task.detached {
            let process = Process()
            process.executableURL = executable
            process.arguments = ["--timeout", String(timeout)] + arguments
            if externalCaller {
                var environment = ProcessInfo.processInfo.environment
                for key in ["ZMUX_WINDOW_ID", "ZMUX_WORKSPACE_ID", "ZMUX_SURFACE_ID", "ZMUX_TAB_ID"] {
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
    /// Exercises the Swift reply wire against a real PTY, without OS permission or
    /// a synthetic UserNotifications response. Native banner/text-input UI is a separate gate.
    private func smokeNotificationReply(surface: UInt64, session: UInt64) async throws {
        guard let before = snapshot, let workspace = before.workspace,
              let pane = workspace.root.panes.first(where: { $0.surfaces.contains { $0.id == surface } }),
              pane.selectedSurface != surface else {
            throw BridgeFailure(message: "Reply smoke requires a background terminal")
        }
        let selected = Dictionary(uniqueKeysWithValues: workspace.root.panes.map { ($0.id, $0.selectedSurface) })
        let nonce = UUID().uuidString.lowercased()
        let title = "CLI reply \(nonce)"
        _ = try await nativeCLI(["notify", "--surface", String(surface), "--title", title, "--reply", "--desktop", "false"])
        try await waitUntil { $0.notifications.contains { $0.title == title && !$0.read } }
        guard let item = snapshot?.notifications.last(where: { $0.title == title }), let target = item.target else {
            throw BridgeFailure(message: "Reply notification omitted its exact inbox target")
        }
        let command = "printf '\\nZMUX_REPLY_%s\\n' '\(nonce)'"
        let text = " \t" + command + "\t "
        let receipt = try await bridge.notificationAction(.reply(target: target, text: text))
        guard let reply = receipt.reply, reply.target == target, reply.outcome == .complete,
              reply.bytesWritten == UInt64(command.utf8.count + 1), reply.totalBytes == reply.bytesWritten,
              receipt.navigation == nil, receipt.focusedWorkspaceId == nil, receipt.focusedSurfaceId == nil else {
            throw BridgeFailure(message: "Reply did not confirm the canonical text and terminator without navigation")
        }
        // UI snapshots intentionally omit hidden frames. The public targeted read
        // inspects the background PTY without selecting its tab or borrowing focus.
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: .seconds(5))
        var observed = false
        while clock.now < deadline {
            let data = try await nativeCLI(["read-screen", "--surface", String(surface)], timeout: 1000)
            guard let screen = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                  (screen["surface_id"] as? NSNumber)?.uint64Value == surface,
                  (screen["session_id"] as? NSNumber)?.uint64Value == session,
                  let contents = screen["contents"] as? String else {
                throw BridgeFailure(message: "Targeted reply screen read did not identify the background terminal")
            }
            if contents.components(separatedBy: "\n").contains("ZMUX_REPLY_\(nonce)"),
               snapshot?.notifications.first(where: { $0.id == item.id })?.read == true {
                observed = true
                break
            }
            try await Task.sleep(for: .milliseconds(25))
        }
        guard observed else { throw BridgeFailure(message: "Reply text and read acknowledgement were not observed on the background terminal") }
        guard let after = snapshot, after.model.focusedWorkspace == workspace.id,
              after.workspace?.focusedPane == workspace.focusedPane,
              after.workspace?.root.panes.allSatisfy({ selected[$0.id] == $0.selectedSurface }) == true else {
            throw BridgeFailure(message: "A completed background reply changed focus or selected tabs")
        }
    }

    private func smokeWorkspaceMetadata(workspaceID: UInt64, foregroundWorkspace: UInt64) async throws {
        guard let before = snapshot, before.model.focusedWorkspace == foregroundWorkspace else {
            throw BridgeFailure(message: "Workspace metadata fixture requires a background target")
        }
        let responder = window.firstResponder
        let selected = Dictionary(uniqueKeysWithValues: before.model.workspaces.map { workspace in
            (workspace.id, Dictionary(uniqueKeysWithValues: workspace.root.panes.map { ($0.id, $0.selectedSurface) }))
        })
        let target = ["--workspace", String(workspaceID)]
        let literal = "<b>Native metadata</b>"
        let log = "<script>literal log</script>\n" + String(repeating: "Retained context.\n", count: 30)
        _ = try await nativeCLI(["set-status", "notes", "Lower priority", "--priority", "1"] + target, externalCaller: true)
        _ = try await nativeCLI(["set-status", "build", literal, "--priority", "20", "--icon", "hammer.fill", "--color", "#33AA77"] + target, externalCaller: true)
        _ = try await nativeCLI(["set-progress", "0.42", "--label", "Indexing source"] + target, externalCaller: true)
        _ = try await nativeCLI(["log", log, "--level", "warning", "--source", "native-smoke"] + target, externalCaller: true)
        try await waitUntil { $0.workspaceMetadata?.first(where: { $0.workspaceId == workspaceID })?.logs.count == 1 }
        window.contentView?.layoutSubtreeIfNeeded()
        guard let row = workspaceRows.first(where: { $0.value == workspaceID })?.key,
              row.subviews.compactMap({ $0 as? NSTextField }).contains(where: { $0.stringValue == "build: " + literal && !$0.isHidden }),
              row.subviews.compactMap({ $0 as? NSTextField }).contains(where: { $0.stringValue == "42% · Indexing source" && !$0.isHidden }),
              row.subviews.compactMap({ $0 as? NSProgressIndicator }).contains(where: { abs($0.doubleValue - 0.42) < 0.001 && !$0.isHidden }),
              row.bounds.height <= 86.5,
              window.firstResponder === responder,
              let button = row.subviews.compactMap({ $0 as? NSButton }).first(where: { $0.accessibilityLabel()?.hasPrefix("Show workspace activity") == true }) else {
            throw BridgeFailure(message: "CLI metadata did not reach the bounded native status/progress preview or changed input focus")
        }
        button.performClick(nil)
        guard metadataPopover?.isShown == true, let inspector = metadataInspector, inspector.workspaceID == workspaceID,
              let scroll = inspector.view.subviews.compactMap({ $0 as? NSScrollView }).first,
              let details = scroll.documentView as? NSTextView,
              details.string.contains(literal), details.string.contains("notes · priority 1"),
              details.string.contains("WARNING · native-smoke"), details.string.contains("<script>literal log</script>"),
              details.string.contains("0 older entries omitted"), !details.isEditable, !details.isRichText,
              scroll.hasVerticalScroller else { throw BridgeFailure(message: "Workspace activity details lost status/log fields or treated metadata as rich text") }
        let metadataResponder = window.firstResponder
        let ownerData = try await nativeCLI(["list-status"] + target, externalCaller: true)
        guard let owner = try JSONSerialization.jsonObject(with: ownerData) as? [String: Any],
              let rawUUID = owner["workspace_uuid"] as? String, let ownerUUID = UUID(uuidString: rawUUID) else {
            throw BridgeFailure(message: "Native PR fixture did not resolve the exact shared workspace UUID")
        }
        let prNumber = UInt64.max
        let prURL = "https://github.com/acme/pull/pull/\(prNumber)"
        try await WorkspacePullRequestSmoke.set(workspace: ownerUUID, number: prNumber, url: prURL)
        try await waitUntil { $0.workspaceMetadata?.first(where: { $0.workspaceId == workspaceID })?.pullRequest?.number == prNumber }
        window.contentView?.layoutSubtreeIfNeeded()
        guard row.subviews.compactMap({ $0 as? NSTextField }).contains(where: { $0.stringValue == "42% · Indexing source · PR #\(prNumber)" && !$0.isHidden }),
              details.string.contains("PR #\(prNumber)"), details.string.contains(prURL),
              details.string.contains(literal), details.string.contains("<script>literal log</script>"),
              row.bounds.height <= 86.5, window.firstResponder === metadataResponder,
              snapshot?.model.focusedWorkspace == foregroundWorkspace else {
            throw BridgeFailure(message: "Shared PR metadata did not reach the row/inspector intact or changed focus/status/log state")
        }
        let selection = (details.string as NSString).range(of: literal)
        details.setSelectedRange(selection)
        if let layout = details.layoutManager, let container = details.textContainer { layout.ensureLayout(for: container) }
        scroll.contentView.scroll(to: NSPoint(x: 0, y: 40))
        scroll.reflectScrolledClipView(scroll.contentView)
        let origin = scroll.contentView.bounds.origin
        _ = try await nativeCLI(["log", "Build complete", "--level", "success", "--source", "native-smoke"] + target, externalCaller: true)
        try await waitUntil { $0.workspaceMetadata?.first(where: { $0.workspaceId == workspaceID })?.logs.count == 2 }
        guard details.string.contains("Build complete"), details.selectedRange() == selection,
              abs(scroll.contentView.bounds.origin.y - origin.y) < 1 else {
            throw BridgeFailure(message: "Live workspace log refresh lost the reader's selection or scroll position")
        }
        _ = try await nativeCLI(["pr", "clear", "--workspace", ownerUUID.uuidString], externalCaller: true)
        try await waitUntil { $0.workspaceMetadata?.first(where: { $0.workspaceId == workspaceID })?.pullRequest == nil }
        let clearSelection = details.selectedRange()
        let clearDocument = details.string as NSString
        guard let retainedMetadata = snapshot?.workspaceMetadata?.first(where: { $0.workspaceId == workspaceID }),
              retainedMetadata.statuses.count == 2, retainedMetadata.logs.count == 2,
              retainedMetadata.progress?.value == 0.42,
              row.subviews.compactMap({ $0 as? NSTextField }).contains(where: { $0.stringValue == "42% · Indexing source" && !$0.isHidden }),
              details.string.contains("No manual pull request"), !details.string.contains(prURL),
              clearSelection.location <= clearDocument.length,
              clearSelection.length <= clearDocument.length - clearSelection.location,
              clearDocument.substring(with: clearSelection) == literal,
              window.firstResponder === metadataResponder else {
            throw BridgeFailure(message: "PR clear changed other metadata/selected text or left stale native projection")
        }
        let todoText = "Native todo <b>literal</b>"
        let todoData = try await nativeCLI(["todo", "add", todoText, "--state", "in-progress", "--origin", "agent"] + target, externalCaller: true)
        guard let todoResponse = try JSONSerialization.jsonObject(with: todoData) as? [String: Any],
              let todoItems = todoResponse["todos"] as? [[String: Any]],
              let todoID = todoItems.first?["id"] as? String, let todoUUID = UUID(uuidString: todoID) else {
            throw BridgeFailure(message: "Public todo add omitted the shared item identity")
        }
        try await waitUntil { $0.workspaceMetadata?.first(where: { $0.workspaceId == workspaceID })?.todos.first?.id == todoUUID }
        guard details.string.contains(todoText), details.string.contains("1. in-progress · agent · " + todoID.lowercased()),
              row.accessibilityLabel()?.contains("1 todo items") == true,
              snapshot?.model.focusedWorkspace == foregroundWorkspace, window.firstResponder === metadataResponder else {
            throw BridgeFailure(message: "Shared todo metadata did not reach the real row/inspector or changed focus")
        }
        _ = try await nativeCLI(["todo", "check", todoID] + target, externalCaller: true)
        try await waitUntil { $0.workspaceMetadata?.first(where: { $0.workspaceId == workspaceID })?.todos.first?.state == .completed }
        guard details.string.contains("1. completed · agent · " + todoID.lowercased()) else {
            throw BridgeFailure(message: "Todo completion did not update the open native inspector")
        }
        _ = try await nativeCLI(["todo", "clear"] + target, externalCaller: true)
        try await waitUntil { $0.workspaceMetadata?.first(where: { $0.workspaceId == workspaceID })?.todos.isEmpty == true }
        let todoSelection = details.selectedRange()
        let todoDocument = details.string as NSString
        guard let todoRetained = snapshot?.workspaceMetadata?.first(where: { $0.workspaceId == workspaceID }),
              todoRetained.statuses.count == 2, todoRetained.logs.count == 2, todoRetained.progress?.value == 0.42,
              details.string.contains("No todo items"), !details.string.contains(todoText),
              todoSelection.location <= todoDocument.length,
              todoSelection.length <= todoDocument.length - todoSelection.location,
              todoDocument.substring(with: todoSelection) == literal,
              snapshot?.model.focusedWorkspace == foregroundWorkspace, window.firstResponder === metadataResponder else {
            throw BridgeFailure(message: "Todo clear changed other metadata, selection, focus or retained stale inspector content")
        }
        _ = try await nativeCLI(["clear-status", "build"] + target, externalCaller: true)
        _ = try await nativeCLI(["clear-status", "notes"] + target, externalCaller: true)
        _ = try await nativeCLI(["clear-progress"] + target, externalCaller: true)
        _ = try await nativeCLI(["clear-log"] + target, externalCaller: true)
        try await waitUntil { $0.workspaceMetadata?.contains(where: { $0.workspaceId == workspaceID }) != true }
        window.contentView?.layoutSubtreeIfNeeded()
        guard row.bounds.height <= 50.5, button.isHidden, details.string.contains("No statuses"),
              details.string.contains("No progress"), details.string.contains("No log entries") else {
            throw BridgeFailure(message: "Clearing workspace metadata did not update the row and open details")
        }
        metadataPopover?.performClose(nil)
        guard let after = snapshot, after.model.focusedWorkspace == foregroundWorkspace,
              after.workspace?.focusedPane == before.workspace?.focusedPane,
              Dictionary(uniqueKeysWithValues: after.model.workspaces.map { workspace in
                  (workspace.id, Dictionary(uniqueKeysWithValues: workspace.root.panes.map { ($0.id, $0.selectedSurface) }))
              }) == selected, window.firstResponder === responder else {
            throw BridgeFailure(message: "Workspace metadata updates changed the selected target or did not restore input focus after details")
        }
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
        try await smokeUnreadNotificationJump(workspace: retained, foregroundWorkspace: foregroundWorkspace)
        sidebarModes?.selectedSegment = 0
        setNotificationActionsVisible(false)
        setSidebarDocument(workspaceList)
    }
    private func smokeUnreadNotificationJump(workspace: Workspace, foregroundWorkspace: UInt64) async throws {
        let selectedTabs = Dictionary(uniqueKeysWithValues: workspace.root.panes.map { ($0.id, $0.selectedSurface) })
        for (index, shortcut) in [true, false].enumerated() {
            let title = "CLI unread jump \(index)"
            _ = try await nativeCLI(["notify", "--workspace", String(workspace.id), "--title", title, "--desktop", "false"], externalCaller: true)
            try await waitUntil { $0.notifications.contains { $0.title == title && !$0.read } }
            guard let item = snapshot?.notifications.last(where: { $0.title == title }),
                  snapshot?.model.focusedWorkspace == foregroundWorkspace,
                  jumpToUnreadButton.isEnabled, !jumpToUnreadButton.isHidden else {
                throw BridgeFailure(message: "Unread notification changed focus before an explicit jump or disabled the inbox action")
            }
            if shortcut { try performShortcut("u", modifiers: [.command, .shift]) }
            else { jumpToUnreadButton.performClick(nil) }
            try await waitUntil { value in
                guard let focused = value.workspace, focused.id == workspace.id,
                      focused.focusedPane == workspace.focusedPane,
                      value.notifications.first(where: { $0.id == item.id })?.read == true else { return false }
                return Dictionary(uniqueKeysWithValues: focused.root.panes.map { ($0.id, $0.selectedSurface) }) == selectedTabs
            }
            selectWorkspace(foregroundWorkspace)
            try await waitUntil { $0.model.focusedWorkspace == foregroundWorkspace }
        }
    }
    /// Explicit eval prepares storage/target-blank fixtures. This exercises real
    /// WebKit profile ownership, not pointer input or complete browser persistence.
    private func smokeBrowserProfiles(origin: String, pane: UInt64, privateSurface: UInt64) async throws {
        func add(_ name: String) async throws -> String {
            let data = try await nativeCLI(["browser", "profiles", "add", name])
            guard let result = try JSONSerialization.jsonObject(with: data) as? [String: Any], let id = result["id"] as? String else { throw BridgeFailure(message: "Profile add omitted durable identity") }
            return id
        }
        func open(_ profile: String) async throws -> UInt64 {
            let data = try await nativeCLI(["browser", "open", "--pane", String(pane), "--profile", profile, "--no-focus", origin + "/first"])
            guard let result = try JSONSerialization.jsonObject(with: data) as? [String: Any], let created = result["created_surfaces"] as? [[String: Any]], let id = (created.first?["surface_id"] as? NSNumber)?.uint64Value else { throw BridgeFailure(message: "Profile open omitted surface identity") }
            try await waitUntil { [weak self] _ in self?.browsers[id]?.webView.title == "First fixture" && self?.browsers[id]?.webView.isLoading == false }
            return id
        }
        func eval(_ script: String, _ surface: UInt64) async throws -> Any { try await browserCLIValue(["browser", "eval", script, "--surface", String(surface)]) }
        let suffix = UUID().uuidString
        let a = try await add("Smoke A " + suffix), b = try await add("Smoke B " + suffix)
        let first = try await open(a), sibling = try await open(a), separate = try await open(b)
        guard browsers[first]?.webView.configuration.websiteDataStore.identifier?.uuidString.lowercased() == a,
              browsers[separate]?.webView.configuration.websiteDataStore.identifier?.uuidString.lowercased() == b,
              browsers[privateSurface]?.webView.configuration.websiteDataStore.isPersistent == false else {
            throw BridgeFailure(message: "Native host selected a substitute profile datastore")
        }
        _ = try await eval("document.cookie = 'zmuxProfile=ownerA; Max-Age=3600; Path=/'; localStorage.setItem('zmuxProfile', 'ownerA'); null", first)
        guard try await eval("localStorage.getItem('zmuxProfile')", sibling) as? String == "ownerA",
              try await eval("document.cookie.includes('zmuxProfile=ownerA')", sibling) as? Bool == true,
              try await eval("localStorage.getItem('zmuxProfile') === null && !document.cookie.includes('zmuxProfile=ownerA')", separate) as? Bool == true,
              try await eval("localStorage.getItem('zmuxProfile') === null && !document.cookie.includes('zmuxProfile=ownerA')", privateSurface) as? Bool == true else {
            throw BridgeFailure(message: "Native persistent profile sharing/isolation failed")
        }
        guard let found = try await browserCLIValue(["browser", "find", "role", "button", "--name", "Continue", "--exact", "--surface", String(first)]) as? [String: Any], let oldRef = found["ref"] as? String else { throw BridgeFailure(message: "Profile fixture omitted element ref") }
        _ = try await nativeCLI(["close-surface", "--surface", String(sibling)])
        _ = try await nativeCLI(["close-surface", "--surface", String(first)])
        _ = try await nativeCLI(["reopen-closed"])
        try await waitUntil { [weak self] value in
            guard let surface = value.workspace?.root.panes.first(where: { $0.id == pane })?.selected else { return false }
            return surface.browserProfileId?.uuidString.lowercased() == a && surface.id != first && self?.browsers[surface.id]?.webView.title == "First fixture" && self?.browsers[surface.id]?.webView.isLoading == false
        }
        guard let reopened = snapshot?.workspace?.root.panes.first(where: { $0.id == pane })?.selectedSurface,
              try await eval("localStorage.getItem('zmuxProfile') === 'ownerA' && document.cookie.includes('zmuxProfile=ownerA')", reopened) as? Bool == true else { throw BridgeFailure(message: "Profile storage identity did not survive last-host close/reopen") }
        let stale = try await nativeCLI(["browser", "wait", "--selector", oldRef, "--state", "hidden", "--surface", String(reopened)], expectFailure: true)
        guard String(decoding: stale, as: UTF8.self).contains("stale_reference") else { throw BridgeFailure(message: "Profile reopen reused a stale host reference") }
        // A target-blank DOM fixture tests popup routing; it is not a pointer claim.
        let prior = Set(browsers.keys)
        _ = try await eval("const link = document.createElement('a'); link.href = '/first'; link.target = '_blank'; document.body.append(link); link.click(); null", reopened)
        try await waitUntil { value in value.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.contains { !prior.contains($0.id) && $0.browserProfileId?.uuidString.lowercased() == a } }
        guard let popup = snapshot?.model.workspaces.flatMap({ $0.root.panes }).flatMap({ $0.surfaces }).first(where: { !prior.contains($0.id) && $0.browserProfileId?.uuidString.lowercased() == a }) else { throw BridgeFailure(message: "Popup lost its parent profile") }
        try await waitUntil { [weak self] _ in self?.browsers[popup.id]?.webView.title == "First fixture" && self?.browsers[popup.id]?.webView.isLoading == false }
        guard try await eval("localStorage.getItem('zmuxProfile')", popup.id) as? String == "ownerA" else { throw BridgeFailure(message: "Popup did not use its parent's datastore") }
        for id in [popup.id, separate, reopened] { _ = try await nativeCLI(["close-surface", "--surface", String(id)]) }
        selectTab(privateSurface)
        try await waitUntil { $0.workspace?.root.panes.first(where: { $0.id == pane })?.selectedSurface == privateSurface }
        // Exercise the real AppKit menu item and its shared create-browser route.
        let priorMenuSurfaces = Set(browsers.keys)
        menuNeedsUpdate(browserProfilesMenu)
        guard let index = browserProfilesMenu.items.firstIndex(where: { ($0.representedObject as? String)?.lowercased() == a }),
              validateMenuItem(browserProfilesMenu.items[index]) else {
            throw BridgeFailure(message: "Browser profile menu did not expose the durable profile")
        }
        browserProfilesMenu.performActionForItem(at: index)
        try await waitUntil { value in
            guard let selected = value.workspace?.root.panes.first(where: { $0.id == pane })?.selected else { return false }
            return !priorMenuSurfaces.contains(selected.id) && selected.browserProfileId?.uuidString.lowercased() == a
        }
        guard let menuSurface = focusedPane?.selectedSurface else { throw BridgeFailure(message: "Profile menu did not select its new tab") }
        _ = try await nativeCLI(["browser", "goto", "--surface", String(menuSurface), origin + "/first"])
        try await waitUntil { [weak self] _ in self?.browsers[menuSurface]?.webView.title == "First fixture" && self?.browsers[menuSurface]?.webView.isLoading == false }
        guard try await eval("localStorage.getItem('zmuxProfile') === 'ownerA' && document.cookie.includes('zmuxProfile=ownerA')", menuSurface) as? Bool == true else {
            throw BridgeFailure(message: "Native profile menu opened a substitute website datastore")
        }
        _ = try await nativeCLI(["close-surface", "--surface", String(menuSurface)])
        selectTab(privateSurface)
        try await waitUntil { $0.workspace?.root.panes.first(where: { $0.id == pane })?.selectedSurface == privateSurface }
        try await smokeBrowserProfileManager(origin: origin, pane: pane, privateSurface: privateSurface)
    }
    private func smokeBrowserProfileManager(origin: String, pane: UInt64, privateSurface: UInt64) async throws {
        guard let item = NSApp.mainMenu?.items.flatMap({ $0.submenu?.items ?? [] }).first(where: { $0.action == #selector(showBrowserProfiles(_:)) }),
              let menu = item.menu else { throw BridgeFailure(message: "Profile management menu is unavailable") }
        menu.performActionForItem(at: menu.index(of: item))
        guard let content = browserProfilesWindow?.window?.contentView,
              browserProfilesWindow?.window?.isVisible == true else { throw BridgeFailure(message: "Profile manager did not open") }
        @MainActor func descendants(_ view: NSView) -> [NSView] {
            var result = [view]
            for child in view.subviews { result += descendants(child) }
            return result
        }
        let controls = descendants(content)
        guard let name = controls.compactMap({ $0 as? NSTextField }).first(where: { $0.accessibilityLabel() == "New profile name" }),
              let status = controls.compactMap({ $0 as? NSTextField }).first(where: { $0.accessibilityLabel() == "Profile status" }),
              let create = controls.compactMap({ $0 as? NSButton }).first(where: { $0.title == "Create Profile" }),
              let rename = controls.compactMap({ $0 as? NSButton }).first(where: { $0.title == "Rename Profile" }),
              let renameName = controls.compactMap({ $0 as? NSTextField }).first(where: { $0.accessibilityLabel() == "Selected profile new name" }),
              let open = controls.compactMap({ $0 as? NSButton }).first(where: { $0.title == "Open Profile" }),
              let profiles = controls.compactMap({ $0 as? NSPopUpButton }).first else {
            throw BridgeFailure(message: "Profile manager controls are unavailable")
        }
        let submittedName = "Smoke Manager " + UUID().uuidString
        let previousIDs = Set(snapshot?.browserProfiles.profiles.map(\.id) ?? [])
        name.stringValue = submittedName
        create.performClick(nil)
        guard !create.isEnabled, !open.isEnabled else { throw BridgeFailure(message: "Profile manager allowed duplicate pending work") }
        try await waitUntil { $0.browserProfiles.profiles.contains { $0.name == submittedName && !previousIDs.contains($0.id) } && create.isEnabled }
        guard let profile = snapshot?.browserProfiles.profiles.first(where: { $0.name == submittedName }),
              (profiles.selectedItem?.representedObject as? String) == profile.id.uuidString,
              open.isEnabled else { throw BridgeFailure(message: "Profile manager did not retain the created identity") }
        // The real duplicate failure must be shown without changing the catalog.
        let createdIDs = Set(snapshot?.browserProfiles.profiles.map(\.id) ?? [])
        name.stringValue = submittedName
        create.performClick(nil)
        try await waitUntil { _ in create.isEnabled && status.stringValue.contains("already exists") }
        guard Set(snapshot?.browserProfiles.profiles.map(\.id) ?? []) == createdIDs else {
            throw BridgeFailure(message: "Duplicate manager creation changed the catalog")
        }
        let priorSurfaces = Set(browsers.keys)
        open.performClick(nil)
        try await waitUntil { value in
            guard let selected = value.workspace?.root.panes.first(where: { $0.id == pane })?.selected else { return false }
            return !priorSurfaces.contains(selected.id) && selected.browserProfileId == profile.id && browsers[selected.id]?.webView.configuration.websiteDataStore.identifier == profile.id
        }
        guard let surface = focusedPane?.selectedSurface,
              browsers[surface]?.webView.configuration.websiteDataStore.identifier == profile.id else {
            throw BridgeFailure(message: "Managed profile did not open its UUID datastore")
        }
        _ = try await nativeCLI(["browser", "goto", "--surface", String(surface), origin + "/first"])
        try await waitUntil { [weak self] _ in self?.browsers[surface]?.webView.title == "First fixture" && self?.browsers[surface]?.webView.isLoading == false }
        guard try await browserCLIValue(["browser", "eval", "localStorage.getItem('zmuxProfile') === null && !document.cookie.includes('zmuxProfile=ownerA')", "--surface", String(surface)]) as? Bool == true else {
            throw BridgeFailure(message: "Managed profile opened another profile's website data")
        }
        guard try await browserCLIValue(["browser", "eval", "localStorage.setItem('zmuxManager', 'created'); document.cookie = 'zmuxManager=created; Max-Age=3600; Path=/'; localStorage.getItem('zmuxManager') === 'created' && document.cookie.includes('zmuxManager=created')", "--surface", String(surface)]) as? Bool == true else {
            throw BridgeFailure(message: "Managed profile did not retain its own website data")
        }
        guard try await browserCLIValue(["browser", "eval", "localStorage.getItem('zmuxManager') === null && !document.cookie.includes('zmuxManager=created')", "--surface", String(privateSurface)]) as? Bool == true else {
            throw BridgeFailure(message: "Managed profile data leaked into the private browser")
        }
        // Real manager and CLI rename the same live UUID without replacing its host
        // or losing existing website data, then survive close/reopen through the manager.
        guard let retainedHost = browsers[surface] else { throw BridgeFailure(message: "Managed profile host disappeared") }
        try await BrowserProfileLeaseSmoke.run(profile: profile.id, browser: retainedHost)
        let renamedName = "Renamed " + UUID().uuidString
        renameName.stringValue = renamedName
        rename.performClick(nil)
        guard !rename.isEnabled, !create.isEnabled, !open.isEnabled else { throw BridgeFailure(message: "Profile rename allowed overlapping manager work") }
        try await waitUntil { $0.browserProfiles.profiles.contains { $0.id == profile.id && $0.name == renamedName } && rename.isEnabled }
        guard browsers[surface] === retainedHost,
              (profiles.selectedItem?.representedObject as? String) == profile.id.uuidString,
              profiles.selectedItem?.title == renamedName else { throw BridgeFailure(message: "Profile rename replaced the host or selected another UUID") }
        _ = try await nativeCLI(["browser", "profiles", "rename", renamedName, submittedName])
        try await waitUntil { $0.browserProfiles.profiles.contains { $0.id == profile.id && $0.name == submittedName } }
        guard browsers[surface] === retainedHost,
              try await browserCLIValue(["browser", "eval", "localStorage.getItem('zmuxManager') === 'created' && document.cookie.includes('zmuxManager=created')", "--surface", String(surface)]) as? Bool == true else {
            throw BridgeFailure(message: "Profile rename lost its live website data")
        }
        _ = try await nativeCLI(["close-surface", "--surface", String(surface)])
        let beforeReopen = Set(browsers.keys)
        open.performClick(nil)
        try await waitUntil { value in
            guard let selected = value.workspace?.root.panes.first(where: { $0.id == pane })?.selected else { return false }
            return !beforeReopen.contains(selected.id) && selected.browserProfileId == profile.id
        }
        guard let reopened = focusedPane?.selectedSurface,
              browsers[reopened]?.webView.configuration.websiteDataStore.identifier == profile.id else {
            throw BridgeFailure(message: "Renamed profile reopened a different datastore")
        }
        _ = try await nativeCLI(["browser", "goto", "--surface", String(reopened), origin + "/first"])
        try await waitUntil { [weak self] _ in self?.browsers[reopened]?.webView.title == "First fixture" && self?.browsers[reopened]?.webView.isLoading == false }
        guard try await browserCLIValue(["browser", "eval", "localStorage.getItem('zmuxManager') === 'created' && document.cookie.includes('zmuxManager=created')", "--surface", String(reopened)]) as? Bool == true else {
            throw BridgeFailure(message: "Renamed profile lost data after manager reopen")
        }
        _ = try await nativeCLI(["close-surface", "--surface", String(reopened)])
        browserProfilesWindow?.close()
        selectTab(privateSurface)
        try await waitUntil { $0.workspace?.root.panes.first(where: { $0.id == pane })?.selectedSurface == privateSurface }
        FileHandle.standardError.write(Data("Native browser profiles: real manager/CLI rename, immutable UUID/live host, cookie/localStorage retention and manager reopen passed\n".utf8))
    }
    /// Public bundled-CLI assertions on a never-selected WebKit document. Fixture
    /// DOM writes are explicit eval; no synthetic input is presented as pointer parity.
    private func smokeBrowserLocators(surface: UInt64) async throws {
        let target = ["--surface", String(surface)]
        guard let button = try await browserCLIValue(["browser"] + target + ["find", "role", "button", "--name", "Continue", "--exact"]) as? [String: Any],
              let reference = button["ref"] as? String,
              try await browserCLIValue(["browser", "get", "text", reference] + target) as? String == "Continue",
              try await browserCLIValue(["browser", "is", "visible", reference] + target) as? Bool == true else {
            throw BridgeFailure(message: "Native semantic locator did not produce a reusable live ref")
        }
        _ = try await browserCLIValue(["browser", "eval", "const fixture = document.createElement('section'); fixture.id = 'zmux-locators'; fixture.innerHTML = '<label for=zmux-locator-input>Account name</label><input id=zmux-locator-input placeholder=Account data-testid=account><span class=zmux-duplicate>Duplicate locator</span><span class=zmux-duplicate>Duplicate locator</span><div id=zmux-ready style=display:none>Delayed readiness</div>'; document.body.append(fixture); null"] + target)
        guard let label = try await browserCLIValue(["browser", "find", "label", "Account name", "--exact"] + target) as? [String: Any],
              let labelledRef = label["ref"] as? String,
              let attribute = try await browserCLIValue(["browser", "find", "testid", "account", "--exact"] + target) as? [String: Any],
              attribute["ref"] as? String == labelledRef,
              let hidden = try await browserCLIValue(["browser", "wait", "--selector", "#zmux-ready", "--state", "hidden", "--timeout-ms", "1000"] + target) as? [String: Any],
              hidden["matched"] as? Bool == true else {
            throw BridgeFailure(message: "Native labels, test ids or hidden wait returned unrelated results")
        }
        let ambiguous = try await nativeCLI(["browser", "find", "text", "Duplicate locator", "--exact"] + target, expectFailure: true)
        guard String(decoding: ambiguous, as: UTF8.self).contains("ambiguous_locator") else {
            throw BridgeFailure(message: "Native locator ambiguity failed for an unrelated reason")
        }
        _ = try await browserCLIValue(["browser", "eval", "setTimeout(() => document.getElementById('zmux-ready').style.display = 'block', 700); null"] + target)
        guard let ready = try await browserCLIValue(["browser"] + target + ["wait", "--selector", "#zmux-ready", "--timeout-ms", "3000"]) as? [String: Any],
              ready["matched"] as? Bool == true, let readyRef = ready["ref"] as? String,
              try await browserCLIValue(["browser", "get", "text", readyRef] + target) as? String == "Delayed readiness" else {
            throw BridgeFailure(message: "Native wait failed to observe the delayed page transition")
        }
        let timedOut = try await nativeCLI(["browser", "wait", "--selector", "#zmux-never-present", "--timeout-ms", "300"] + target, expectFailure: true)
        guard String(decoding: timedOut, as: UTF8.self).contains("browser_timeout") else {
            throw BridgeFailure(message: "Native wait timeout had an unrelated failure: " + String(decoding: timedOut.prefix(1024), as: UTF8.self))
        }
        _ = try await browserCLIValue(["browser", "eval", "document.getElementById('zmux-locators').remove(); null"] + target)
        let stale = try await nativeCLI(["browser", "wait", "--selector", readyRef, "--state", "hidden", "--timeout-ms", "1000"] + target, expectFailure: true)
        guard String(decoding: stale, as: UTF8.self).contains("stale_reference") else {
            throw BridgeFailure(message: "Disconnected native wait ref was silently retargeted")
        }
    }
    private func smokeBrowserAutomation(report: URL, surface: UInt64, pane: Pane, terminalSession: UInt64) async throws -> String {
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
        try await smokeBrowserLocators(surface: hiddenID)
        guard snapshot?.model.focusedWorkspace == originalWorkspace, focusedPane?.selectedSurface == surface else {
            throw BridgeFailure(message: "Locator/wait commands changed hidden browser focus")
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
        guard try await browserCLIValue(["browser", "eval", "document.title = 'Changed title fixture'"] + target) as? String == "Changed title fixture" else {
            throw BridgeFailure(message: "Browser title fixture did not change the live document")
        }
        try await smokePublishedBrowserTitle(surface: surface, title: "Changed title fixture")
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
        try await smokePublishedBrowserTitle(surface: surface, title: "CSP fixture")
        try await smokeBrowserProfiles(origin: origin, pane: pane.id, privateSurface: surface)
        try await BrowserZoomSmoke.validate(surface: surface, cli: { try await self.nativeCLI($0, expectFailure: $1, externalCaller: true) }, snapshot: { self.snapshot }, browser: { self.browsers[$0] })
        try await BrowserFillSmoke.validate(cli: { try await self.nativeCLI($0, expectFailure: $1, externalCaller: true) }, snapshot: { self.snapshot }, browser: { self.browsers[$0] })
        try await BrowserClickSmoke.validate(origin: origin, cli: { try await self.nativeCLI($0, expectFailure: $1, externalCaller: true) }, snapshot: { self.coordinator.snapshot }, browser: { self.browsers[$0] })
        try await BrowserInteractionSmoke.validate(origin: origin, cli: { try await self.nativeCLI($0, expectFailure: $1, externalCaller: true) }, snapshot: { self.coordinator.snapshot }, browser: { self.browsers[$0] })
        try await BrowserEditScrollSmoke.validate(origin: origin, cli: { try await self.nativeCLI($0, expectFailure: $1, externalCaller: true) }, snapshot: { self.coordinator.snapshot }, browser: { self.browsers[$0] })
        try await BrowserKeyboardSmoke.validate(origin: origin, cli: { try await self.nativeCLI($0, expectFailure: $1, externalCaller: true) }, snapshot: { self.coordinator.snapshot }, browser: { self.browsers[$0] })
        try await WorkspaceNumberSmoke.validate(window: window, cli: { try await self.nativeCLI($0, expectFailure: $1, externalCaller: true) }, snapshot: { self.coordinator.snapshot }, terminals: { self.terminals }, browser: { self.browsers[$0] })
        try await WorkspaceOrderSmoke.validate(window: window, cli: { try await self.nativeCLI($0, expectFailure: $1, externalCaller: true) }, snapshot: { self.coordinator.snapshot }, rows: { self.workspaceList.arrangedSubviews.compactMap { $0 as? WorkspaceRowView } }, terminals: { self.terminals }, browser: { self.browsers[$0] })
        @MainActor func nativeTabs(_ view: NSView) -> [TerminalTabControl] {
            if let tab = view as? TerminalTabControl { return [tab] }
            return view.subviews.flatMap { nativeTabs($0) }
        }
        try await SurfaceOrderSmoke.validate(window: window, cli: { try await self.nativeCLI($0, expectFailure: $1, externalCaller: true) }, snapshot: { self.coordinator.snapshot }, tabs: { nativeTabs(self.canvas) }, terminals: { self.terminals }, browser: { self.browsers[$0] })
        return origin
    }

    private func smokePublishedBrowserTitle(surface: UInt64, title: String) async throws {
        do {
            try await waitUntil { value in
                value.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }
                    .contains { $0.id == surface && $0.title == title }
            }
        } catch {
            throw BridgeFailure(message: "Browser document title did not reach the Rust surface: \(title)")
        }
        @MainActor func tabDisplaysTitle(_ view: NSView) -> Bool {
            if let tab = view as? TerminalTabControl, tab.surface.id == surface {
                return tab.subviews.compactMap { $0 as? NSTextField }.contains { $0.stringValue == title }
            }
            return view.subviews.contains { tabDisplaysTitle($0) }
        }
        guard window.title == title, mainHeader.titleLabel.stringValue == title, tabDisplaysTitle(canvas) else {
            throw BridgeFailure(message: "Browser document title did not reach the native header and tab: \(title)")
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

    // Keep this async boundary outside the large smoke method's local-function
    // context. Detached work performs filesystem cleanup away from AppKit.
    private nonisolated static func removeNativeKeyFixture(_ fixture: URL) async throws {
        let cleanup = Task<Void, Error>.detached {
            try FileManager.default.removeItem(at: fixture)
        }
        try await cleanup.value
    }

    /// Native key dispatch is checked at the PTY byte boundary. The raw child has
    /// a deadline, restores its tty, and reports bytes rather than echoing key names.
    private func smokeTerminalSpecialKeys() async throws {
        guard let previous = focusedPane?.selectedSurface else { throw BridgeFailure(message: "No selected tab for terminal key smoke") }
        try performShortcut("t", modifiers: [.command])
        try await waitUntil { value in
            guard let pane = value.workspace?.root.panes.first(where: { $0.id == value.workspace?.focusedPane }),
                  pane.selectedSurface != previous, let session = pane.sessionId else { return false }
            return value.frames.contains { $0.sessionId == session && $0.snapshot?.state == "running" }
        }
        guard let pane = focusedPane, let session = pane.sessionId, let terminal = focusedTerminal,
              window.firstResponder === terminal else { throw BridgeFailure(message: "Terminal key fixture did not own native input") }
        let surface = pane.selectedSurface
        typealias Key = (code: UInt16, symbol: UInt32, modifiers: NSEvent.ModifierFlags, bytes: String)
        let normal: [Key] = [
            (122, UInt32(NSF1FunctionKey), [], "\u{1b}OP"), (120, UInt32(NSF2FunctionKey), [], "\u{1b}OQ"),
            (99, UInt32(NSF3FunctionKey), [], "\u{1b}OR"), (118, UInt32(NSF4FunctionKey), [], "\u{1b}OS"),
            (96, UInt32(NSF5FunctionKey), [], "\u{1b}[15~"), (97, UInt32(NSF6FunctionKey), [], "\u{1b}[17~"),
            (98, UInt32(NSF7FunctionKey), [], "\u{1b}[18~"), (100, UInt32(NSF8FunctionKey), [], "\u{1b}[19~"),
            (101, UInt32(NSF9FunctionKey), [], "\u{1b}[20~"), (109, UInt32(NSF10FunctionKey), [], "\u{1b}[21~"),
            (103, UInt32(NSF11FunctionKey), [], "\u{1b}[23~"), (111, UInt32(NSF12FunctionKey), [], "\u{1b}[24~"),
            (122, UInt32(NSF1FunctionKey), [.shift], "\u{1b}[1;2P"), (120, UInt32(NSF2FunctionKey), [.option], "\u{1b}[1;3Q"),
            (99, UInt32(NSF3FunctionKey), [.control], "\u{1b}[1;5R"), (118, UInt32(NSF4FunctionKey), [.shift, .option, .control], "\u{1b}[1;8S"),
            (96, UInt32(NSF5FunctionKey), [.control], "\u{1b}[15;5~"), (111, UInt32(NSF12FunctionKey), [.shift, .option], "\u{1b}[24;4~"),
            (115, UInt32(NSHomeFunctionKey), [], "\u{1b}[H"), (119, UInt32(NSEndFunctionKey), [], "\u{1b}[F"),
            (116, UInt32(NSPageUpFunctionKey), [], "\u{1b}[5~"), (121, UInt32(NSPageDownFunctionKey), [], "\u{1b}[6~"),
            (117, UInt32(NSDeleteFunctionKey), [], "\u{1b}[3~"),
            (115, UInt32(NSHomeFunctionKey), [.control], "\u{1b}[1;5H"), (119, UInt32(NSEndFunctionKey), [.option], "\u{1b}[1;3F"),
            (115, UInt32(NSHomeFunctionKey), [.shift], "\u{1b}[1;2H"), (116, UInt32(NSPageUpFunctionKey), [.control], "\u{1b}[5;5~"),
            (121, UInt32(NSPageDownFunctionKey), [.option], "\u{1b}[6;3~"), (117, UInt32(NSDeleteFunctionKey), [.shift], "\u{1b}[3;2~"),
            (117, UInt32(NSDeleteFunctionKey), [.option, .control], "\u{1b}[3;7~"),
            (123, UInt32(NSLeftArrowFunctionKey), [], "\u{1b}[D"), (126, UInt32(NSUpArrowFunctionKey), [.control], "\u{1b}[1;5A"),
            // Reserved app/history keys must contribute no PTY bytes, even with modifiers.
            (111, UInt32(NSF12FunctionKey), [.command], ""),
            (116, UInt32(NSPageUpFunctionKey), [.shift, .control], ""), (121, UInt32(NSPageDownFunctionKey), [.shift, .option], "")
        ]
        let application: [Key] = [
            (115, UInt32(NSHomeFunctionKey), [], "\u{1b}OH"), (119, UInt32(NSEndFunctionKey), [], "\u{1b}OF"),
            (123, UInt32(NSLeftArrowFunctionKey), [], "\u{1b}OD"), (126, UInt32(NSUpArrowFunctionKey), [], "\u{1b}OA"),
            (119, UInt32(NSEndFunctionKey), [.control], "\u{1b}[1;5F"), (124, UInt32(NSRightArrowFunctionKey), [.option], "\u{1b}[1;3C")
        ]
        let token = String(UUID().uuidString.prefix(8)).lowercased()
        let program = #"""
import os, select, termios, time, tty
token = "\#(token)"
def emit(phase):
    os.write(1, ("\r\nKEY_%s_%s\r\n" % (token, phase)).encode())
def capture():
    data = bytearray()
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        ready, _, _ = select.select([0], [], [], max(0, deadline - time.monotonic()))
        if not ready: break
        chunk = os.read(0, 4096)
        if not chunk: raise RuntimeError("native key input closed")
        data.extend(chunk)
        if len(data) > 4096: raise RuntimeError("key input exceeded its bound")
        if b"\0" in data:
            value, tail = data.split(b"\0", 1)
            if tail: raise RuntimeError("input followed the ordered key sentinel")
            return value.hex()
    raise RuntimeError("native key sentinel was not received before the deadline")
saved = termios.tcgetattr(0)
records = []
try:
    tty.setraw(0)
    for mode in range(2):
        os.write(1, b"\x1b[?1h" if mode else b"\x1b[?1l")
        emit("ready%d" % mode)
        records.append(capture())
finally:
    termios.tcsetattr(0, termios.TCSANOW, saved)
    os.write(1, b"\x1b[?1l")
for mode, value in enumerate(records): emit("%d_%s_END" % (mode, value))
emit("done")
"""#
        // The key contract starts after Python is ready. A private file avoids
        // feeding multiline setup source through interactive shell line editing;
        // that separate transport issue remains under investigation.
        let fixture = try await Task.detached {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("zmux-native-keys-" + UUID().uuidString, isDirectory: true)
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false,
                                                    attributes: [.posixPermissions: 0o700])
            guard FileManager.default.createFile(atPath: root.appendingPathComponent("keys.py").path,
                                                  contents: Data(program.utf8), attributes: [.posixPermissions: 0o600]) else {
                try FileManager.default.removeItem(at: root)
                throw BridgeFailure(message: "Could not create native key fixture source")
            }
            return root
        }.value
        do {
            let path = fixture.appendingPathComponent("keys.py").path
            terminal.insertText("python3 '" + path.replacingOccurrences(of: "'", with: "'\\''") + "'\r", replacementRange: NSRange(location: NSNotFound, length: 0))
            for (mode, keys) in [normal, application].enumerated() {
                do {
                    try await waitUntil { value in
                        value.frames.contains { frame in
                            frame.sessionId == session && frame.snapshot?.viewport.applicationCursor == (mode == 1)
                                && frame.snapshot?.viewport.contents.replacingOccurrences(of: "\n", with: "").contains("KEY_\(token)_ready\(mode)") == true
                        }
                    }
                } catch {
                    let readinessFailure = error.localizedDescription
                    // Only this newly created fixture terminal is inspected. Escape
                    // controls/non-ASCII and cap retained ASCII diagnostic bytes.
                    func diagnosticText(_ text: String, limit: Int) -> String {
                        let escaped = text.unicodeScalars.suffix(limit).map { scalar -> String in
                            switch scalar.value {
                            case 0x20...0x7e: return String(scalar)
                            case 0x0a: return "\\n"
                            case 0x0d: return "\\r"
                            case 0x09: return "\\t"
                            default: return "\\u{" + String(scalar.value, radix: 16) + "}"
                            }
                        }.joined()
                        return String(escaped.suffix(limit))
                    }
                    let responder = window.firstResponder.map { "\(String(reflecting: type(of: $0)))@\(ObjectIdentifier($0))" } ?? "nil"
                    var details = ["mode=\(mode)", "session=\(session)", "surface=\(surface)",
                                   "expectedReady=KEY_\(token)_ready\(mode)", "windowKey=\(window.isKeyWindow)",
                                   "firstResponder=\(responder)", "terminalMounted=\(terminal.window === window)"]
                    if let frame = snapshot?.frames.first(where: { $0.sessionId == session }) {
                        details.append("frameStarting=\(frame.starting),pid=\(frame.processId.map { String($0) } ?? "nil"),failure=\(diagnosticText(frame.failure ?? "nil", limit: 512))")
                        if let value = frame.snapshot {
                            let viewport = value.viewport
                            details.append("frameState=\(value.state),bytesRead=\(value.bytesRead),exitCode=\(value.exitCode.map { String($0) } ?? "nil"),failure=\(diagnosticText(value.failure ?? "nil", limit: 512))")
                            details.append("applicationCursor=\(viewport.applicationCursor),scrollbackOffset=\(viewport.scrollbackOffset),scrollbackLines=\(viewport.scrollbackLines),alternateScreen=\(viewport.alternateScreen),rows=\(viewport.size.rows),cols=\(viewport.size.cols)")
                        } else { details.append("frameSnapshot=unavailable") }
                    } else { details.append("frame=not-visible") }
                    let diagnosticWorkspaces: [Workspace] = snapshot?.model.workspaces ?? []
                    let diagnosticPanes: [Pane] = diagnosticWorkspaces.flatMap { $0.root.panes }
                    let diagnosticSurfaces: [Surface] = diagnosticPanes.flatMap { $0.surfaces }
                    let ownsTarget = diagnosticSurfaces.contains { candidate in
                        candidate.id == surface && candidate.kind == "terminal" && candidate.sessionId == session
                    }
                    if ownsTarget {
                        do {
                            let data = try await nativeCLI(["read-screen", "--surface", String(surface)], timeout: 1000, externalCaller: true)
                            guard let screen = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                                  (screen["surface_id"] as? NSNumber)?.uint64Value == surface,
                                  (screen["session_id"] as? NSNumber)?.uint64Value == session,
                                  let contents = screen["contents"] as? String else {
                                throw BridgeFailure(message: "Diagnostic screen did not identify the fixture surface/session")
                            }
                            details.append("readScreenFixtureTail=\(diagnosticText(contents, limit: 4096))")
                        } catch { details.append("readScreenError=\(diagnosticText(error.localizedDescription, limit: 512))") }
                    } else { details.append("readScreen=skipped-fixture-target-no-longer-owned") }
                    throw BridgeFailure(message: readinessFailure + "; special-key readiness: " + details.joined(separator: "; "))
                }
                for key in keys {
                    let characters = String(UnicodeScalar(key.symbol)!)
                    guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: key.modifiers.union(.function),
                        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
                        characters: characters, charactersIgnoringModifiers: characters, isARepeat: false, keyCode: key.code) else {
                        throw BridgeFailure(message: "Could not construct a native special-key event")
                    }
                    window.sendEvent(event)
                    // Exercise encoding rather than saturating the bounded input queue.
                    try await Task.sleep(for: .milliseconds(10))
                }
                // Control-Space emits NUL after all preceding key events on the same
                // input route. The child uses it as an ordering fence, without timing guesses.
                guard let sentinel = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.control],
                    timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
                    characters: " ", charactersIgnoringModifiers: " ", isARepeat: false, keyCode: 49) else {
                    throw BridgeFailure(message: "Could not construct the native key ordering sentinel")
                }
                window.sendEvent(sentinel)
            }
            try await waitUntil { value in
                value.frames.contains { $0.sessionId == session && $0.snapshot?.viewport.applicationCursor == false
                    && $0.snapshot?.viewport.contents.replacingOccurrences(of: "\n", with: "").contains("KEY_\(token)_done") == true }
            }
            let data = try await nativeCLI(["read-screen", "--surface", String(surface)], timeout: 1000)
            guard let screen = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                  (screen["session_id"] as? NSNumber)?.uint64Value == session,
                  let contents = screen["contents"] as? String else { throw BridgeFailure(message: "Key fixture screen was unavailable") }
            let compact = contents.replacingOccurrences(of: "\n", with: "")
            for (mode, keys) in [normal, application].enumerated() {
                let expected = keys.map { $0.bytes }.joined().utf8.map { String(format: "%02x", $0) }.joined()
                guard compact.contains("KEY_\(token)_\(mode)_\(expected)_END") else {
                    throw BridgeFailure(message: "Native special keys differed from xterm PTY bytes in mode \(mode): " + String(compact.suffix(2048)))
                }
            }
            closeTerminalTab(surface)
            try await waitUntil { $0.workspace?.root.panes.first(where: { $0.id == pane.id })?.selectedSurface == previous }
        } catch {
            let primary = error
            do { try await Self.removeNativeKeyFixture(fixture) }
            catch { throw BridgeFailure(message: "\(primary.localizedDescription); key fixture cleanup failed: \(error.localizedDescription)") }
            throw primary
        }
        try await Self.removeNativeKeyFixture(fixture)
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
            "t": 17, "w": 13, "b": 11, "u": 32, "[": 33, "]": 30,
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
    private var coordinator: ApplicationCoordinator?
    private var terminating = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        WindowLifecycleSmoke.trace("delegate entered before coordinator construction")
        let coordinator = ApplicationCoordinator()
        self.coordinator = coordinator
        WindowLifecycleSmoke.trace("coordinator constructed; constructing initial presenter")
        let controller = coordinator.mainWindowHost
        WindowLifecycleSmoke.trace("initial presenter constructed; scheduling startup")
        Task {
            do {
                if CommandLine.arguments.contains("--smoke-test") {
                    // This isolated context must release its control socket before the
                    // normal app context starts; never create two owners for one path.
                    try await NotificationPermissionSmoke.validate()
                }
                let storage: NativeDesktopStorage
                if CommandLine.arguments.contains("--terminal-engine-smoke") || CommandLine.arguments.contains("--idle-workload-smoke") || CommandLine.arguments.contains("--terminal-selection-smoke") {
                    storage = .transient
                } else if let index = CommandLine.arguments.firstIndex(of: "--completed-history-smoke"), CommandLine.arguments.count > index + 2 {
                    storage = .persistentDirectory(URL(fileURLWithPath: CommandLine.arguments[index + 2], isDirectory: true))
                } else if let index = CommandLine.arguments.firstIndex(of: "--profile-operations-smoke"), CommandLine.arguments.count > index + 4 {
                    storage = .persistentDirectory(URL(fileURLWithPath: CommandLine.arguments[index + 2], isDirectory: true))
                } else if let index = CommandLine.arguments.firstIndex(of: "--window-lifecycle-smoke"), CommandLine.arguments.count > index + 3 {
                    storage = .persistentDirectory(URL(fileURLWithPath: CommandLine.arguments[index + 2], isDirectory: true))
                } else if let index = CommandLine.arguments.firstIndex(of: "--smoke-test"), CommandLine.arguments.count > index + 1 {
                    let report = URL(fileURLWithPath: CommandLine.arguments[index + 1])
                    storage = .persistentDirectory(report.deletingLastPathComponent().appendingPathComponent("profile-fixture-" + UUID().uuidString, isDirectory: true))
                } else { storage = .applicationSupport }
                WindowLifecycleSmoke.trace("before coordinator.start")
                try await coordinator.start(storage: storage, terminalEngine: ProcessInfo.processInfo.environment["ZMUX_TERMINAL_ENGINE"])
                WindowLifecycleSmoke.trace("after coordinator.start")
                if let index = CommandLine.arguments.firstIndex(of: "--profile-operations-smoke"), CommandLine.arguments.count > index + 4 {
                    let report = URL(fileURLWithPath: CommandLine.arguments[index + 1])
                    let phase = CommandLine.arguments[index + 3]
                    do {
                        try await BrowserProfileAcceptance.run(coordinator: coordinator, report: report, phase: phase, origin: CommandLine.arguments[index + 4])
                        try await coordinator.shutdown()
                        try BrowserProfileAcceptance.shutdownComplete(report: report, phase: phase)
                        exit(0)
                    } catch {
                        FileHandle.standardError.write(Data("Native profile acceptance failed: \(error)\n".utf8))
                        do { try await coordinator.shutdown() } catch {
                            FileHandle.standardError.write(Data("Native profile acceptance cleanup failed: \(error)\n".utf8))
                        }
                        exit(1)
                    }
                }
                if let index = CommandLine.arguments.firstIndex(of: "--completed-history-smoke"), CommandLine.arguments.count > index + 2 {
                    do {
                        try await CompletedTerminalHistorySmoke.run(coordinator: coordinator, report: URL(fileURLWithPath: CommandLine.arguments[index + 1]))
                        exit(0)
                    } catch {
                        FileHandle.standardError.write(Data("Native completed history failed: \(error)\n".utf8))
                        do { try await coordinator.shutdown() } catch {
                            FileHandle.standardError.write(Data("Completed history cleanup failed: \(error)\n".utf8))
                        }
                        exit(1)
                    }
                }
                if let index = CommandLine.arguments.firstIndex(of: "--terminal-selection-smoke"), CommandLine.arguments.count > index + 1 {
                    do {
                        try await TerminalSelectionSmoke.run(coordinator: coordinator, report: URL(fileURLWithPath: CommandLine.arguments[index + 1]))
                        exit(0)
                    } catch {
                        FileHandle.standardError.write(Data("Native selection failed: \(error)\n".utf8))
                        do { try await coordinator.shutdown() } catch { FileHandle.standardError.write(Data("Selection cleanup failed: \(error)\n".utf8)) }
                        exit(1)
                    }
                }
                if let index = CommandLine.arguments.firstIndex(of: "--idle-workload-smoke"), CommandLine.arguments.count > index + 1 {
                    do {
                        try await NativeIdleWorkloadSmoke.run(coordinator: coordinator, report: URL(fileURLWithPath: CommandLine.arguments[index + 1]))
                        exit(0)
                    } catch {
                        FileHandle.standardError.write(Data("Native idle workload failed: \(error)\n".utf8))
                        do { try await coordinator.shutdown() } catch { FileHandle.standardError.write(Data("Idle workload cleanup failed: \(error)\n".utf8)) }
                        exit(1)
                    }
                }
                if let index = CommandLine.arguments.firstIndex(of: "--terminal-engine-smoke"), CommandLine.arguments.count > index + 1 {
                    do {
                        try await TerminalEngineSmoke.run(coordinator: coordinator, report: URL(fileURLWithPath: CommandLine.arguments[index + 1]))
                        try await coordinator.shutdown()
                        exit(0)
                    } catch {
                        FileHandle.standardError.write(Data("Native terminal engine smoke failed: \(error)\n".utf8))
                        do { try await coordinator.shutdown() } catch { FileHandle.standardError.write(Data("Terminal engine cleanup failed: \(error)\n".utf8)) }
                        exit(1)
                    }
                }
                if let index = CommandLine.arguments.firstIndex(of: "--window-lifecycle-smoke"), CommandLine.arguments.count > index + 3 {
                    let report = URL(fileURLWithPath: CommandLine.arguments[index + 1])
                    do {
                        WindowLifecycleSmoke.trace("enter fixture phase " + CommandLine.arguments[index + 3])
                        try await WindowLifecycleSmoke.run(coordinator: coordinator, report: report, phase: CommandLine.arguments[index + 3])
                        WindowLifecycleSmoke.trace("fixture returned; before explicit shutdown")
                        try await coordinator.shutdown()
                        exit(0)
                    } catch {
                        WindowLifecycleSmoke.trace("fixture failed: " + error.localizedDescription)
                        FileHandle.standardError.write(Data("Native window lifecycle failed: \(error)\n".utf8))
                        do { try await coordinator.shutdown() } catch {
                            FileHandle.standardError.write(Data("Native window cleanup failed: \(error)\n".utf8))
                        }
                        exit(1)
                    }
                }
                if let index = CommandLine.arguments.firstIndex(of: "--smoke-test"), CommandLine.arguments.count > index + 1 {
                    let report = URL(fileURLWithPath: CommandLine.arguments[index + 1])
                    do {
                        try await controller.smokeTest(report: report)
                        try await coordinator.shutdown()
                        exit(0)
                    } catch {
                        FileHandle.standardError.write(Data("Native smoke failed: \(error.localizedDescription)\n".utf8))
                        do { try await coordinator.shutdown() } catch {
                            FileHandle.standardError.write(Data("Cleanup failed: \(error)\n".utf8))
                        }
                        exit(1)
                    }
                }
            } catch {
                FileHandle.standardError.write(Data("zmux startup failed: \(error)\n".utf8))
                if CommandLine.arguments.contains("--smoke-test") || CommandLine.arguments.contains("--profile-operations-smoke") || CommandLine.arguments.contains("--window-lifecycle-smoke") || CommandLine.arguments.contains("--terminal-engine-smoke") || CommandLine.arguments.contains("--idle-workload-smoke") || CommandLine.arguments.contains("--terminal-selection-smoke") || CommandLine.arguments.contains("--completed-history-smoke") {
                    do { try await coordinator.shutdown() } catch {
                        FileHandle.standardError.write(Data("Startup cleanup failed: \(error)\n".utf8))
                    }
                    exit(1)
                }
                let alert = NSAlert()
                alert.messageText = "Unable to start zmux"
                alert.informativeText = error.localizedDescription
                alert.runModal()
                coordinator.requestApplicationTermination()
            }
        }
    }

    func applicationDidBecomeActive(_ notification: Notification) { coordinator?.appActive(true) }
    func applicationDidResignActive(_ notification: Notification) { coordinator?.appActive(false) }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let coordinator else { return .terminateNow }
        if terminating { return .terminateLater }
        terminating = true
        WindowLifecycleSmoke.trace("application termination requested; before shared shutdown")
        Task {
            do { try await coordinator.shutdown(); WindowLifecycleSmoke.trace("termination shutdown complete") } catch {
                FileHandle.standardError.write(Data("zmux shutdown: \(error)\n".utf8))
                if CommandLine.arguments.contains("--window-lifecycle-smoke") { exit(1) }
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
        guard let app = BrowserInputApplication.shared as? BrowserInputApplication else {
            FileHandle.standardError.write(Data("Browser input application containment is unavailable\n".utf8))
            exit(1)
        }
        app.setActivationPolicy(.regular)
        let delegate = AppDelegate()
        app.delegate = delegate
        withExtendedLifetime(delegate) { app.run() }
    }
}

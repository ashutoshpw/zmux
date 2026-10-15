import AppKit
import WebKit

enum UICommand: Sendable {
    case action(String, UInt64?, String?, String?, UInt64?)
    case surface(String, UInt64?, UInt64?, Int?, UInt64?)
    case navigation(String, String, UInt64?, UInt64?, UInt64?, Bool?)
    case attention(String, UInt64?, UInt64?, Bool?)
    case browser(String, UInt64?, UInt64?, String, String?, UUID?)
    case browserAutomation(UInt64, BrowserAction)
    case downloadCancel(BrowserDownloadRecord)
    case pasteFiles(UInt64, [String])
    case cancelUpload(UInt64)
    case input(UInt64, String)
    case resize(UInt64, Int, Int)
    case scroll(UInt64, Int)
    case wheel(UInt64, Int, Int, Int, Int, Bool)
}

/// Application lifetime owns one Rust Desktop. Main-window presenters borrow its
/// views and command routes; constructing a presenter cannot start another owner.
@MainActor
final class ApplicationCoordinator: BrowserCommandOwner, BrowserDownloadOwner {
    private(set) var snapshot: DesktopSnapshot?
    private(set) var mainWindowHosts: [MainWindowIdentity: MainWindowHost] = [:]
    private(set) lazy var mainWindowHost = MainWindowHost(coordinator: self)
    private var startRequested = false
    private var shutdownTask: Task<Void, Error>?
    let bridge = NativeBridge()
    lazy var browserDownloads = BrowserDownloads(owner: self, bridge: bridge)
    lazy var notificationDelivery = makeNotificationDelivery()
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
                self?.mainWindowHost.window.makeKeyAndOrderFront(nil)
                NSApp.activate()
            },
            reportError: { [weak self] message in self?.reportError(message) }
        )
    }
    lazy var imagePastes = ClipboardImagePastes(bridge: bridge, reportError: { [weak self] message in
        self?.reportError(message)
    }, statusChanged: { [weak self] in
        guard let self, let snapshot = self.snapshot else { return }
        self.mainWindowHost.refreshSharedStatus(snapshot)
    })
    private let stream: AsyncStream<UICommand>
    private let continuation: AsyncStream<UICommand>.Continuation
    private var commandTask: Task<Void, Never>?
    private var pollingTask: Task<Void, Never>?
    private let browserEvents = AsyncStream<(UInt64, BrowserHostEvent?)>.makeStream(bufferingPolicy: .bufferingOldest(256))
    private var browserEventGeneration: UInt64 = 0
    private var browserEventsNeedRecovery = false
    private var browserEventTask: Task<Void, Never>?
    private var browserEpoch: UUID?
    private(set) var terminals: [UInt64: TerminalView] = [:]
    private(set) var browsers: [UInt64: BrowserView] = [:]
    private var browserDataStores: [UUID: WKWebsiteDataStore] = [:]

    init() {
        (stream, continuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingOldest(256))
    }

    func start(storage: NativeDesktopStorage = .applicationSupport) async throws {
        guard !startRequested, shutdownTask == nil else {
            throw BridgeFailure(message: "The application desktop cannot be started more than once or after shutdown.")
        }
        startRequested = true
        _ = mainWindowHost
        try await bridge.start(storage: storage)
        guard shutdownTask == nil else {
            throw BridgeFailure(message: "Application shutdown interrupted desktop startup.")
        }
        browserEventTask = Task { [weak self] in
            guard let self else { return }
            for await (generation, event) in self.browserEvents.stream {
                if Task.isCancelled { break }
                if self.browserEventsNeedRecovery { await self.recoverBrowserHostEvents() }
                guard generation == self.browserEventGeneration, let event else { continue }
                do { try await self.bridge.browserEvent(event) }
                catch {
                    self.browserEventsNeedRecovery = true
                    self.reportError("Browser host synchronization failed: " + error.localizedDescription)
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
                    case .browser(let operation, let surface, let pane, let url, let title, let profile):
                        try await self.bridge.browser(operation, surface: surface, pane: pane, url: url, title: title, profile: profile)
                    case .downloadCancel(let record):
                        try await self.bridge.cancelBrowserDownload(record)
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
                    self.reportError(error.localizedDescription)
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
                    if let snapshot = try await self.bridge.poll() { try self.reconcile(snapshot) }
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
                    self.reportError(error.localizedDescription)
                    do { try await Task.sleep(for: .milliseconds(250)) } catch { break }
                }
            }
        }
        mainWindowHost.window.makeKeyAndOrderFront(nil)
        NSApp.activate()
        appActive(NSApp.isActive)
    }

    private func performShutdown() async throws {
        continuation.finish()
        commandTask?.cancel()
        pollingTask?.cancel()
        browsers.values.forEach { $0.close() }
        browsers.removeAll()
        try await browserDownloads.shutdown()
        browserEvents.continuation.finish()
        browserEventTask?.cancel()
        notificationDelivery.stop()
        mainWindowHost.finishPresentation()
        await imagePastes.stopAccepting()
        try await bridge.close()
        await imagePastes.desktopClosed()
    }

    func shutdown() async throws {
        if let shutdownTask { return try await shutdownTask.value }
        let task = Task { try await self.performShutdown() }
        shutdownTask = task
        try await task.value
    }

    func enqueue(_ command: UICommand) {
        mainWindowHost.clearErrorForCommand()
        switch continuation.yield(command) {
        case .enqueued: break
        case .dropped:
            reportError("Command queue is full; the new command was not accepted")
        case .terminated:
            reportError("The application desktop is closed; the command was not accepted")
        @unknown default:
            reportError("The application command queue is unavailable")
        }
    }
    func reportError(_ message: String) { mainWindowHost.showError(message) }
    func appActive(_ active: Bool) {
        enqueue(.attention("app_active", nil, nil, active && mainWindowHost.window.isKeyWindow))
        if active { notificationDelivery.refreshPermission() }
    }

    private func reconcile(_ snapshot: DesktopSnapshot) throws {
        // Validate canonical ownership before touching native registries. The
        // singleton policy remains closed until native lifecycle is implemented.
        let projection = try MainWindowProjection.singleton(snapshot)
        self.snapshot = snapshot
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
        }
        browserDownloads.update(snapshot)
        // Keep the bounded browser set mounted even when its tabs are hidden so
        // automation targets never need to change the user's selection or focus.
        for surface in snapshot.model.workspaces.flatMap({ $0.root.panes }).flatMap({ $0.surfaces }) where surface.kind == "browser" {
            let view: BrowserView
            if let existing = browsers[surface.id], existing.profileID == surface.browserProfileId { view = existing }
            else {
                browsers.removeValue(forKey: surface.id)?.close()
                let dataStore: WKWebsiteDataStore
                if let profile = surface.browserProfileId {
                    guard snapshot.browserProfiles.profiles.contains(where: { $0.id == profile }) else {
                        reportError("Browser profile is unavailable; its data store was not replaced")
                        continue
                    }
                    if let existing = browserDataStores[profile] { dataStore = existing }
                    else {
                        guard browserDataStores.count < 32 else { reportError("Browser profile data store capacity reached"); continue }
                        dataStore = WKWebsiteDataStore(forIdentifier: profile)
                        browserDataStores[profile] = dataStore
                    }
                } else { dataStore = .nonPersistent() }
                view = BrowserView(surface: surface, owner: self, dataStore: dataStore)
                view.frame = NSRect(x: 0, y: 0, width: min(1920, max(640, mainWindowHost.browserViewportSize.width)), height: min(1200, max(480, mainWindowHost.browserViewportSize.height)))
                view.layoutSubtreeIfNeeded()
            }
            browsers[surface.id] = view
            view.onAutomationAction = { [weak self] action in self?.enqueue(.browserAutomation(surface.id, action)) }
            view.bindAutomation(epoch: snapshot.browserEpoch, identityNamespace: snapshot.model.identityNamespace) { [weak self] event in
                guard let self else { return }
                if case .dropped = self.browserEvents.continuation.yield((self.browserEventGeneration, event)) {
                    self.browserEventsNeedRecovery = true
                    self.reportError("Browser event queue is full; resynchronizing browser hosts")
                }
            }
        }
        imagePastes.update(snapshot)
        notificationDelivery.update(snapshot.nativeNotifications)
        mainWindowHosts.removeAll(keepingCapacity: true)
        if let projection { mainWindowHosts[projection.identity] = mainWindowHost }
        let change = mainWindowHost.present(projection?.snapshot ?? snapshot, identity: projection?.identity)
        // Layout creates/reparents the borrowed views before their first frame;
        // responder reconciliation follows content updates, as in the singleton UI.
        for frame in snapshot.frames { terminals[frame.sessionId]?.update(frame) }
        for surface in snapshot.model.workspaces.flatMap({ $0.root.panes }).flatMap({ $0.surfaces }) where surface.kind == "browser" {
            browsers[surface.id]?.update(surface)
        }
        mainWindowHost.completePresentation(selectionChanged: change.selectionChanged, layoutChanged: change.layoutChanged)
    }

    private func recoverBrowserHostEvents() async {
        browserEventsNeedRecovery = false
        browserEventGeneration &+= 1
        // Discard queued incremental events captured before this full-owner snapshot.
        // New events retain order after the snapshot while acknowledgements are awaited.
        let attachments = browsers.values.compactMap { $0.automationAttachment() }
        let observations = browsers.values.compactMap { $0.automationObservation() }
        for event in attachments + observations {
            if Task.isCancelled { return }
            do { try await bridge.browserEvent(event) }
            catch {
                browserEventsNeedRecovery = true
                reportError("Browser synchronization is retrying: " + error.localizedDescription)
                return
            }
        }
    }

    func reserveBrowserDownload(origin: BrowserOwner, profile: UUID?, webView: WKWebView, conversionOwner: BrowserView, filename: String) async throws -> BrowserDownloadToken {
        try await browserDownloads.reserve(origin: origin, profile: profile, webView: webView, conversionOwner: conversionOwner, filename: filename)
    }
    func adoptBrowserDownload(_ download: WKDownload, token: BrowserDownloadToken, webView: WKWebView) { browserDownloads.adopt(download, token: token, webView: webView) }
    func abandonBrowserDownload(_ token: BrowserDownloadToken) { browserDownloads.abandon(token) }
    func cancelBrowserDownloadConversion(_ token: BrowserDownloadToken) { browserDownloads.cancelConversion(token) }
    func cancelBrowserDownloads(host: UUID) { browserDownloads.cancelHost(host) }
    func cancelBrowserDownload(_ record: BrowserDownloadRecord) { enqueue(.downloadCancel(record)) }
    func createBrowser(in pane: UInt64, url: String, profile: UUID? = nil) { enqueue(.browser("create_browser", nil, pane, url, nil, profile)) }
    func navigateBrowser(_ id: UInt64, url: String) { enqueue(.browser("navigate_browser", id, nil, url, nil, nil)) }
    func selectTab(_ id: UInt64) { enqueue(.surface("select_surface", id, nil, nil, nil)) }

    func terminal(session: UInt64, pane: Pane) -> TerminalView {
        let terminal = terminals[session] ?? TerminalView(pane: pane)
        terminal.pane = pane
        terminals[session] = terminal
        return terminal
    }

    func downloadWindow(for origin: BrowserOwner) -> NSWindow? {
        guard let snapshot, snapshot.model.identityNamespace == origin.identityNamespace,
              browsers[origin.surfaceId]?.hostID == origin.hostId,
              let workspace = snapshot.model.workspaces.first(where: { $0.root.panes.contains(where: { $0.surfaces.contains(where: { $0.id == origin.surfaceId }) }) }),
              let window = snapshot.model.windows.first(where: { $0.workspaceIds.contains(workspace.id) }) else { return nil }
        return mainWindowHosts[MainWindowIdentity(namespace: snapshot.model.identityNamespace, windowId: window.id)]?.window
    }
}

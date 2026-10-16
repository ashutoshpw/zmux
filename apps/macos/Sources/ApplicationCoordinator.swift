import AppKit
import WebKit

enum UICommand: Sendable {
    case window(String, UInt64?, Bool?)
    case windowEvent(NativeWindowEvent)
    case workspaceCreate(UInt64, String)
    case workspaceGroup(UUID, WorkspaceGroupCommand)
    case workspaceMove(UUID, UUID, Bool)
    case workspaceOrder(UUID, UUID, Int, UInt64)
    case workspaceNumber(UUID, Int)
    case surfaceOrder(UUID, UInt64, UInt64, Int, Bool, UInt64?)
    case windowHistory(UInt64, String)
    case windowReopen(UInt64, UInt64?)
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
    case key(UInt64, TerminalKeyEvent)
    case paste(UInt64, String)
    case resize(UInt64, Int, Int)
    case scroll(UInt64, Int)
    case wheel(UInt64, Int, Int, Int, Int, Bool)
    case pointer(UInt64, TerminalPointerEvent)
    /// Runs the wrapped command and reports whether the core accepted it. Used by closes that
    /// discard unsaved file text only once the tab is really gone.
    indirect case tracked(UICommand, CommandOutcome)
}

/// File-service work that can wait on a slow volume (a probe and an executor wait can hold one
/// request for about three seconds). It has its own bounded queue and task so it can never stall
/// terminal input, resize or wheel events queued behind it in the `UICommand` loop.
enum FileCommand: Sendable {
    /// Paths chosen in `File > Open...`, whether to focus the first, and whether to force Markdown.
    case open([String], Bool, Bool)
    case detach(FileOwnerToken)
}

/// Application lifetime owns one Rust Desktop. Main-window presenters borrow its
/// views and command routes; constructing a presenter cannot start another owner.
@MainActor
final class ApplicationCoordinator: BrowserCommandOwner, BrowserDownloadOwner, FilePanelOwner {
    private(set) var snapshot: DesktopSnapshot?
    private(set) var mainWindowHosts: [MainWindowIdentity: MainWindowHost] = [:]
    private lazy var initialWindowHost = MainWindowHost(coordinator: self)
    private var initialHostClaimed = false
    private var windowEpoch: UUID?
    private var windowTasks: [UUID: Task<Void, Never>] = [:]
    var mainWindowHost: MainWindowHost {
        if let key = NSApp.keyWindow, let host = mainWindowHosts.values.first(where: { $0.window === key }) { return host }
        if let snapshot, let id = snapshot.model.focusedWindow,
           let host = mainWindowHosts[MainWindowIdentity(namespace: snapshot.model.identityNamespace, windowId: id)] { return host }
        return initialWindowHost
    }
    private var startRequested = false
    private var shutdownTask: Task<Void, Error>?
    private let shutdownPresentation = ShutdownPresentation()
    private var shutdownPreparationStopped = false
    private var shutdownImagesReleased = false
    private var shutdownNativeIssues: [String] = []
    let bridge = NativeBridge()
    let splitLayouts = WorkspaceSplitPreferences()
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
            activate: { [weak self] receipt in
                guard let self, let snapshot = self.snapshot, let id = receipt.focusedWindowId,
                      let target = receipt.navigation?.target,
                      target.identityNamespace == snapshot.model.identityNamespace,
                      let host = self.mainWindowHosts[MainWindowIdentity(namespace: target.identityNamespace, windowId: id)] else { return }
                host.installMenus(); host.window.makeKeyAndOrderFront(nil)
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
    /// Bound of the file queue; a request past it is refused visibly, never queued without limit.
    static let fileCommandCapacity = 32
    private let fileStream: AsyncStream<FileCommand>
    private let fileContinuation: AsyncStream<FileCommand>.Continuation
    private var fileCommandTask: Task<Void, Never>?
    private var pollingTask: Task<Void, Never>?
    private let browserEvents = AsyncStream<(UInt64, BrowserHostEvent?)>.makeStream(bufferingPolicy: .bufferingOldest(256))
    private var browserEventGeneration: UInt64 = 0
    private var browserEventsNeedRecovery = false
    private var browserEventTask: Task<Void, Never>?
    private var browserEpoch: UUID?
    private(set) var terminals: [UInt64: TerminalView] = [:]
    private(set) var completedHistories: [UInt64: CompletedTerminalHistoryView] = [:]
    private var completedHistoryNamespace: UUID?
    private(set) var browsers: [UInt64: BrowserView] = [:]
    private lazy var browserProfileStores = BrowserProfileStores(bridge: bridge)
    /// One retained file panel per file surface, created and pruned in `reconcile` like `browsers`.
    private(set) var files: [UInt64: FilePanelView] = [:]
    private var filesNamespace: UUID?
    /// Panels whose tab vanished while they held unsaved text. They are never silently dropped.
    private(set) var orphanedFiles: [FilePanelView] = []
    private var presentingOrphan = false
    /// Replaces the Save / Don't Save / Cancel sheet; used by the in-app smoke only.
    var fileCloseChoiceOverride: (@MainActor (FilePanelView) async -> FileCloseChoice)?

    init() {
        (stream, continuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingOldest(256))
        (fileStream, fileContinuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingOldest(Self.fileCommandCapacity))
    }

    func start(storage: NativeDesktopStorage = .applicationSupport, terminalEngine: String? = nil) async throws {
        guard !startRequested, shutdownTask == nil else {
            throw BridgeFailure(message: "The application desktop cannot be started more than once or after shutdown.")
        }
        startRequested = true
        _ = mainWindowHost
        WindowLifecycleSmoke.trace("coordinator before bridge.start")
        try await bridge.start(storage: storage, terminalEngine: terminalEngine)
        WindowLifecycleSmoke.trace("coordinator after bridge.start")
        guard shutdownTask == nil else {
            throw BridgeFailure(message: "Application shutdown interrupted desktop startup.")
        }
        browserEventTask = Task { [weak self] in
            guard let self else { return }
            for await (generation, event) in self.browserEvents.stream {
                if Task.isCancelled || self.shutdownTask != nil { break }
                if self.browserEventsNeedRecovery { await self.recoverBrowserHostEvents() }
                guard !Task.isCancelled, self.shutdownTask == nil else { break }
                guard generation == self.browserEventGeneration, let event else { continue }
                do {
                    if case .rejectedObsolete = try await self.bridge.browserEvent(event) {
                        WindowLifecycleSmoke.trace("Native browser event rejected as obsolete; payload and downstream effects not applied")
                    }
                }
                catch {
                    guard !Task.isCancelled, self.shutdownTask == nil else { break }
                    self.browserEventsNeedRecovery = true
                    self.reportError("Browser host synchronization failed: " + error.localizedDescription)
                }
                if self.browserEventsNeedRecovery { await self.recoverBrowserHostEvents() }
            }
        }
        commandTask = Task { [weak self, stream] in
            for await command in stream {
                guard let self, !Task.isCancelled, self.shutdownTask == nil else {
                    // A command that never ran must not leave a waiting close hanging.
                    if case .tracked(_, let outcome) = command { outcome.finish(false) }
                    break
                }
                do { try await self.perform(command) } catch {
                    guard !Task.isCancelled, self.shutdownTask == nil else { break }
                    self.reportError(error.localizedDescription)
                }
            }
        }
        fileCommandTask = Task { [weak self, fileStream] in
            for await command in fileStream {
                guard let self, !Task.isCancelled, self.shutdownTask == nil else { break }
                do {
                    switch command {
                    case .open(let paths, let focus, let forceMarkdown):
                        let result = try await self.bridge.fileOpen(paths: paths, focus: focus, forceMarkdown: forceMarkdown)
                        let failures = result.items.filter { !$0.ok }.map { item -> String in
                            let operand = item.path ?? item.url ?? "item \(item.index)"
                            return operand + ": " + (item.error?.message ?? "could not be opened") + " (" + (item.error?.code ?? "unknown") + ")"
                        }
                        if !failures.isEmpty { self.reportError(failures.prefix(3).joined(separator: "; ")) }
                    case .detach(let owner):
                        try await self.bridge.fileDetach(owner)
                    }
                } catch {
                    guard !Task.isCancelled, self.shutdownTask == nil else { break }
                    self.reportError(error.localizedDescription)
                }
            }
        }
        pollingTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self, self.shutdownTask == nil else { break }
                do {
                    if (self.snapshot?.browserProfiles.nativePendingRequests ?? 0) > 0 || (self.snapshot?.browserProfiles.reconciliationPendingRequests ?? 0) > 0 {
                        try await self.browserProfileStores.poll(downloadsBusy: { self.browserDownloads.isProfileBusy($0) }, modelBusy: { profile in
                            self.snapshot?.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.contains { $0.browserProfileId == profile } ?? true
                        }, reportError: { [weak self] in self?.reportError($0) })
                    }
                    if self.browserEventsNeedRecovery {
                        // A wakeup, not a second sender: recovery must progress even
                        // when the last lifecycle event was the one that failed.
                        _ = self.browserEvents.continuation.yield((self.browserEventGeneration, nil))
                    }
                    if let snapshot = try await self.bridge.poll() { try self.reconcile(snapshot) }
                    guard !Task.isCancelled, self.shutdownTask == nil else { break }
                    if (self.snapshot?.browserPendingRequests ?? 0) > 0 {
                        let batch = try await self.bridge.drainBrowserRequests()
                        guard !Task.isCancelled, self.shutdownTask == nil else { break }
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
                    guard !Task.isCancelled, self.shutdownTask == nil else { break }
                    self.reportError(error.localizedDescription)
                    do { try await Task.sleep(for: .milliseconds(250)) } catch { break }
                }
            }
        }
        WindowLifecycleSmoke.trace("coordinator loops scheduled; before initial window activation")
        mainWindowHost.installMenus()
        WindowLifecycleSmoke.trace("initial menus installed; before makeKeyAndOrderFront")
        mainWindowHost.window.makeKeyAndOrderFront(nil)
        WindowLifecycleSmoke.trace("initial window ordered; before NSApp.activate")
        NSApp.activate()
        WindowLifecycleSmoke.trace("initial app activated; before appActive")
        appActive(NSApp.isActive)
        WindowLifecycleSmoke.trace("coordinator initial activation completed")
    }

    private func performShutdown() async throws {
        continuation.finish()
        fileContinuation.finish()
        commandTask?.cancel()
        fileCommandTask?.cancel()
        pollingTask?.cancel()
        windowTasks.values.forEach { $0.cancel() }
        windowTasks.removeAll()
        browsers.values.forEach { $0.close() }
        browsers.removeAll()
        // Shutdown does not talk to the file service: its own shutdown joins the file worker.
        files.values.forEach { $0.shutdown() }
        orphanedFiles.forEach { $0.shutdown() }
        files.removeAll()
        orphanedFiles.removeAll()
        browserDownloads.beginShutdown()
        browserEvents.continuation.finish()
        browserEventTask?.cancel()
        notificationDelivery.stop()
        for host in mainWindowHosts.values { host.finishPresentation() }
        Task {
            await imagePastes.stopAccepting()
            shutdownPreparationStopped = true
        }
        var ticket: UInt64?
        var downloadsComplete = false
        var releasingImages = false
        var nativeAcknowledged = false
        var lastObservationError: String?
        var lastPendingState: String?
        WindowLifecycleSmoke.trace("shutdown before shared Desktop ticket")
        while true {
            var completedIssues: [String]?
            do {
                let report: DesktopShutdownReport
                if let ticket { report = try await bridge.shutdownStatus(ticket: ticket) }
                else {
                    report = try await bridge.beginShutdown()
                    ticket = report.ticket
                }
                let pending = report.pending
                let pendingState = "phase=\(report.phase) sessions=\(pending.sessions) launches=\(pending.launches) tmux=\(pending.tmux) transfers=\(pending.transfers) connections=\(pending.connections) profiles=\(pending.profiles) persistence=\(pending.persistence) downloads=\(pending.downloads) nativeDownloads=\(pending.nativeDownloads) native=\(pending.nativeCleanup) control=\(pending.control)"
                if lastPendingState != pendingState {
                    WindowLifecycleSmoke.trace("shutdown " + pendingState)
                    lastPendingState = pendingState
                }
                if !downloadsComplete {
                    let progress = try await browserDownloads.shutdownProgress()
                    downloadsComplete = progress.complete
                    if let issue = progress.issue { shutdownPresentation.show(issue) }
                }
                // Image source files can still be read by Rust transfer workers.
                // Release them only after those workers have physically joined.
                if report.runtimeCleanupComplete, shutdownPreparationStopped, !releasingImages {
                    releasingImages = true
                    Task {
                        await imagePastes.desktopClosed()
                        shutdownImagesReleased = true
                    }
                }
                if snapshot?.browserProfiles.enabled == true {
                    try await browserProfileStores.pollReconciliations(reportError: { [weak self] in self?.reportError($0) })
                }
                browserProfileStores.cancelUnused(activeOwners: [], reportError: { [weak self] in self?.reportError($0) })
                if downloadsComplete, browserProfileStores.cleanupComplete, shutdownImagesReleased, !nativeAcknowledged {
                    try await bridge.nativeCleanupComplete(ticket: report.ticket)
                    nativeAcknowledged = true
                }
                if report.physicalCleanupComplete, report.phase == "complete", nativeAcknowledged {
                    try await bridge.releaseShutdown(ticket: report.ticket)
                    completedIssues = report.issues + shutdownNativeIssues
                    if report.issuesDropped > 0 { completedIssues?.append("Additional cleanup issues were recorded.") }
                } else if report.deadlineExpired {
                    shutdownPresentation.show("Cleanup is taking longer than expected. zmux is still waiting for its sessions and file operations to finish. zmux will close when cleanup finishes.")
                }
            } catch {
                // A missing observation cannot authorize process termination.
                // begin/ack/release are idempotent; polling never renews the ticket.
                let message = String(error.localizedDescription.prefix(512))
                shutdownPresentation.show("Cleanup is still pending: " + message)
                if lastObservationError != message {
                    WindowLifecycleSmoke.trace("shutdown observation: " + message)
                    lastObservationError = message
                }
            }
            if let completedIssues {
                shutdownPresentation.close()
                WindowLifecycleSmoke.trace("shutdown after all physical owners joined")
                if !completedIssues.isEmpty {
                    throw BridgeFailure(message: completedIssues.joined(separator: "\n"))
                }
                return
            }
            // Cleanup must continue even if a caller cancels its awaiting task.
            // A bounded timer keeps retries responsive without a cancellation spin.
            await withCheckedContinuation { continuation in
                DispatchQueue.global().asyncAfter(deadline: .now() + 0.1) { @Sendable in continuation.resume() }
            }
        }
    }

    func requestApplicationTermination() {
        // AppKit may enter a nested termination loop for terminateLater. Invoke
        // it from a run-loop selector, after the current Swift actor job returns,
        // so the delegate's asynchronous shutdown can acquire the main actor.
        NSApp.perform(#selector(NSApplication.terminate(_:)), with: nil,
                      afterDelay: 0, inModes: [.common])
    }

    func shutdown() async throws {
        if let shutdownTask { return try await shutdownTask.value }
        let task = Task { try await self.performShutdown() }
        shutdownTask = task
        try await task.value
    }

    /// Runs one queued command against the core. A failure is thrown to the command loop, which
    /// shows it; a tracked command also reports it to whoever is waiting for the answer.
    private func perform(_ command: UICommand) async throws {
        switch command {
        case .window(let operation, let window, let focus): try await bridge.window(operation, window: window, focus: focus)
        case .windowEvent(let event): try await bridge.windowEvent(event)
        case .workspaceMove(let workspace, let destination, let focus): try await bridge.moveWorkspace(workspace, destination: destination, focus: focus)
        case .workspaceOrder(let workspace, let window, let index, let revision): try await bridge.reorderWorkspace(workspace, window: window, index: index, revision: revision)
        case .surfaceOrder(let namespace, let surface, let pane, let value, let relative, let revision): try await bridge.reorderSurface(namespace: namespace, surface: surface, pane: pane, value: value, relative: relative, revision: revision)
        case .workspaceNumber(let window, let digit): try await bridge.selectWorkspaceNumber(digit, window: window)
        case .workspaceGroup(let namespace, let command):
            guard snapshot?.model.identityNamespace == namespace else { throw BridgeFailure(message: "Workspace group belongs to a previous desktop") }
            try await bridge.workspaceGroup(command)
        case .workspaceCreate(let window, let name): try await bridge.action("create_workspace_in_window", title: name, window: window)
        case .windowHistory(let window, let direction): try await bridge.action("navigate_focus_in_window", direction: direction, window: window)
        case .windowReopen(let window, let id): try await bridge.action("reopen_closed_in_window", closed: id, window: window)
        case .action(let action, let pane, let axis, let title, let workspace):
            try await bridge.action(action, pane: pane, axis: axis, title: title, workspace: workspace)
        case .surface(let action, let surface, let pane, let index, let closed):
            try await bridge.action(action, pane: pane, surface: surface, index: index, closed: closed)
        case .navigation(let action, let direction, let pane, let surface, let workspace, let focus):
            try await bridge.action(action, pane: pane, workspace: workspace, surface: surface, direction: direction, focus: focus)
        case .browser(let operation, let surface, let pane, let url, let title, let profile):
            try await bridge.browser(operation, surface: surface, pane: pane, url: url, title: title, profile: profile)
        case .downloadCancel(let record):
            try await bridge.cancelBrowserDownload(record)
        case .browserAutomation(let surface, let action):
            try await bridge.browserAction(surface: surface, action: action)
        case .attention(let operation, let id, let through, let active):
            try await bridge.attention(operation, id: id, through: through, active: active)
        case .pasteFiles(let surface, let paths):
            try await bridge.pasteFiles(surface: surface, paths: paths)
        case .cancelUpload(let id):
            try await bridge.cancelUpload(id: id)
        case .input(let session, let text):
            try await bridge.input(session: session, text: text)
        case .key(let session, let event):
            try await bridge.key(session: session, event: event)
        case .paste(let session, let text):
            try await bridge.paste(session: session, text: text)
        case .wheel(let session, let lines, let row, let col, let modifiers, let history):
            try await bridge.wheel(session: session, lines: lines, row: row, col: col, modifiers: modifiers, history: history)
        case .pointer(let session, let event):
            try await bridge.pointer(session: session, event: event)
        case .scroll(let session, let lines):
            try await bridge.scroll(session: session, lines: lines)
        case .resize(let session, let rows, let cols):
            try await bridge.resize(session: session, rows: rows, cols: cols)
        case .tracked(let inner, let outcome):
            do {
                try await perform(inner)
                outcome.finish(true)
            } catch {
                outcome.finish(false)
                throw error
            }
        }
    }

    @discardableResult
    func enqueue(_ command: UICommand) -> Bool {
        mainWindowHost.clearErrorForCommand()
        if case .paste(_, let text) = command, text.isEmpty || text.utf8.count > 16_384 {
            reportError("Development build pastes must contain 1–16 KiB of text")
            return false
        }
        switch continuation.yield(command) {
        case .enqueued: return true
        case .dropped:
            reportError("Command queue is full; the new command was not accepted")
        case .terminated:
            reportError("The application desktop is closed; the command was not accepted")
        @unknown default:
            reportError("The application command queue is unavailable")
        }
        return false
    }
    /// Queues file-service work (`File > Open...`, releasing a binding) off the terminal input path.
    /// A full or closed queue is reported, never dropped silently.
    @discardableResult
    func enqueueFile(_ command: FileCommand) -> Bool {
        if case .open = command { mainWindowHost.clearErrorForCommand() }
        switch fileContinuation.yield(command) {
        case .enqueued: return true
        case .dropped:
            switch command {
            case .open: reportError("The file queue is full (\(Self.fileCommandCapacity) requests waiting on the file service); the files were not opened. Try again shortly.")
            case .detach: reportError("The file queue is full; a closed file’s binding stays until Zmux restarts")
            }
        case .terminated:
            // A binding released during shutdown is cleaned up by the file service's own shutdown.
            if case .open = command { reportError("The application desktop is closed; the files were not opened") }
        @unknown default:
            reportError("The file command queue is unavailable")
        }
        return false
    }
    func reportError(_ message: String) {
        if shutdownTask != nil, shutdownNativeIssues.count < 16 {
            shutdownNativeIssues.append(String(message.prefix(512)))
        }
        WindowLifecycleSmoke.trace("coordinator error: " + message)
        mainWindowHost.showError(message)
    }
    func appActive(_ active: Bool) {
        guard shutdownTask == nil else { return }
        if !active { terminals.values.forEach { $0.cancelPointerGestures() } }
        enqueue(.attention("app_active", nil, nil, active))
        if active { notificationDelivery.refreshPermission() }
    }

    private func reconcile(_ snapshot: DesktopSnapshot) throws {
        guard shutdownTask == nil else { return }
        // Validate canonical ownership before touching native registries. The
        // Reject inconsistent membership before touching any native presenter.
        if self.snapshot == nil { WindowLifecycleSmoke.trace("first snapshot windows=\(snapshot.model.windows.count) requests=\(snapshot.windowRequests.count)") }
        let projections = try MainWindowProjection.all(snapshot)
        if windowEpoch != snapshot.windowEpoch {
            for task in windowTasks.values { task.cancel() }
            windowTasks.removeAll()
            for host in mainWindowHosts.values { host.closeAuthorized() }
            mainWindowHosts.removeAll()
            if windowEpoch != nil {
                initialWindowHost.closeAuthorized()
                initialWindowHost = MainWindowHost(coordinator: self)
                initialHostClaimed = false
            }
            windowEpoch = snapshot.windowEpoch
        }
        self.snapshot = snapshot
        try splitLayouts.reconcile(snapshot)
        let retainedHistories = Set(snapshot.model.workspaces.flatMap { $0.root.panes }
            .flatMap { $0.surfaces }.filter { $0.sessionId == nil && $0.localZellijCompletion != nil }.map(\.id))
        retainCompletedHistories(retainedHistories, namespace: snapshot.model.identityNamespace)
        let liveSessions = Set(snapshot.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.compactMap { $0.sessionId })
        terminals = terminals.filter { liveSessions.contains($0.key) }
        reconcileFilePanels(snapshot)
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
        browserProfileStores.cancelUnused(activeOwners: Set(snapshot.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.compactMap { surface in
            surface.browserProfileId.map { BrowserProfileStores.ConstructionOwner(namespace: snapshot.model.identityNamespace, browserEpoch: snapshot.browserEpoch, surface: surface.id, profile: $0) }
        }), refresh: { [weak self] in
            guard let self, self.shutdownTask == nil, let current = self.snapshot else { return }
            do { try self.reconcile(current) } catch { self.reportError("Unused profile constructor reconcile: " + error.localizedDescription) }
        }, reportError: { [weak self] in self?.reportError($0) })
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
                    guard let acquired = browserProfileStores.store(for: profile, surface: surface.id, namespace: snapshot.model.identityNamespace, browserEpoch: snapshot.browserEpoch, isNeeded: { [weak self] in
                        guard let self, self.shutdownTask == nil, let current = self.snapshot,
                              current.model.identityNamespace == snapshot.model.identityNamespace, current.browserEpoch == snapshot.browserEpoch,
                              current.browserProfiles.profiles.contains(where: { $0.id == profile }) else { return false }
                        return current.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }
                            .contains { $0.id == surface.id && $0.kind == "browser" && $0.browserProfileId == profile }
                    }, refresh: { [weak self] in
                        guard let self, self.shutdownTask == nil, let current = self.snapshot else { return }
                        do { try self.reconcile(current) }
                        catch { self.reportError(error.localizedDescription) }
                    }, reportError: { [weak self] message in self?.reportError(message) }) else { continue }
                    dataStore = acquired.store
                    view = BrowserView(surface: surface, owner: self, dataStore: dataStore)
                    browserProfileStores.constructed(acquired, profile: profile, view: view.webView,
                        refresh: { [weak self] in
                            guard let self, self.shutdownTask == nil, let current = self.snapshot else { return }
                            do { try self.reconcile(current) } catch { self.reportError("Profile construction reconcile: " + error.localizedDescription) }
                        }, reportError: { [weak self] in self?.reportError($0) })
                } else {
                    dataStore = .nonPersistent()
                    view = BrowserView(surface: surface, owner: self, dataStore: dataStore)
                }
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
        // Detach every changed old hierarchy before attaching any new hierarchy.
        // A shared surface/session view can belong to at most one native parent.
        for (identity, host) in mainWindowHosts {
            if let projection = projections.first(where: { $0.identity == identity }) { host.detachLayoutIfNeeded(projection.snapshot, identity: identity) }
            else { host.detachContent() }
        }
        var changes: [(MainWindowHost, Bool, Bool)] = []
        for projection in projections {
            guard let host = mainWindowHosts[projection.identity] else { continue }
            let change = host.present(projection.snapshot, identity: projection.identity)
            changes.append((host, change.selectionChanged, change.layoutChanged))
            if let token = host.nativeToken, snapshot.windowHosts.contains(token) { host.nativeReady = true }
        }
        for frame in snapshot.frames { terminals[frame.sessionId]?.update(frame) }
        for surface in snapshot.model.workspaces.flatMap({ $0.root.panes }).flatMap({ $0.surfaces }) where surface.kind == "browser" { browsers[surface.id]?.update(surface) }
        for (host, selection, layout) in changes { host.completePresentation(selectionChanged: selection, layoutChanged: layout) }
        let pending = Set(snapshot.windowRequests.map(\.operationId))
        windowTasks = windowTasks.filter { pending.contains($0.key) }
        for request in snapshot.windowRequests where windowTasks[request.operationId] == nil {
            windowTasks[request.operationId] = Task { [weak self] in
                guard let self else { return }
                do { try await self.performWindowRequest(request) }
                catch let failure as NativeWindowPreflightFailure {
                    do {
                        try await self.bridge.windowEvent(NativeWindowEvent(event: "failed", operationId: request.operationId, epoch: request.epoch, message: failure.message))
                    } catch { self.reportError("Native window failure acknowledgement uncertain: " + error.localizedDescription) }
                } catch { self.reportError("Native window acknowledgement retained without replay: " + error.localizedDescription) }
            }
        }
    }

    /// Creates, updates and prunes file panels before any window presents them, so `paneView`
    /// always finds the retained view of a selected file tab.
    private func reconcileFilePanels(_ snapshot: DesktopSnapshot) {
        let surfaces = snapshot.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.filter { $0.kind == "file" }
        if filesNamespace != snapshot.model.identityNamespace {
            for (id, view) in files { files.removeValue(forKey: id); retireFilePanel(view) }
            filesNamespace = snapshot.model.identityNamespace
        }
        let live = Set(surfaces.map(\.id))
        for (id, view) in files where !live.contains(id) {
            files.removeValue(forKey: id)
            retireFilePanel(view)
        }
        for surface in surfaces {
            guard let file = surface.file else {
                reportError("File tab \(surface.id) has no file description; it was not opened")
                continue
            }
            let view: FilePanelView
            if let existing = files[surface.id], existing.path == file.path { view = existing }
            else {
                if let stale = files.removeValue(forKey: surface.id) { retireFilePanel(stale) }
                view = FilePanelView(surface: surface, file: file, owner: self)
                files[surface.id] = view
            }
            view.update(surface: surface, files: snapshot.files)
        }
    }

    /// A clean panel is released at once, and so is one whose Don't Save is waiting for the core's
    /// close that just landed. One that still holds unsaved text is kept aside and the person is
    /// asked what to do with it; its text is never discarded silently.
    private func retireFilePanel(_ view: FilePanelView) {
        if view.isDiscardPending {
            // Don't Save was answered and the core has since closed the tab: the text is knowingly
            // discarded, so there is nothing to ask. The close settlement only gives the hold back.
            view.close()
        } else if view.hasUnsavedChanges {
            orphanedFiles.append(view)
            presentNextOrphan()
        } else {
            view.close()
        }
    }

    private func presentNextOrphan() {
        guard !presentingOrphan, let view = orphanedFiles.first else { return }
        presentingOrphan = true
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = "Unsaved changes to “" + view.displayName + "” have no tab"
        alert.informativeText = "The tab was closed while it had unsaved changes, and this app cannot save them any more. Copy the text to keep it, or discard it."
        alert.addButton(withTitle: "Copy Text")
        alert.addButton(withTitle: "Discard")
        Task { [weak self] in
            let response = await presentSheet(alert, in: self?.mainWindowHost.window)
            guard let self else { return }
            if response == .alertFirstButtonReturn { view.copyTextToPasteboard() }
            self.orphanedFiles.removeAll { $0 === view }
            view.close()
            self.presentingOrphan = false
            self.presentNextOrphan()
        }
    }

    var hasUnsavedFiles: Bool { !orphanedFiles.isEmpty || files.values.contains { $0.hasUnsavedChanges } }

    func filePanels(for surfaces: [UInt64]) -> [FilePanelView] {
        surfaces.compactMap { files[$0] }
    }

    /// Settles every file among `surfaces` before an app-initiated close: asks about unsaved text
    /// (Save / Don't Save / Cancel) and makes the model's dirty bit match each panel. Nil means
    /// the close must not proceed. The settlement's Don't Save answers stay pending: the caller
    /// finishes it with the core's answer to the close, so a refused close discards nothing.
    func resolveFilesBeforeClose(_ surfaces: [UInt64], window: NSWindow?) async -> FileCloseSettlement? {
        // The decision (Cancel and a failed Save abort with nothing discarded; Don't Save is
        // applied only once the core has closed the tab) lives in FileCloseResolution, which the
        // macOS fixture exercises; this only supplies the question.
        await FileCloseResolution.resolve(filePanels(for: surfaces)) { [self] view in
            if let override = fileCloseChoiceOverride { return await override(view) }
            let alert = NSAlert()
            alert.messageText = "Do you want to save the changes to “" + view.displayName + "”?"
            alert.informativeText = "Your changes will be lost if you don’t save them."
            alert.addButton(withTitle: "Save")
            alert.addButton(withTitle: "Don’t Save")
            alert.addButton(withTitle: "Cancel")
            switch await presentSheet(alert, in: window) {
            case .alertFirstButtonReturn: return .save
            case .alertSecondButtonReturn: return .discard
            default: return .cancel
            }
        }
    }

    /// Quit: resolves every dirty panel, and refuses to quit while an orphan still holds text.
    /// Quitting is not a core close that can be refused: the application terminates next and asks
    /// again if any panel still holds text, so a Don't Save is applied before it returns.
    func resolveFilesBeforeQuit() async -> Bool {
        guard orphanedFiles.isEmpty else {
            presentNextOrphan()
            return false
        }
        guard let settlement = await resolveFilesBeforeClose(Array(files.keys), window: mainWindowHost.window) else { return false }
        settlement.finish(closeSucceeded: true)
        return true
    }

    /// Sends a close command and reports the core's answer: it succeeds once the core executed the
    /// command, and fails when the command was not accepted, the core refused it, or the
    /// application stopped before running it. The failure is also shown to the person.
    func enqueueTracked(_ command: UICommand) -> CommandOutcome {
        let outcome = CommandOutcome()
        if !enqueue(.tracked(command, outcome)) { outcome.finish(false) }
        return outcome
    }

    func refreshFileChrome() {
        for host in mainWindowHosts.values { host.refreshFileChrome() }
    }

    func fileCall(_ request: FileHostRequest, body: Data) async throws -> FileHostReply {
        try await bridge.file(request, body: body)
    }
    /// One gate for every file panel: attach and reload requests never run more than
    /// `FileLoadPolicy.maxInFlight` at a time, so opening or restoring many file tabs queues them
    /// instead of overflowing the Rust file worker's queue.
    let fileLoadGate = FileLoadGate()
    func filePanelFocused(_ surface: UInt64) {}
    func filePanelStateChanged(_ surface: UInt64) { refreshFileChrome() }
    func filePanelReport(_ message: String) { reportError(message) }
    func filePanelDetach(_ owner: FileOwnerToken) {
        // Not `enqueue`: an automatic release must not clear an error the person has not read.
        enqueueFile(.detach(owner))
    }

    func windowKeyChanged(_ host: MainWindowHost, key: Bool) {
        if key { host.installMenus() }
        else { terminals.values.filter { $0.window === host.window }.forEach { $0.cancelPointerGestures() } }
        guard host.nativeReady, let token = host.nativeToken else { return }
        enqueue(.windowEvent(NativeWindowEvent(event: "key", token: token, key: key)))
    }

    private func performWindowRequest(_ request: NativeWindowRequest) async throws {
        guard !Task.isCancelled, shutdownTask == nil, let snapshot, request.epoch == snapshot.windowEpoch, request.identityNamespace == snapshot.model.identityNamespace else { return }
        let identity = MainWindowIdentity(namespace: request.identityNamespace, windowId: request.windowId)
        WindowLifecycleSmoke.trace("native request \(request.kind) window=\(request.windowId) operation=\(request.operationId)")
        switch request.kind {
        case "open":
            guard snapshot.model.windows.contains(where: { $0.id == request.windowId }) else {
                try await bridge.windowEvent(NativeWindowEvent(event: "failed", operationId: request.operationId, epoch: request.epoch, message: "Window closed before native materialization"))
                return
            }
            let host: MainWindowHost
            if let existing = mainWindowHosts[identity] { host = existing }
            else {
                guard mainWindowHosts.count < 2 else { throw NativeWindowPreflightFailure(message: "Native main-window capacity is full") }
                if !initialHostClaimed { host = initialWindowHost; initialHostClaimed = true }
                else { host = MainWindowHost(coordinator: self) }
                let token = WindowHostToken(epoch: request.epoch, identityNamespace: request.identityNamespace, windowId: request.windowId, hostId: UUID())
                host.bind(token, windowUUID: request.windowUuid, inheritLegacy: mainWindowHosts.isEmpty)
                mainWindowHosts[identity] = host
            }
            try reconcile(snapshot)
            if request.focus { host.installMenus(); host.window.makeKeyAndOrderFront(nil); NSApp.activate() }
            else if let key = NSApp.keyWindow, key !== host.window {
                host.window.order(.below, relativeTo: key.windowNumber)
            } else { host.window.orderFront(nil) }
            guard let token = host.nativeToken else { throw BridgeFailure(message: "Native window token is unavailable") }
            try await bridge.windowEvent(NativeWindowEvent(event: "complete", token: token, operationId: request.operationId))
            host.nativeReady = true
            if host.window.isKeyWindow { try await bridge.windowEvent(NativeWindowEvent(event: "key", token: token, key: true)) }
        case "focus":
            guard let host = mainWindowHosts[identity], let token = host.nativeToken, request.hostId == token.hostId else { throw NativeWindowPreflightFailure(message: "Focus target is not the admitted native host") }
            host.installMenus(); host.window.makeKeyAndOrderFront(nil); NSApp.activate()
            try await bridge.windowEvent(NativeWindowEvent(event: "complete", token: token, operationId: request.operationId))
        case "close":
            let host = mainWindowHosts[identity]
            let token = host?.nativeToken ?? WindowHostToken(epoch: request.epoch, identityNamespace: request.identityNamespace, windowId: request.windowId, hostId: UUID())
            guard request.hostId == nil || request.hostId == token.hostId else { throw NativeWindowPreflightFailure(message: "Closure target is not the admitted native host") }
            host?.detachContent(); host?.closeAuthorized()
            mainWindowHosts.removeValue(forKey: identity)
            try await bridge.windowEvent(NativeWindowEvent(event: "complete", token: token, operationId: request.operationId))
        case "terminate":
            let token = mainWindowHosts[identity]?.nativeToken ?? WindowHostToken(epoch: request.epoch, identityNamespace: request.identityNamespace, windowId: request.windowId, hostId: UUID())
            guard request.hostId == nil || request.hostId == token.hostId else { throw NativeWindowPreflightFailure(message: "Last close target is not the admitted native host") }
            // Acknowledge only the termination request. The unchanged model is saved
            // by the same Cmd-Q path; this does not claim successful persistence.
            try await bridge.windowEvent(NativeWindowEvent(event: "complete", token: token, operationId: request.operationId))
            if !Task.isCancelled { requestApplicationTermination() }
        default: throw NativeWindowPreflightFailure(message: "Unsupported native window request")
        }
    }

    private func recoverBrowserHostEvents() async {
        guard !Task.isCancelled, shutdownTask == nil else { return }
        browserEventsNeedRecovery = false
        browserEventGeneration &+= 1
        // Discard queued incremental events captured before this full-owner snapshot.
        // New events retain order after the snapshot while acknowledgements are awaited.
        let attachments = browsers.values.compactMap { $0.automationAttachment() }
        let observations = browsers.values.compactMap { $0.automationObservation() }
        for event in attachments + observations {
            if Task.isCancelled { return }
            do {
                if case .rejectedObsolete = try await bridge.browserEvent(event) {
                    WindowLifecycleSmoke.trace("Native browser recovery event rejected as obsolete; payload and downstream effects not applied")
                }
            }
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

    private func retainCompletedHistories(_ surfaces: Set<UInt64>, namespace: UUID) {
        if completedHistoryNamespace != namespace {
            completedHistories.removeAll()
            completedHistoryNamespace = namespace
        }
        completedHistories = completedHistories.filter { surfaces.contains($0.key) }
    }

    func completedHistory(surface: UInt64, history: CompletedLocalZellijHistory,
                          onFocus: @escaping () -> Void) -> CompletedTerminalHistoryView {
        if let retained = completedHistories[surface], retained.completionDigest == history.receipt.bindingDigest {
            retained.updateFocus(onFocus)
            return retained
        }
        let view = CompletedTerminalHistoryView(history: history, onFocus: onFocus)
        completedHistories[surface] = view
        return view
    }

    func downloadWindow(for origin: BrowserOwner) -> NSWindow? {
        guard let snapshot, snapshot.model.identityNamespace == origin.identityNamespace,
              browsers[origin.surfaceId]?.hostID == origin.hostId,
              let workspace = snapshot.model.workspaces.first(where: { $0.root.panes.contains(where: { $0.surfaces.contains(where: { $0.id == origin.surfaceId }) }) }),
              let window = snapshot.model.windows.first(where: { $0.workspaceIds.contains(workspace.id) }) else { return nil }
        return mainWindowHosts[MainWindowIdentity(namespace: snapshot.model.identityNamespace, windowId: window.id)]?.window
    }
}

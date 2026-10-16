import AppKit
import IOKit.pwr_mgt

/// Native presentation only. The shared Desktop owns notification records and
/// mutations; a menu retains the exact identities it displayed until it closes.
@MainActor
final class StatusMenuController: NSObject, NSMenuDelegate {
    private weak var coordinator: ApplicationCoordinator?
    private var statusItem: NSStatusItem?
    private let menu = NSMenu(title: "zmux")
    private var snapshot: DesktopSnapshot?
    private var tracking = false
    private var unreadCount: Int?
    private var awakeAssertion: IOPMAssertionID?
    private struct MenuState: Equatable {
        let epoch: UUID?
        let namespace: UUID?
        let ids: [UInt64]
        let read: [UInt64]
        let available: [UInt64]
    }
    private var renderedState: MenuState?

    private var menuState: MenuState {
        let records = snapshot?.notifications ?? []
        return MenuState(epoch: snapshot?.nativeNotifications.epoch, namespace: snapshot?.model.identityNamespace,
            ids: records.map(\.id), read: records.filter(\.read).map(\.id),
            available: records.filter { record in snapshot.map { record.target?.isAvailable(in: $0) == true } ?? false }.map(\.id))
    }

    init(coordinator: ApplicationCoordinator) {
        self.coordinator = coordinator
        super.init()
        menu.autoenablesItems = false
        menu.delegate = self
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        statusItem = item
        item.menu = menu
        item.button?.imagePosition = .imageOnly
        item.button?.imageScaling = .scaleNone
        item.button?.setAccessibilityLabel("zmux menu")
        update(nil)
    }

    func update(_ snapshot: DesktopSnapshot?) {
        self.snapshot = snapshot
        let unread = snapshot?.notifications.filter { !$0.read }.count ?? 0
        if unreadCount != unread {
            unreadCount = unread
            statusItem?.button?.image = Self.icon(attention: unread > 0)
        }
        let summary = unread == 0 ? "No unread notifications" : "\(unread) unread notification\(unread == 1 ? "" : "s")"
        statusItem?.button?.toolTip = "zmux — " + summary
        statusItem?.button?.setAccessibilityValue(summary)
        // Never replace payloads under a pointer or collapse an open submenu.
        if !tracking, renderedState != menuState { rebuild() }
    }

    func menuNeedsUpdate(_ menu: NSMenu) { if menu === self.menu, !tracking { rebuild() } }
    func menuWillOpen(_ menu: NSMenu) {
        guard menu === self.menu else { return }
        rebuild()
        tracking = true
    }
    func menuDidClose(_ menu: NSMenu) {
        guard menu === self.menu else { return }
        tracking = false
        // A quiet Desktop may publish no further snapshot after dismissal.
        // Apply changes coalesced during tracking now, rather than waiting for
        // another terminal event to refresh the closed menu's entries.
        if renderedState != menuState { rebuild() }
    }

    private func rebuild() {
        guard statusItem != nil else { return }
        renderedState = menuState
        menu.removeAllItems()
        let records = snapshot?.notifications ?? []
        let unread = records.filter { !$0.read }.count
        add(unread == 0 ? "No unread notifications" : "\(unread) unread notification\(unread == 1 ? "" : "s")",
            id: "summary", action: nil, enabled: false)
        menu.addItem(.separator())
        add("Show zmux", id: "show", action: #selector(showWindow), enabled: snapshot != nil)
        let awake = add("Keep Mac Awake", id: "awake", action: #selector(toggleAwake))
        awake.state = awakeAssertion == nil ? .off : .on
        menu.addItem(.separator())
        if let snapshot {
            for notification in records.suffix(8).reversed() {
                menu.addItem(Self.notificationItem(notification, snapshot: snapshot, target: self, action: #selector(openNotification(_:))))
            }
            if !records.isEmpty { menu.addItem(.separator()) }
            add("Show Notifications", id: "notifications", action: #selector(showNotifications))
            let jump = add("Jump to Latest Unread", id: "jump", action: #selector(jumpToUnread),
                           enabled: records.contains { !$0.read && $0.target?.isAvailable(in: snapshot) == true })
            jump.keyEquivalent = "u"; jump.keyEquivalentModifierMask = [.command, .shift]
            let read = add("Mark All Read", id: "read", action: #selector(mutate(_:)), enabled: unread > 0)
            read.representedObject = NotificationMenuMutationTarget(.init(snapshot: snapshot, mutation: .markRead))
            let clear = add("Clear All", id: "clear", action: #selector(mutate(_:)), enabled: !records.isEmpty)
            clear.representedObject = NotificationMenuMutationTarget(.init(snapshot: snapshot, mutation: .clear))
        }
        menu.addItem(.separator())
        add("Cloud Account…", id: "cloud", action: #selector(showCloud), enabled: snapshot != nil)
        add("Enable Desktop Notifications…", id: "permission", action: #selector(enableNotifications), enabled: snapshot != nil)
        menu.addItem(.separator())
        let quit = add("Quit zmux", id: "quit", action: #selector(quit))
        quit.keyEquivalent = "q"; quit.keyEquivalentModifierMask = .command
    }

    @discardableResult
    private func add(_ title: String, id: String, action: Selector?, enabled: Bool = true) -> NSMenuItem {
        let item = menu.addItem(withTitle: title, action: action, keyEquivalent: "")
        item.identifier = NSUserInterfaceItemIdentifier("zmux.status." + id)
        item.target = self; item.isEnabled = enabled
        return item
    }

    static func notificationItem(_ record: AttentionNotification, snapshot: DesktopSnapshot,
                                 target: AnyObject, action: Selector) -> NSMenuItem {
        let date = Date(timeIntervalSince1970: Double(record.timestampMs) / 1000)
            .formatted(date: .omitted, time: .shortened)
        let heading = (record.read ? "" : "● ") + shortLine(record.title, limit: 64) + "  " + date
        let detail = shortLine(record.body.isEmpty ? record.subtitle : record.body, limit: 100)
        let item = NSMenuItem(title: heading + (detail.isEmpty ? "" : "\n" + detail), action: action, keyEquivalent: "")
        item.identifier = NSUserInterfaceItemIdentifier("zmux.notification.\(record.id)")
        item.target = target
        item.isEnabled = record.target?.isAvailable(in: snapshot) == true
        if let destination = record.target { item.representedObject = NotificationMenuOpenTarget(destination) }
        item.toolTip = [record.title, record.subtitle, record.body].filter { !$0.isEmpty }.joined(separator: "\n")
        return item
    }

    private static func shortLine(_ text: String, limit: Int) -> String {
        let line = text.split(whereSeparator: { $0.isNewline || $0.isWhitespace }).joined(separator: " ")
        return String(line.prefix(limit)) + (line.count > limit ? "…" : "")
    }

    private static func icon(attention: Bool) -> NSImage {
        // Original compact terminal mark. Template rendering follows the macOS
        // menu bar appearance without baking either a dark or light background.
        let image = NSImage(size: NSSize(width: 18, height: 18), flipped: false) { _ in
            NSColor.black.setStroke(); NSColor.black.setFill()
            let chevron = NSBezierPath()
            chevron.lineWidth = 1.8; chevron.lineCapStyle = .round; chevron.lineJoinStyle = .round
            chevron.move(to: NSPoint(x: 3, y: 13)); chevron.line(to: NSPoint(x: 8, y: 9))
            chevron.line(to: NSPoint(x: 3, y: 5)); chevron.stroke()
            let cursor = NSBezierPath()
            cursor.lineWidth = 1.6; cursor.lineCapStyle = .round
            cursor.move(to: NSPoint(x: 10, y: 5)); cursor.line(to: NSPoint(x: 15, y: 5)); cursor.stroke()
            if attention { NSBezierPath(ovalIn: NSRect(x: 12, y: 11, width: 5, height: 5)).fill() }
            return true
        }
        image.isTemplate = true
        return image
    }

    @objc private func showWindow() { coordinator?.showFromStatusMenu() }
    @objc private func showNotifications() { coordinator?.showFromStatusMenu(notifications: true) }
    @objc private func showCloud() {
        coordinator?.showFromStatusMenu()
        coordinator?.mainWindowHost.showCloudSettings()
    }
    @objc private func jumpToUnread() { coordinator?.notificationDelivery.jumpToUnread() }
    @objc private func enableNotifications() {
        coordinator?.showFromStatusMenu()
        coordinator?.notificationDelivery.enable()
    }
    @objc private func openNotification(_ item: NSMenuItem) {
        guard let payload = item.representedObject as? NotificationMenuOpenTarget else { return }
        coordinator?.notificationDelivery.openInbox(payload.target)
    }
    @objc private func mutate(_ item: NSMenuItem) {
        guard let payload = item.representedObject as? NotificationMenuMutationTarget else { return }
        coordinator?.enqueue(.notificationMutation(payload.selection))
    }
    @objc private func quit() { coordinator?.requestApplicationTermination() }

    @objc private func toggleAwake() {
        if awakeAssertion != nil { releaseAwake() }
        else {
            var assertion: IOPMAssertionID = 0
            let result = IOPMAssertionCreateWithName(kIOPMAssertionTypeNoIdleSleep as CFString,
                IOPMAssertionLevel(kIOPMAssertionLevelOn), "zmux Keep Mac Awake" as CFString, &assertion)
            if result == kIOReturnSuccess { awakeAssertion = assertion }
            else { coordinator?.reportError("Unable to keep this Mac awake (\(result))") }
        }
        if let item = menu.items.first(where: { $0.identifier?.rawValue == "zmux.status.awake" }) {
            item.state = awakeAssertion == nil ? .off : .on
        }
    }
    private func releaseAwake() {
        guard let assertion = awakeAssertion else { return }
        let result = IOPMAssertionRelease(assertion)
        if result == kIOReturnSuccess { awakeAssertion = nil }
        else { coordinator?.reportError("Unable to release zmux's keep-awake assertion (\(result))") }
    }

    func remove() {
        guard let item = statusItem else { return }
        releaseAwake()
        menu.cancelTracking()
        menu.delegate = nil
        item.menu = nil
        NSStatusBar.system.removeStatusItem(item)
        statusItem = nil; snapshot = nil
    }
}

final class NotificationMenuOpenTarget: NSObject {
    let target: NotificationTarget
    init(_ target: NotificationTarget) { self.target = target }
}
final class NotificationMenuMutationTarget: NSObject {
    let selection: NotificationMenuSelection
    init(_ selection: NotificationMenuSelection) { self.selection = selection }
}

final class WorkspaceContextTarget: NSObject {
    let namespace: UUID
    let workspace: UInt64
    let uuid: UUID
    init(namespace: UUID, workspace: UInt64, uuid: UUID) {
        self.namespace = namespace; self.workspace = workspace; self.uuid = uuid
    }
}

// Packaged native acceptance uses the actual status button, menu targets, CLI,
// shared Desktop and AppKit windows. No alternate notification store is injected.
extension StatusMenuController {
    func validateNativeMenu(report: URL, contextMenu: @MainActor (UInt64) -> NSMenu?,
                            cli: @MainActor ([String]) async throws -> Data) async throws {
        guard let coordinator, let initial = coordinator.snapshot, let sourceWindow = initial.model.focusedWindow,
              let selected = initial.model.focusedWorkspace,
              let other = initial.model.workspaces.first(where: { $0.id != selected }),
              let item = statusItem, let button = item.button, button.window != nil,
              button.bounds.width > 0, button.image?.isTemplate == true else {
            throw BridgeFailure(message: "No visible native status button or inactive notification workspace")
        }
        let identity = ObjectIdentifier(self)
        let processes = Dictionary(uniqueKeysWithValues: initial.frames.map { ($0.sessionId, $0.processId) })
        func wait(_ message: String, _ predicate: @MainActor () -> Bool) async throws {
            let deadline = ContinuousClock.now + .seconds(10)
            while !predicate() {
                guard ContinuousClock.now < deadline else { throw BridgeFailure(message: message) }
                try await Task.sleep(for: .milliseconds(20))
            }
        }
        func entry(_ menu: NSMenu, _ id: String) throws -> NSMenuItem {
            menu.update()
            guard let item = menu.items.first(where: { $0.identifier?.rawValue == id }) else {
                let entries = menu.items.compactMap { $0.identifier?.rawValue }.joined(separator: ", ")
                throw BridgeFailure(message: "Native menu action missing: \(id); entries: \(entries)")
            }
            guard item.isEnabled else { throw BridgeFailure(message: "Native menu action disabled: " + id) }
            return item
        }
        func invoke(_ menu: NSMenu, _ item: NSMenuItem) throws {
            let index = menu.index(of: item)
            guard index >= 0, item.isEnabled else { throw BridgeFailure(message: "Captured native menu action was replaced") }
            menu.performActionForItem(at: index)
        }
        // A real NSStatusBarButton opens the system menu. Close it on a common
        // tracking-mode timer so acceptance needs no Accessibility permission.
        var opened = false
        let timer = Timer(timeInterval: 0.3, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated {
                opened = self?.tracking == true
                self?.menu.cancelTracking()
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        RunLoop.main.add(timer, forMode: .eventTracking)
        button.performClick(nil)
        timer.invalidate()
        guard opened else { throw BridgeFailure(message: "Status button did not open the actual system menu") }
        if let image = button.bitmapImageRepForCachingDisplay(in: button.bounds) {
            button.cacheDisplay(in: button.bounds, to: image)
            guard let png = image.representation(using: .png, properties: [:]) else {
                throw BridgeFailure(message: "Actual status icon screenshot unavailable")
            }
            try png.write(to: report.deletingLastPathComponent().appendingPathComponent("zmux-status-icon.png"), options: .atomic)
        } else { throw BridgeFailure(message: "Actual status button has no drawing surface") }

        _ = try await cli(["new-window", "--no-focus"])
        try await wait("Second native window did not become ready") {
            coordinator.mainWindowHosts.count == 2 && coordinator.mainWindowHosts.values.allSatisfy(\.nativeReady)
        }
        guard let second = coordinator.snapshot?.model.windows.first(where: { $0.id != sourceWindow }),
              let foreign = second.workspaceIds.first else { throw BridgeFailure(message: "Second window has no workspace") }
        func notify(_ workspace: UInt64, _ title: String) async throws {
            _ = try await cli(["notify", "--workspace", String(workspace), "--title", title, "--body", "Status menu acceptance", "--desktop", "false"])
            try await wait("Notification did not reach the shared snapshot") {
                coordinator.snapshot?.notifications.contains(where: { $0.title == title }) == true
            }
        }
        try await notify(other.id, "Workspace context notification")
        try await notify(foreign, "Other window notification")
        guard let scoped = contextMenu(other.id) else { throw BridgeFailure(message: "Inactive workspace has no real context menu") }
        try invoke(scoped, entry(scoped, "zmux.workspace.copy-id"))
        guard let uuid = initial.workspaceIdentities.first(where: { $0.workspaceId == other.id })?.uuid,
              NSPasteboard.general.string(forType: .string) == uuid.uuidString.lowercased() else {
            throw BridgeFailure(message: "Workspace clipboard action copied the wrong identity")
        }
        try invoke(scoped, entry(scoped, "zmux.workspace.read"))
        try await wait("Workspace menu did not mark its notification read") {
            coordinator.snapshot?.notifications.first(where: { $0.workspaceId == other.id })?.read == true
        }
        guard coordinator.snapshot?.model.focusedWorkspace == selected,
              coordinator.snapshot?.notifications.first(where: { $0.workspaceId == foreign })?.read == false else {
            throw BridgeFailure(message: "Workspace context menu changed focus or another window's attention")
        }
        try invoke(scoped, entry(scoped, "zmux.workspace.clear-latest"))
        try await wait("Clear Latest did not remove its exact workspace record") {
            coordinator.snapshot?.notifications.contains(where: { $0.workspaceId == other.id }) == false
        }
        try await notify(other.id, "Captured status notification")
        menuWillOpen(menu)
        defer { menuDidClose(menu) }
        let read = try entry(menu, "zmux.status.read")
        let clear = try entry(menu, "zmux.status.clear")
        try await notify(foreign, "Notification after menu opening")
        guard menu.index(of: read) >= 0 else { throw BridgeFailure(message: "Live updates replaced an open status menu") }
        try invoke(menu, read)
        try await wait("Captured mark-read did not complete") {
            let records = coordinator.snapshot?.notifications ?? []
            return records.filter { $0.title != "Notification after menu opening" }.allSatisfy(\.read)
                && records.first(where: { $0.title == "Notification after menu opening" })?.read == false
        }
        try invoke(menu, clear)
        try await wait("Captured clear consumed a new arrival or retained older records") {
            coordinator.snapshot?.notifications.count == 1 && coordinator.snapshot?.notifications.first?.title == "Notification after menu opening"
        }
        guard coordinator.snapshot?.model.focusedWorkspace == selected else {
            throw BridgeFailure(message: "Status attention mutation stole workspace focus")
        }
        menuDidClose(menu)
        guard let remaining = coordinator.snapshot?.notifications.first else { throw BridgeFailure(message: "New arrival disappeared") }
        try invoke(menu, entry(menu, "zmux.notification.\(remaining.id)"))
        try await wait("Status preview did not open the exact other-window target") {
            coordinator.snapshot?.model.focusedWorkspace == foreign && coordinator.mainWindowHost.window.isKeyWindow
                && coordinator.snapshot?.notifications.first?.read == true
        }
        try invoke(menu, entry(menu, "zmux.status.notifications"))
        func hasNotificationRow(_ view: NSView) -> Bool {
            view is NotificationRowView || view.subviews.contains(where: hasNotificationRow)
        }
        guard let content = coordinator.mainWindowHost.window.contentView, hasNotificationRow(content) else {
            throw BridgeFailure(message: "Show Notifications did not expose the actual inbox")
        }
        let host = coordinator.mainWindowHost
        host.window.miniaturize(nil)
        try await wait("Fixture window did not minimize") { host.window.isMiniaturized }
        try invoke(menu, entry(menu, "zmux.status.show"))
        try await wait("Show zmux did not restore the minimized window") { !host.window.isMiniaturized && host.window.isKeyWindow }
        try invoke(menu, entry(menu, "zmux.status.awake"))
        guard awakeAssertion != nil else { throw BridgeFailure(message: "Keep Mac Awake failed to acquire a power assertion") }
        try invoke(menu, entry(menu, "zmux.status.awake"))
        guard awakeAssertion == nil else { throw BridgeFailure(message: "Keep Mac Awake failed to release its power assertion") }
        guard coordinator.statusMenu.map(ObjectIdentifier.init) == identity,
              processes.allSatisfy({ id, pid in coordinator.snapshot?.frames.first(where: { $0.sessionId == id })?.processId == pid }) else {
            throw BridgeFailure(message: "Menu actions replaced the application status owner or restarted an existing child")
        }
        // Regular mode deliberately saves/quits on final-window close. Exercise
        // the real hidden-application state here; last-close termination retains
        // its existing shared/window lifecycle acceptance rather than assuming
        // an unimplemented menu-bar-only mode.
        NSApp.hide(nil)
        try await wait("Application did not hide") { NSApp.isHidden }
        try invoke(menu, entry(menu, "zmux.status.show"))
        try await wait("Status menu did not restore the hidden application") {
            !NSApp.isHidden && coordinator.mainWindowHost.window.isVisible && coordinator.mainWindowHost.window.isKeyWindow
        }
        guard coordinator.statusMenu.map(ObjectIdentifier.init) == identity, statusItem === item,
              processes.allSatisfy({ id, pid in coordinator.snapshot?.frames.first(where: { $0.sessionId == id })?.processId == pid }) else {
            throw BridgeFailure(message: "Application hide/show replaced the status item or restarted an existing child")
        }
        let result: [String: Bool] = ["native_status_button": true, "system_menu_opened": true,
            "workspace_context_actions": true, "captured_mutations": true, "cross_window_notification_open": true,
            "inbox_presentation": true, "minimized_window_restore": true, "keep_awake_toggle": true,
            "single_application_owner": true, "hidden_window_recovery": true]
        try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys]).write(to: report, options: .atomic)
    }

    func verifyRemovedAfterShutdown() throws {
        guard statusItem == nil, awakeAssertion == nil, snapshot == nil, menu.delegate == nil else {
            throw BridgeFailure(message: "Application shutdown retained a native status item or power assertion")
        }
    }
}

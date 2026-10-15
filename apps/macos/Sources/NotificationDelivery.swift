import AppKit
import Foundation
import UserNotifications

// Original MIT native adapter. Rust owns inbox identity and delivery eligibility.
enum NotificationAuthorization: String, Codable, Sendable {
    case unknown, notDetermined = "not_determined", denied, authorized, provisional, ephemeral, unavailable
    var permitsDelivery: Bool { self == .authorized || self == .provisional || self == .ephemeral }
}

/// The serial permission sender may coalesce grants, but never cross an unacknowledged
/// restriction. A send acknowledgement and a new authoritative desired snapshot are
/// different boundaries: only the latter makes cached delivery eligibility safe again.
struct PermissionObservations: Sendable {
    struct Observation: Sendable, Equatable {
        let epoch: UUID
        let generation: UInt64
        let state: NotificationAuthorization
        var event: NativeNotificationEvent { .authorization(epoch: epoch, generation: generation, state: state) }
    }
    private(set) var epoch: UUID?
    private(set) var snapshotGeneration: UInt64 = 0
    private(set) var restrictiveGeneration: UInt64 = 0
    private var confirmedGeneration: UInt64 = 0
    private var barrier: Observation?
    private var latest: Observation?

    init() {}

    var next: Observation? { barrier ?? latest }
    var blocksDelivery: Bool { barrier != nil || snapshotGeneration < restrictiveGeneration }

    mutating func updateSnapshot(epoch: UUID, generation: UInt64) {
        if self.epoch != epoch {
            self = Self()
            self.epoch = epoch
        }
        snapshotGeneration = max(snapshotGeneration, generation)
        confirm(epoch: epoch, generation: generation)
    }
    mutating func observe(epoch: UUID, generation: UInt64, state: NotificationAuthorization) {
        guard self.epoch == epoch, generation > confirmedGeneration else { return }
        let observation = Observation(epoch: epoch, generation: generation, state: state)
        if latest.map({ generation > $0.generation }) ?? true { latest = observation }
        if !state.permitsDelivery {
            restrictiveGeneration = max(restrictiveGeneration, generation)
            if barrier == nil { barrier = observation }
        }
    }
    mutating func confirm(epoch: UUID, generation: UInt64) {
        guard self.epoch == epoch else { return }
        confirmedGeneration = max(confirmedGeneration, generation)
        if let barrier, barrier.generation <= confirmedGeneration { self.barrier = nil }
        if let latest, latest.generation <= confirmedGeneration { self.latest = nil }
        if barrier == nil, let latest, !latest.state.permitsDelivery { barrier = latest }
    }
}

private struct NotificationWireKey: CodingKey {
    let stringValue: String
    let intValue: Int? = nil
    init(stringValue: String) { self.stringValue = stringValue }
    init?(intValue: Int) { nil }
}

private func requireNotificationKeys(_ decoder: any Decoder, allowed: Set<String>) throws {
    let container = try decoder.container(keyedBy: NotificationWireKey.self)
    guard container.allKeys.allSatisfy({ allowed.contains($0.stringValue) }) else {
        throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Unexpected notification destination fields"))
    }
}

enum NotificationDestination: Codable, Sendable, Equatable {
    case workspace(workspaceId: UInt64)
    case surface(surfaceId: UInt64, sessionId: UInt64?)

    private enum CodingKeys: String, CodingKey { case kind, workspaceId, surfaceId, sessionId }
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "workspace":
            try requireNotificationKeys(decoder, allowed: ["kind", "workspaceId"])
            let id = try c.decode(UInt64.self, forKey: .workspaceId)
            guard id > 0 else { throw DecodingError.dataCorruptedError(forKey: .workspaceId, in: c, debugDescription: "Workspace identity must be positive") }
            self = .workspace(workspaceId: id)
        case "surface":
            try requireNotificationKeys(decoder, allowed: ["kind", "surfaceId", "sessionId"])
            let id = try c.decode(UInt64.self, forKey: .surfaceId)
            guard id > 0, c.contains(.sessionId) else { throw DecodingError.dataCorruptedError(forKey: .surfaceId, in: c, debugDescription: "Surface identity and explicit session ownership are required") }
            let session = try c.decodeIfPresent(UInt64.self, forKey: .sessionId)
            guard session != 0 else { throw DecodingError.dataCorruptedError(forKey: .sessionId, in: c, debugDescription: "Session identity must be positive or null") }
            self = .surface(surfaceId: id, sessionId: session)
        default:
            throw DecodingError.dataCorruptedError(forKey: .kind, in: c, debugDescription: "Unsupported notification destination")
        }
    }
    func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .workspace(let id):
            try c.encode("workspace", forKey: .kind); try c.encode(id, forKey: .workspaceId)
        case .surface(let id, let session):
            try c.encode("surface", forKey: .kind); try c.encode(id, forKey: .surfaceId)
            if let session { try c.encode(session, forKey: .sessionId) }
            else { try c.encodeNil(forKey: .sessionId) }
        }
    }

    var openLabel: String {
        switch self { case .workspace: "Open workspace"; case .surface: "Open tab" }
    }
    var unavailableLabel: String {
        switch self { case .workspace: "Workspace closed"; case .surface: "Tab closed" }
    }
}

struct NotificationTarget: Codable, Sendable, Equatable {
    let epoch: UUID
    let identityNamespace: UUID
    let notificationId: UInt64
    let destination: NotificationDestination

    private enum CodingKeys: String, CodingKey { case epoch, identityNamespace, notificationId, destination }
    init(from decoder: any Decoder) throws {
        try requireNotificationKeys(decoder, allowed: ["epoch", "identityNamespace", "notificationId", "destination"])
        let c = try decoder.container(keyedBy: CodingKeys.self)
        epoch = try c.decode(UUID.self, forKey: .epoch)
        identityNamespace = try c.decode(UUID.self, forKey: .identityNamespace)
        notificationId = try c.decode(UInt64.self, forKey: .notificationId)
        destination = try c.decode(NotificationDestination.self, forKey: .destination)
        guard notificationId > 0 else { throw DecodingError.dataCorruptedError(forKey: .notificationId, in: c, debugDescription: "Notification identity must be positive") }
    }

    func isAvailable(in snapshot: DesktopSnapshot) -> Bool {
        guard epoch == snapshot.nativeNotifications.epoch, identityNamespace == snapshot.model.identityNamespace else { return false }
        switch destination {
        case .workspace(let id): return snapshot.model.workspaces.contains { $0.id == id }
        case .surface(let id, let session):
            return snapshot.model.workspaces.flatMap { $0.root.panes }.flatMap { $0.surfaces }.contains { $0.id == id && $0.sessionId == session }
        }
    }

    func matches(_ receipt: NativeNotificationExecution) -> Bool {
        switch destination {
        case .workspace(let id): receipt.focusedWorkspaceId == id
        case .surface(let id, _): receipt.focusedSurfaceId == id
        }
    }
}

struct NativeNotificationRequest: Codable, Sendable, Equatable {
    let identifier: String
    let target: NotificationTarget
    let authorizationGeneration: UInt64
    let title: String
    let subtitle: String
    let body: String
    let replyEnabled: Bool
}

struct NativeNotificationSnapshot: Codable, Sendable {
    let epoch: UUID
    let authorization: NotificationAuthorization
    let authorizationGeneration: UInt64
    let desired: [NativeNotificationRequest]
}

struct NativeNotificationExecution: Decodable, Sendable {
    let revision: UInt64
    let focusedWorkspaceId: UInt64?
    let focusedSurfaceId: UInt64?
    let navigation: NotificationNavigation?
    let reply: NotificationReply?
}

struct NotificationReply: Decodable, Sendable {
    enum Outcome: String, Decodable, Sendable { case complete, empty, notWritten = "not_written", partial, unknown }
    let target: NotificationTarget
    let outcome: Outcome
    let bytesWritten: UInt64
    let totalBytes: UInt64
    let reason: String?
}

struct NotificationNavigation: Decodable, Sendable {
    let opened: Bool
    let notificationId: UInt64?
    let notificationUuid: UUID?
    let target: NotificationTarget?
}

enum NativeNotificationEvent: Encodable, Sendable {
    case authorization(epoch: UUID, generation: UInt64, state: NotificationAuthorization)
    case scheduled(target: NotificationTarget, authorizationGeneration: UInt64)
    case failed(target: NotificationTarget, authorizationGeneration: UInt64, message: String)

    private enum CodingKeys: String, CodingKey { case event, epoch, generation, state, target, authorizationGeneration, message }
    func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .authorization(let epoch, let generation, let state):
            try c.encode("authorization", forKey: .event)
            try c.encode(epoch, forKey: .epoch); try c.encode(generation, forKey: .generation); try c.encode(state, forKey: .state)
        case .scheduled(let target, let generation):
            try c.encode("scheduled", forKey: .event)
            try c.encode(target, forKey: .target); try c.encode(generation, forKey: .authorizationGeneration)
        case .failed(let target, let generation, let message):
            try c.encode("failed", forKey: .event)
            try c.encode(target, forKey: .target); try c.encode(generation, forKey: .authorizationGeneration)
            try c.encode(message, forKey: .message)
        }
    }
}

enum NativeAttentionAction: Encodable, Sendable {
    case open(target: NotificationTarget)
    case openInbox(target: NotificationTarget)
    case dismiss(target: NotificationTarget)
    case jumpToUnread
    case reply(target: NotificationTarget, text: String)
    private enum CodingKeys: String, CodingKey { case action, target, text }
    func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .open(let target): try c.encode("open", forKey: .action); try c.encode(target, forKey: .target)
        case .openInbox(let target): try c.encode("open_inbox", forKey: .action); try c.encode(target, forKey: .target)
        case .dismiss(let target): try c.encode("dismiss", forKey: .action); try c.encode(target, forKey: .target)
        case .jumpToUnread: try c.encode("jump_to_unread", forKey: .action)
        case .reply(let target, let text):
            try c.encode("reply", forKey: .action); try c.encode(target, forKey: .target)
            try c.encode(text, forKey: .text)
        }
    }
}

@MainActor
final class NotificationDelivery: NSObject, UNUserNotificationCenterDelegate {
    private struct Pending {
        let request: NativeNotificationRequest
        var expired = false
        var timer: Task<Void, Never>?
    }
    private let center = UNUserNotificationCenter.current()
    private let sendEvent: @MainActor (NativeNotificationEvent) async throws -> Void
    private let performAction: @MainActor (NativeAttentionAction) async throws -> NativeNotificationExecution
    private let activate: @MainActor () -> Void
    private let reportError: @MainActor (String) -> Void
    private var epoch: UUID?
    private var authorizationGeneration: UInt64 = 0
    private var permissionObservations = PermissionObservations()
    private var observedAuthorization = NotificationAuthorization.unknown
    private var snapshotAuthorization = NotificationAuthorization.unknown
    private var desired: [String: NativeNotificationRequest] = [:]
    private var attempted: [String: UInt64] = [:]
    // An expired UN callback still owns its slot until the OS actually calls it.
    private var pending: [String: Pending] = [:]
    private var events: [NativeNotificationEvent] = []
    private var eventTask: Task<Void, Never>?
    private var settingsInFlight = false
    private var refreshRequested = false
    private var authorizationInFlight = false
    private var actionsInFlight = 0
    // Runtime tombstones remain authoritative after this bounded callback cache evicts.
    private var replyAttempts: [NotificationTarget] = []
    private static let replyActionIdentifier = "zmux.reply"
    private var stopped = false

    init(sendEvent: @escaping @MainActor (NativeNotificationEvent) async throws -> Void,
         performAction: @escaping @MainActor (NativeAttentionAction) async throws -> NativeNotificationExecution,
         activate: @escaping @MainActor () -> Void,
         reportError: @escaping @MainActor (String) -> Void) {
        self.sendEvent = sendEvent; self.performAction = performAction
        self.activate = activate; self.reportError = reportError
        super.init()
        center.delegate = self
        let reply = UNTextInputNotificationAction(identifier: Self.replyActionIdentifier, title: "Reply", options: [], textInputButtonTitle: "Send", textInputPlaceholder: "Reply to terminal")
        center.setNotificationCategories([
            UNNotificationCategory(identifier: "zmux.inbox", actions: [], intentIdentifiers: [], options: [.customDismissAction]),
            UNNotificationCategory(identifier: "zmux.inbox.reply", actions: [reply], intentIdentifiers: [], options: [.customDismissAction])
        ])
        // A previous process's tokens must never target this process's inbox.
        center.removeAllPendingNotificationRequests()
        center.removeAllDeliveredNotifications()
    }

    func update(_ snapshot: NativeNotificationSnapshot) {
        guard !stopped else { return }
        if epoch != snapshot.epoch {
            remove(Array(desired.keys))
            desired.removeAll(); attempted.removeAll(); replyAttempts.removeAll()
            epoch = snapshot.epoch
            observedAuthorization = .unknown
        }
        authorizationGeneration = max(authorizationGeneration, snapshot.authorizationGeneration)
        permissionObservations.updateSnapshot(epoch: snapshot.epoch, generation: snapshot.authorizationGeneration)
        snapshotAuthorization = snapshot.authorization
        guard snapshot.desired.count <= 128 else {
            reportError("Native notification snapshot exceeded its limit; delivery is paused")
            remove(Array(desired.keys)); desired.removeAll()
            return
        }
        var next: [String: NativeNotificationRequest] = [:]
        for request in snapshot.desired {
            guard request.target.epoch == snapshot.epoch,
                  request.identifier == Self.identifier(request.target),
                  !request.replyEnabled || Self.isTerminal(request.target),
                  next.updateValue(request, forKey: request.identifier) == nil else {
                reportError("Invalid native notification snapshot; delivery is paused")
                remove(Array(desired.keys)); desired.removeAll()
                return
            }
        }
        let obsolete = desired.keys.filter { next[$0] != desired[$0] }
        remove(Array(obsolete))
        for identifier in obsolete { attempted.removeValue(forKey: identifier) }
        desired = next
        if !snapshot.authorization.permitsDelivery { remove(Array(desired.keys)) }
        pump()
        if observedAuthorization == .unknown { refreshPermission() }
    }

    /// Activation refresh never prompts. Only enable() may request OS permission.
    func refreshPermission() {
        guard !stopped else { return }
        if settingsInFlight || authorizationInFlight { refreshRequested = true; return }
        guard let epoch,
              authorizationGeneration < UInt64.max else { return }
        refreshRequested = false
        authorizationGeneration += 1
        let generation = authorizationGeneration
        settingsInFlight = true
        // UserNotifications invokes completion blocks on its own queue. Explicit
        // Sendable boundaries prevent inherited main-actor entry checks; only
        // value observations cross into the main-actor task below.
        center.getNotificationSettings { @Sendable [weak self] settings in
            let state: NotificationAuthorization
            switch settings.authorizationStatus {
            case .notDetermined: state = .notDetermined
            case .denied: state = .denied
            case .authorized: state = .authorized
            case .provisional: state = .provisional
            @unknown default: state = .unavailable
            }
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.settingsInFlight = false
                guard !self.stopped, self.epoch == epoch, self.authorizationGeneration == generation else {
                    if !self.stopped { self.refreshPermission() }
                    return
                }
                self.observedAuthorization = state
                if !state.permitsDelivery { self.remove(Array(self.desired.keys)) }
                self.emit(.authorization(epoch: epoch, generation: generation, state: state))
                self.pump()
                if self.refreshRequested { self.refreshPermission() }
            }
        }
    }

    func enable() {
        guard !stopped, !authorizationInFlight else { return }
        guard NSApp.isActive else { reportError("Activate zmux before enabling system notifications"); return }
        authorizationInFlight = true
        center.requestAuthorization(options: [.alert, .sound, .badge]) { @Sendable [weak self] _, error in
            let message = error.map { String($0.localizedDescription.prefix(512)) }
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.authorizationInFlight = false
                guard !self.stopped else { return }
                if let message { self.reportError("Notification permission: " + message) }
                self.refreshPermission()
            }
        }
    }

    func openInbox(_ target: NotificationTarget) {
        performInboxAction(.openInbox(target: target), expectedEpoch: target.epoch) { target.matches($0) }
    }

    func jumpToUnread() {
        guard let epoch else { reportError("Notification actions are unavailable"); return }
        performInboxAction(.jumpToUnread, expectedEpoch: epoch) { receipt in
            guard let navigation = receipt.navigation, navigation.opened,
                  let target = navigation.target, target.epoch == epoch,
                  navigation.notificationId == target.notificationId else { return false }
            return target.matches(receipt)
        }
    }

    private func performInboxAction(_ action: NativeAttentionAction, expectedEpoch: UUID,
                                    accepts: @escaping @MainActor (NativeNotificationExecution) -> Bool) {
        guard !stopped, epoch == expectedEpoch, actionsInFlight < 8 else {
            reportError("Notification actions are unavailable or busy")
            return
        }
        actionsInFlight += 1
        Task { [weak self] in
            guard let self else { return }
            defer { self.actionsInFlight -= 1 }
            do {
                let receipt = try await self.performAction(action)
                guard !self.stopped, self.epoch == expectedEpoch, accepts(receipt) else { return }
                self.activate()
            } catch { self.reportError("Notification target is unavailable: " + error.localizedDescription) }
        }
    }

    func stop() {
        guard !stopped else { return }
        stopped = true
        remove(Array(Set(desired.keys).union(pending.keys)))
        desired.removeAll(); attempted.removeAll(); replyAttempts.removeAll(); events.removeAll()
        for identifier in pending.keys { pending[identifier]?.timer?.cancel() }
        eventTask?.cancel(); eventTask = nil
        center.delegate = nil
    }

    private static func identifier(_ target: NotificationTarget) -> String {
        "zmux.notification.\(target.epoch.uuidString.lowercased()).\(target.notificationId)"
    }
    private static func isTerminal(_ target: NotificationTarget) -> Bool {
        if case .surface(_, let session) = target.destination { return session != nil }
        return false
    }
    private func remove(_ identifiers: [String]) {
        guard !identifiers.isEmpty else { return }
        center.removePendingNotificationRequests(withIdentifiers: identifiers)
        center.removeDeliveredNotifications(withIdentifiers: identifiers)
    }
    private func current(_ request: NativeNotificationRequest) -> Bool {
        !stopped && observedAuthorization.permitsDelivery && snapshotAuthorization.permitsDelivery
            && !permissionObservations.blocksDelivery
            && request.authorizationGeneration > 0 && desired[request.identifier] == request
    }
    private func pump() {
        guard !stopped, !permissionObservations.blocksDelivery,
              observedAuthorization.permitsDelivery, snapshotAuthorization.permitsDelivery else { return }
        for request in desired.values.sorted(by: { $0.target.notificationId < $1.target.notificationId }) {
            guard pending.count < 8 else { break }
            guard current(request), pending[request.identifier] == nil,
                  attempted[request.identifier] != request.authorizationGeneration else { continue }
            attempted[request.identifier] = request.authorizationGeneration
            let content = UNMutableNotificationContent()
            content.title = String(request.title.prefix(128)); content.subtitle = String(request.subtitle.prefix(128))
            content.body = String(request.body.prefix(2048)); content.sound = .default
            content.categoryIdentifier = request.replyEnabled ? "zmux.inbox.reply" : "zmux.inbox"
            pending[request.identifier] = Pending(request: request)
            pending[request.identifier]?.timer = Task { [weak self] in
                do { try await Task.sleep(for: .seconds(5)) } catch { return }
                guard let self, var pending = self.pending[request.identifier] else { return }
                pending.expired = true; self.pending[request.identifier] = pending
                self.remove([request.identifier])
                if self.current(request) { self.emit(.failed(target: request.target, authorizationGeneration: request.authorizationGeneration, message: "System notification scheduling timed out; it will not be replayed")) }
            }
            let identifier = request.identifier
            center.add(UNNotificationRequest(identifier: identifier, content: content, trigger: nil)) { @Sendable [weak self] error in
                let message = error.map { String($0.localizedDescription.prefix(512)) }
                Task { @MainActor [weak self] in self?.scheduled(identifier, error: message) }
            }
        }
    }
    private func scheduled(_ identifier: String, error: String?) {
        guard let callback = pending.removeValue(forKey: identifier) else { return }
        callback.timer?.cancel()
        if callback.expired || !current(callback.request) { remove([identifier]) }
        else if let error {
            remove([identifier])
            emit(.failed(target: callback.request.target, authorizationGeneration: callback.request.authorizationGeneration, message: error))
        } else {
            emit(.scheduled(target: callback.request.target, authorizationGeneration: callback.request.authorizationGeneration))
        }
        pump()
    }
    private func emit(_ event: NativeNotificationEvent) {
        guard !stopped else { return }
        if case .authorization(let epoch, let generation, let state) = event {
            permissionObservations.observe(epoch: epoch, generation: generation, state: state)
        } else {
            if events.count < 256 { events.append(event) }
            else { reportError("Native notification receipt queue is full") }
        }
        guard eventTask == nil else { return }
        eventTask = Task { [weak self] in
            guard let self else { return }
            while !Task.isCancelled, !self.stopped {
                let event: NativeNotificationEvent
                if let observation = self.permissionObservations.next {
                    event = observation.event
                } else if !self.events.isEmpty { event = self.events.removeFirst() }
                else { break }
                guard self.eventIsCurrent(event) else { continue }
                do {
                    try await self.sendEvent(event)
                    if case .authorization(let epoch, let generation, _) = event {
                        self.permissionObservations.confirm(epoch: epoch, generation: generation)
                    }
                }
                catch {
                    self.reportError("Notification synchronization: " + error.localizedDescription)
                    switch event {
                    case .authorization: break // Keep the exact barrier; a snapshot can acknowledge a lost reply.
                    case .scheduled, .failed:
                        if self.eventIsCurrent(event), self.events.count < 256 { self.events.insert(event, at: 0) }
                    }
                    // Retry exact permission observations and receipts, never OS adds or actions.
                    do { try await Task.sleep(for: .milliseconds(250)) } catch { break }
                }
            }
            self.eventTask = nil
        }
    }
    private func eventIsCurrent(_ event: NativeNotificationEvent) -> Bool {
        switch event {
        case .authorization(let epoch, let generation, _):
            return self.epoch == epoch && permissionObservations.next?.generation == generation
        case .scheduled(let target, let generation), .failed(let target, let generation, _):
            return desired.values.contains { $0.target == target && $0.authorizationGeneration == generation && current($0) }
        }
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification) async -> UNNotificationPresentationOptions {
        let identifier = notification.request.identifier
        return await presentation(identifier)
    }
    private func presentation(_ identifier: String) -> UNNotificationPresentationOptions {
        guard let request = desired[identifier], current(request), pending[identifier]?.expired != true else {
            remove([identifier]); return []
        }
        return [.banner, .list, .sound]
    }
    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        let identifier = response.notification.request.identifier
        let action = response.actionIdentifier
        // Only Sendable values cross the framework callback's actor boundary. Reject
        // oversized raw UTF-8 rather than truncating a command. Rust appends the CR.
        let rawText = (response as? UNTextInputNotificationResponse)?.userText
        let oversized = rawText.map { $0.utf8.prefix(4096).count > 4095 } ?? false
        let text = oversized ? nil : rawText
        await receive(identifier, action: action, text: text, oversized: oversized)
    }
    private func receive(_ identifier: String, action: String, text: String?, oversized: Bool) async {
        guard !stopped, actionsInFlight < 8, let request = desired[identifier], current(request) else { remove([identifier]); return }
        guard action == UNNotificationDefaultActionIdentifier || action == UNNotificationDismissActionIdentifier || action == Self.replyActionIdentifier else { return }
        if action == Self.replyActionIdentifier {
            guard request.replyEnabled, Self.isTerminal(request.target), !oversized, text != nil else {
                reportError("Notification reply is unavailable or exceeds 4095 UTF-8 bytes")
                return
            }
            guard !replyAttempts.contains(request.target) else { return }
            if replyAttempts.count == 256 { replyAttempts.removeFirst() }
            replyAttempts.append(request.target) // Claim before suspension; never retry a lost reply.
        }
        actionsInFlight += 1
        defer { actionsInFlight -= 1 }
        do {
            if action == Self.replyActionIdentifier, let text {
                let receipt = try await performAction(.reply(target: request.target, text: text))
                guard let reply = receipt.reply, reply.target == request.target,
                      reply.bytesWritten <= reply.totalBytes,
                      reply.totalBytes <= UInt64(text.utf8.count + 1) else {
                    throw BridgeFailure(message: "Notification reply returned an invalid receipt; it will not be resent")
                }
                guard !stopped, epoch == request.target.epoch else { return }
                switch reply.outcome {
                case .complete:
                    // Rust owns Unicode trimming and the appended CR. A nonempty
                    // canonical payload has at least one text byte and one CR.
                    guard reply.bytesWritten == reply.totalBytes, reply.totalBytes >= 2, receipt.navigation == nil else {
                        throw BridgeFailure(message: "Notification reply returned an inconsistent completion; it will not be resent")
                    }
                    // Complete means local PTY acceptance. It never steals focus.
                case .empty, .notWritten:
                    guard reply.bytesWritten == 0,
                          reply.outcome != .empty || reply.totalBytes == 0 else {
                        throw BridgeFailure(message: "Notification reply returned an inconsistent zero-byte receipt; it will not be resent")
                    }
                    if let navigation = receipt.navigation, navigation.opened,
                       navigation.target == request.target,
                       navigation.notificationId == request.target.notificationId,
                       request.target.matches(receipt) { activate() }
                    if reply.outcome == .notWritten { reportError("Notification reply was not written; no input was resent") }
                case .partial, .unknown:
                    reportError("Notification reply may be incomplete; it will not be resent")
                }
            } else if action == UNNotificationDismissActionIdentifier {
                _ = try await performAction(.dismiss(target: request.target))
            } else {
                let receipt = try await performAction(.open(target: request.target))
                // Rust's exact token/session validation precedes every explicit activation.
                if !stopped, epoch == request.target.epoch, request.target.matches(receipt) { activate() }
            }
            remove([identifier])
        } catch {
            if action == Self.replyActionIdentifier {
                remove([identifier])
                reportError("Notification reply was not confirmed and will not be resent: " + error.localizedDescription)
            } else { reportError("Notification target is unavailable: " + error.localizedDescription) }
        }
    }
}

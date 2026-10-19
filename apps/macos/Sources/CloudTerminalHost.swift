import AppKit
import Foundation

struct CloudTerminalSessionScope: Codable, Sendable, Equatable {
    let namespace: UUID
    let surfaceId: UInt64
    let sessionId: UInt64
}

struct CloudTerminalSessionDescriptor: Sendable, Identifiable {
    let device: CloudDevice
    let scope: CloudTerminalSessionScope
    let paneId: UInt64?
    let title: String

    var id: String {
        "\(device.id):\(scope.namespace.uuidString):\(scope.surfaceId):\(scope.sessionId)"
    }
}

private enum CloudTerminalOperation: String, Codable, Sendable {
    case list, frame, input, key, paste, resize, scroll
}

private struct CloudTerminalKeyEvent: Codable, Sendable {
    let key: String
    let action: String
    let modifiers: Int
    let legacyControl: Int?

    init(_ event: TerminalKeyEvent) {
        key = event.key
        action = event.action.rawValue
        modifiers = event.modifiers
        legacyControl = event.legacyControl
    }
}

private struct CloudTerminalOperationRequest: Codable, Sendable {
    let operation: CloudTerminalOperation
    let namespace: UUID?
    let surfaceId: UInt64?
    let sessionId: UInt64?
    let text: String?
    let event: CloudTerminalKeyEvent?
    let rows: Int?
    let cols: Int?
    let lines: Int?

    private init(operation: CloudTerminalOperation, scope: CloudTerminalSessionScope? = nil,
                 text: String? = nil, event: CloudTerminalKeyEvent? = nil,
                 rows: Int? = nil, cols: Int? = nil, lines: Int? = nil) {
        self.operation = operation
        namespace = scope?.namespace
        surfaceId = scope?.surfaceId
        sessionId = scope?.sessionId
        self.text = text
        self.event = event
        self.rows = rows
        self.cols = cols
        self.lines = lines
    }

    static func list() -> Self { Self(operation: .list) }
    static func frame(_ scope: CloudTerminalSessionScope) -> Self { Self(operation: .frame, scope: scope) }
    static func input(_ text: String, scope: CloudTerminalSessionScope) -> Self { Self(operation: .input, scope: scope, text: text) }
    static func key(_ event: TerminalKeyEvent, scope: CloudTerminalSessionScope) -> Self {
        Self(operation: .key, scope: scope, event: CloudTerminalKeyEvent(event))
    }
    static func paste(_ text: String, scope: CloudTerminalSessionScope) -> Self { Self(operation: .paste, scope: scope, text: text) }
    static func resize(rows: Int, cols: Int, scope: CloudTerminalSessionScope) -> Self {
        Self(operation: .resize, scope: scope, rows: rows, cols: cols)
    }
    static func scroll(_ lines: Int, scope: CloudTerminalSessionScope) -> Self { Self(operation: .scroll, scope: scope, lines: lines) }

    var isMutation: Bool { ![CloudTerminalOperation.list, .frame].contains(operation) }

    var isValid: Bool {
        let hasScope = namespace != nil && surfaceId != nil && sessionId != nil
        switch operation {
        case .list:
            return !hasScope && text == nil && event == nil && rows == nil && cols == nil && lines == nil
        case .frame:
            return hasScope && text == nil && event == nil && rows == nil && cols == nil && lines == nil
        case .input:
            return hasScope && event == nil && rows == nil && cols == nil && lines == nil
                && (text?.utf8.count ?? 0) <= 4_096
        case .paste:
            return hasScope && event == nil && rows == nil && cols == nil && lines == nil
                && (text?.utf8.count ?? 0) <= 16_384
        case .key:
            return hasScope && text == nil && event != nil && rows == nil && cols == nil && lines == nil
        case .resize:
            return hasScope && text == nil && event == nil && (1...120).contains(rows ?? 0)
                && (1...240).contains(cols ?? 0) && lines == nil
        case .scroll:
            return hasScope && text == nil && event == nil && rows == nil && cols == nil
                && (-1_000...1_000).contains(lines ?? 0)
        }
    }
}

private struct CloudTerminalSessionRow: Decodable, Sendable {
    let surfaceId: UInt64
    let sessionId: UInt64
    let paneId: UInt64?
    let title: String
}

private struct CloudTerminalOperationError: Codable, Sendable {
    let code: String
    let message: String
}

private struct CloudTerminalOperationReply: Decodable, Sendable {
    let ok: Bool
    let namespace: UUID?
    let sessions: [CloudTerminalSessionRow]?
    let frame: Frame?
    let accepted: Bool?
    let error: CloudTerminalOperationError?
    let outcomeUnknown: Bool?
}

private struct CloudTerminalFailureReply: Encodable {
    let ok = false
    let error: CloudTerminalOperationError
    let outcomeUnknown: Bool
}

private struct CloudTerminalCleanupContext: Sendable {
    let id: UUID
    let device: CloudDevice?
    let grant: UUID?
    let ownerUserID: String?
    var cloudPresenceMayBeEnabled: Bool
}

private enum CloudTerminalCleanupOutcome {
    case cleared
    case retired(String)
    case failed(String)
}

enum CloudTerminalSharingState: Equatable {
    case off
    case starting
    case sharing
    case stopping
    case failed(String)

    var isSharing: Bool { if case .sharing = self { true } else { false } }
    var message: String {
        switch self {
        case .off: "Terminal sharing is off."
        case .starting: "Enabling terminal sharing…"
        case .sharing: "Other registered devices on this account can view terminal output, resize existing terminals to fit their viewer, and send input."
        case .stopping: "Stopping terminal sharing…"
        case .failed(let message): message
        }
    }
}

/// A terminal relay reply can report a completed refusal or an unknown mutation outcome.
/// Callers must surface the latter and never replay the operation automatically.
struct CloudTerminalRPCFailure: Error, LocalizedError, Sendable {
    let message: String
    let outcomeUnknown: Bool
    var errorDescription: String? { message }
}

/// Application-lifetime Cloud terminal owner. Window controllers borrow it; closing Cloud
/// settings or a viewer never changes the local sharing grant or stops the host PTYs.
@MainActor
final class CloudTerminalHost {
    private let client: CloudClient
    private let bridge: NativeBridge
    private let reportError: @MainActor (String) -> Void
    private var state: CloudTerminalSharingState = .off
    private var localDevice: CloudDevice?
    private var grant: UUID?
    private var grantOwnerUserID: String?
    private var grantCloudPresenceMayBeEnabled = false
    private var pendingCleanup: CloudTerminalCleanupContext?
    private var generation = UUID()
    private var transitionIntent = UUID()
    private var sharingDesired = false
    private var transitionTask: Task<Void, Error>?
    private var transitionTaskID: UUID?
    private var inboxTask: Task<Void, Never>?
    private var presenceTask: Task<Void, Never>?
    private var observers: [UUID: @MainActor (CloudTerminalSharingState) -> Void] = [:]

    init(client: CloudClient, bridge: NativeBridge, reportError: @escaping @MainActor (String) -> Void) {
        self.client = client
        self.bridge = bridge
        self.reportError = reportError
    }

    var sharingState: CloudTerminalSharingState { state }

    func observeSharing(_ observer: @escaping @MainActor (CloudTerminalSharingState) -> Void) -> UUID {
        let id = UUID()
        observers[id] = observer
        observer(state)
        return id
    }

    func removeSharingObserver(_ id: UUID) { observers.removeValue(forKey: id) }

    func setSharingEnabled(_ enabled: Bool) async throws {
        if enabled {
            if state.isSharing { return }
            if case .starting = state, let transitionTask {
                do { try await transitionTask.value }
                catch is CancellationError where transitionTaskID != transitionIntent { return }
                return
            }
            if pendingCleanup != nil || isStopping {
                try await stopSharing()
                throw CloudFailure(message: "The previous sharing grant is stopped. Enable sharing again to create a new grant.")
            }
            try await startSharing()
        } else {
            try await stopSharing()
        }
    }

    private var isStopping: Bool { if case .stopping = state { true } else { false } }

    /// Stops local admission first, then clears server presence. A cloud error cannot leave
    /// the Rust owner accepting work under an old grant.
    func stopSharing() async throws {
        if case .off = state, grant == nil, localDevice == nil,
           pendingCleanup == nil, transitionTask == nil {
            return
        }
        if case .stopping = state, !sharingDesired,
           transitionTaskID == transitionIntent, let transitionTask {
            try await transitionTask.value
            return
        }
        sharingDesired = false
        let intent = UUID()
        transitionIntent = intent
        setState(.stopping)
        generation = UUID()
        inboxTask?.cancel(); inboxTask = nil
        presenceTask?.cancel(); presenceTask = nil
        if pendingCleanup == nil {
            pendingCleanup = CloudTerminalCleanupContext(id: UUID(), device: localDevice, grant: grant,
                ownerUserID: grantOwnerUserID,
                cloudPresenceMayBeEnabled: grantCloudPresenceMayBeEnabled)
        }
        let previous = transitionTask
        let task = Task { @MainActor [weak self] in
            // A start may be suspended between identity lookup, local grant admission and
            // cloud presence. Join it before the unconditional local revoke below.
            if let previous { _ = try? await previous.value }
            guard let self else { return }
            let cleanup = self.pendingCleanup ?? CloudTerminalCleanupContext(
                id: UUID(), device: self.localDevice, grant: self.grant,
                ownerUserID: self.grantOwnerUserID,
                cloudPresenceMayBeEnabled: self.grantCloudPresenceMayBeEnabled)
            self.pendingCleanup = cleanup
            switch await self.finishSharingCleanup(cleanup) {
            case .cleared:
                if self.transitionIntent == intent { self.setState(.off) }
            case .retired(let warning):
                if self.transitionIntent == intent { self.setState(.off) }
                throw CloudFailure(message: warning)
            case .failed(let failure):
                if self.transitionIntent == intent { self.setState(.failed(failure)) }
                throw CloudFailure(message: failure)
            }
        }
        transitionTask = task
        transitionTaskID = intent
        do { try await task.value }
        catch {
            if transitionTaskID == intent { transitionTask = nil; transitionTaskID = nil }
            throw error
        }
        if transitionTaskID == intent { transitionTask = nil; transitionTaskID = nil }
    }

    /// Used by sign-out and local-device revocation. Local sharing always stops even when
    /// the authenticated presence call can no longer reach Cloud.
    func stopForCredentialChange() async -> String? {
        do { try await stopSharing(); return nil }
        catch { return error.localizedDescription }
    }

    /// A restore changes the desktop namespace and invalidates every operation scope. Invalidate
    /// startup synchronously, then queue the ordinary gate-disable and presence-clear path.
    func desktopNamespaceDidChange() {
        guard state != .off || grant != nil || localDevice != nil || pendingCleanup != nil || transitionTask != nil else { return }
        sharingDesired = false
        transitionIntent = UUID()
        generation = UUID()
        inboxTask?.cancel(); inboxTask = nil
        presenceTask?.cancel(); presenceTask = nil
        setState(.stopping)
        Task { @MainActor [weak self] in
            guard let self else { return }
            if let failure = await self.stopForCredentialChange() {
                self.reportError("Terminal sharing could not stop after the desktop session set changed: \(failure)")
            }
        }
    }

    func listSessions(on device: CloudDevice) async throws -> [CloudTerminalSessionDescriptor] {
        try requireConnectable(device)
        let reply = try await send(CloudTerminalOperationRequest.list(), to: device)
        guard let namespace = reply.namespace, let sessions = reply.sessions else {
            throw CloudFailure(message: "The device returned an invalid terminal list.")
        }
        return sessions.map { session in
            CloudTerminalSessionDescriptor(device: device,
                scope: CloudTerminalSessionScope(namespace: namespace, surfaceId: session.surfaceId, sessionId: session.sessionId),
                paneId: session.paneId, title: session.title)
        }
    }

    func readFrame(for session: CloudTerminalSessionDescriptor) async throws -> Frame {
        let reply = try await send(.frame(session.scope), to: session.device)
        guard let frame = reply.frame, frame.sessionId == session.scope.sessionId else {
            throw CloudFailure(message: "The device returned a frame for a different terminal session.")
        }
        return frame
    }

    func sendInput(_ text: String, to session: CloudTerminalSessionDescriptor) async throws {
        _ = try await send(.input(text, scope: session.scope), to: session.device)
    }

    func sendKey(_ event: TerminalKeyEvent, to session: CloudTerminalSessionDescriptor) async throws {
        _ = try await send(.key(event, scope: session.scope), to: session.device)
    }

    func sendPaste(_ text: String, to session: CloudTerminalSessionDescriptor) async throws {
        _ = try await send(.paste(text, scope: session.scope), to: session.device)
    }

    func sendResize(rows: Int, cols: Int, to session: CloudTerminalSessionDescriptor) async throws {
        _ = try await send(.resize(rows: rows, cols: cols, scope: session.scope), to: session.device)
    }

    func sendScroll(_ lines: Int, to session: CloudTerminalSessionDescriptor) async throws {
        _ = try await send(.scroll(lines, scope: session.scope), to: session.device)
    }

    private func send(_ request: CloudTerminalOperationRequest, to device: CloudDevice) async throws -> CloudTerminalOperationReply {
        try requireConnectable(device)
        guard request.isValid else { throw CloudFailure(message: "Invalid Cloud terminal operation.") }
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        let payload = try encoder.encode(request)
        let response: Data
        do { response = try await client.sendTerminalRequest(to: device, payload: payload) }
        catch {
            if let failure = error as? CloudFailure, failure.requiresSignIn || failure.requiresDeviceReauth {
                _ = await stopForCredentialChange()
            }
            throw error
        }
        guard response.count <= 262_144 else {
            let message = "The Cloud terminal response exceeded its size limit."
            if request.isMutation { throw CloudTerminalRPCFailure(message: message + " The terminal change may have been applied and was not retried.", outcomeUnknown: true) }
            throw CloudFailure(message: message)
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let reply: CloudTerminalOperationReply
        do { reply = try decoder.decode(CloudTerminalOperationReply.self, from: response) }
        catch {
            if request.isMutation {
                throw CloudTerminalRPCFailure(message: "The terminal response could not be verified. The change may have been applied and was not retried.", outcomeUnknown: true)
            }
            throw CloudFailure(message: "The device returned an invalid terminal response.")
        }
        guard reply.ok else {
            let base = reply.error?.message ?? "The Cloud terminal operation was rejected."
            let message = reply.outcomeUnknown == true
                ? base + " The terminal change may have been applied; it was not retried. Reconnect in read-only mode and inspect the terminal before sending more input."
                : base
            throw CloudTerminalRPCFailure(message: message, outcomeUnknown: reply.outcomeUnknown == true)
        }
        if request.isMutation, reply.accepted != true {
            let unknown = reply.outcomeUnknown ?? true
            let message = unknown
                ? "The terminal change may have been applied; it was not retried. Reconnect in read-only mode and inspect the terminal before sending more input."
                : "The device did not confirm the terminal operation."
            throw CloudTerminalRPCFailure(message: message, outcomeUnknown: unknown)
        }
        return reply
    }

    private func startSharing() async throws {
        guard pendingCleanup == nil, !isStopping else {
            throw CloudFailure(message: "The previous terminal sharing cleanup is still in progress.")
        }
        sharingDesired = true
        let intent = UUID()
        transitionIntent = intent
        let previous = transitionTask
        let task = Task { @MainActor [weak self] in
            if let previous {
                do { try await previous.value }
                catch {
                    guard let self else { throw error }
                    let message = "Terminal sharing could not confirm its previous stop: \(error.localizedDescription)"
                    if self.transitionIntent == intent { self.setState(.failed(message)) }
                    throw error
                }
            }
            guard let self, self.transitionIntent == intent, self.sharingDesired else { return }
            let loopGeneration = UUID()
            self.generation = loopGeneration
            self.setState(.starting)
            try await self.performStartSharing(intent: intent, loopGeneration: loopGeneration)
        }
        transitionTask = task
        transitionTaskID = intent
        do { try await task.value }
        catch {
            if transitionTaskID == intent { transitionTask = nil; transitionTaskID = nil }
            throw error
        }
        if transitionTaskID == intent { transitionTask = nil; transitionTaskID = nil }
    }

    private func performStartSharing(intent: UUID, loopGeneration: UUID) async throws {
        func checkIntent() throws {
            guard transitionIntent == intent, sharingDesired else { throw CancellationError() }
        }
        var cleanup: CloudTerminalCleanupContext?
        do {
            let identity = try await client.localDeviceIdentity()
            let account = try await client.account()
            try checkIntent()
            guard !account.user.id.isEmpty else { throw CloudFailure(message: "Cloud returned an invalid account identity.") }
            let device = account.devices.first { $0.installationID == identity.installationID }
            guard let device else { throw CloudFailure(message: "Register this Mac before sharing terminal sessions.") }
            let ownerUserID = account.user.id
            let pending = CloudTerminalCleanupContext(id: UUID(), device: device, grant: nil,
                ownerUserID: ownerUserID, cloudPresenceMayBeEnabled: false)
            cleanup = pending
            pendingCleanup = pending
            try checkIntent()
            guard let createdGrant = try await bridge.setTerminalSharing(enabled: true) else {
                throw BridgeFailure(message: "The terminal owner did not return a sharing grant.")
            }
            let enabled = CloudTerminalCleanupContext(id: pending.id, device: device, grant: createdGrant,
                ownerUserID: ownerUserID, cloudPresenceMayBeEnabled: false)
            cleanup = enabled
            pendingCleanup = enabled
            try checkIntent()
            do {
                try await client.setTerminalPresence(device: device, sharing: true, epoch: createdGrant)
                var presenceEnabled = enabled
                presenceEnabled.cloudPresenceMayBeEnabled = true
                cleanup = presenceEnabled
                pendingCleanup = presenceEnabled
            } catch let failure as CloudFailure where failure.outcomeUnknown {
                var uncertainPresence = enabled
                uncertainPresence.cloudPresenceMayBeEnabled = true
                cleanup = uncertainPresence
                pendingCleanup = uncertainPresence
                throw failure
            }
            try checkIntent()
            localDevice = device
            grant = createdGrant
            grantOwnerUserID = ownerUserID
            grantCloudPresenceMayBeEnabled = true
            if pendingCleanup?.id == enabled.id { pendingCleanup = nil }
            setState(.sharing)
            startInbox(device: device, grant: createdGrant, generation: loopGeneration)
            startPresence(device: device, grant: createdGrant, generation: loopGeneration)
        } catch {
            var cleanupFailure: String?
            var cleanupWarning: String?
            if let cleanup {
                // A queued stop waits for this exact start before it disables the Rust gate,
                // so this cleanup cannot revoke a later user's re-enabled grant.
                switch await finishSharingCleanup(cleanup) {
                case .cleared:
                    break
                case .retired(let warning):
                    cleanupWarning = warning
                case .failed(let failure):
                    cleanupFailure = failure
                }
            }
            if transitionIntent == intent {
                if let cleanupFailure { setState(.failed(cleanupFailure)) }
                else { setState(.off) }
            }
            if let cleanupFailure { throw CloudFailure(message: cleanupFailure) }
            if let cleanupWarning { throw CloudFailure(message: cleanupWarning) }
            throw error
        }
    }

    /// Keeps the device, owner and epoch until local revocation and same-account presence
    /// clearing are confirmed. A different authenticated account can retire the old context
    /// only after the Rust gate is confirmed closed.
    private func finishSharingCleanup(_ cleanup: CloudTerminalCleanupContext) async -> CloudTerminalCleanupOutcome {
        var localFailure: String?
        do { _ = try await bridge.setTerminalSharing(enabled: false) }
        catch { localFailure = "The local sharing gate could not be confirmed closed: \(error.localizedDescription)" }
        if let grant = cleanup.grant {
            if !cleanup.cloudPresenceMayBeEnabled {
                if let localFailure { return .failed(localFailure) }
                guard clearSharingCleanupContext(cleanup) else {
                    return .failed("The sharing cleanup context changed before cleanup was confirmed; it was retained for retry.")
                }
                return .cleared
            }
            guard let ownerUserID = cleanup.ownerUserID, !ownerUserID.isEmpty else {
                let detail = "The account owning the previous sharing grant could not be verified; cleanup context was retained."
                return .failed([localFailure, detail].compactMap { $0 }.joined(separator: " "))
            }
            guard let device = cleanup.device else {
                let detail = "Cloud sharing presence could not be cleared because its device identity is unavailable."
                return .failed([localFailure, detail].compactMap { $0 }.joined(separator: " "))
            }
            let account: CloudAccount
            do { account = try await client.account() }
            catch {
                let detail = "The current Cloud account could not be verified, so the previous sharing cleanup context was retained: \(error.localizedDescription)"
                return .failed([localFailure, detail].compactMap { $0 }.joined(separator: " "))
            }
            guard !account.user.id.isEmpty else {
                let detail = "The current Cloud account identity was empty, so the previous sharing cleanup context was retained."
                return .failed([localFailure, detail].compactMap { $0 }.joined(separator: " "))
            }
            if account.user.id != ownerUserID {
                guard localFailure == nil else {
                    return .failed("This Mac is signed in to a different Cloud account, and \(localFailure!). The previous sharing cleanup context was retained.")
                }
                guard clearSharingCleanupContext(cleanup) else {
                    return .failed("The sharing cleanup context changed while account ownership was checked; it was retained for retry.")
                }
                return .retired("The local terminal gate is closed, but the previous account's Cloud sharing presence was not cleared because this Mac is signed in to a different account. The old presence expires when its 15-second lease ends.")
            }
            var failures: [String] = []
            if let localFailure { failures.append(localFailure) }
            do { try await client.setTerminalPresence(device: device, sharing: false, epoch: grant) }
            catch { failures.append("Cloud could not confirm the sharing presence was cleared: \(error.localizedDescription)") }
            if !failures.isEmpty { return .failed(failures.joined(separator: " ")) }
        } else if let localFailure {
            return .failed(localFailure)
        }
        guard clearSharingCleanupContext(cleanup) else {
            return .failed("The sharing cleanup context changed before cleanup was confirmed; it was retained for retry.")
        }
        return .cleared
    }

    private func clearSharingCleanupContext(_ cleanup: CloudTerminalCleanupContext) -> Bool {
        guard pendingCleanup?.id == cleanup.id else { return false }
        pendingCleanup = nil
        if grant == cleanup.grant {
            grant = nil
            localDevice = nil
            grantOwnerUserID = nil
            grantCloudPresenceMayBeEnabled = false
        }
        return true
    }

    private func startInbox(device: CloudDevice, grant: UUID, generation expected: UUID) {
        inboxTask?.cancel()
        inboxTask = Task { [weak self, client] in
            var activeUntil = Date.distantPast
            var failureDelay = 500
            while !Task.isCancelled {
                guard let self, self.generation == expected, self.grant == grant else { return }
                do {
                    if let claim = try await client.claimTerminalRequest(device: device, epoch: grant) {
                        activeUntil = Date().addingTimeInterval(1)
                        try await self.process(claim, grant: grant)
                        failureDelay = 500
                    } else {
                        let delay = Date() < activeUntil ? 40 : 500
                        try await Task.sleep(for: .milliseconds(delay))
                    }
                } catch is CancellationError {
                    return
                } catch {
                    self.reportError("Cloud terminal sharing: \(error.localizedDescription)")
                    if let failure = error as? CloudFailure, failure.requiresSignIn || failure.requiresDeviceReauth {
                        _ = await self.stopForCredentialChange()
                        return
                    }
                    do { try await Task.sleep(for: .milliseconds(failureDelay)) } catch { return }
                    failureDelay = min(failureDelay * 2, 5_000)
                }
            }
        }
    }

    private func startPresence(device: CloudDevice, grant: UUID, generation expected: UUID) {
        presenceTask?.cancel()
        presenceTask = Task { [weak self, client] in
            while !Task.isCancelled {
                do { try await Task.sleep(for: .seconds(5)) }
                catch { return }
                guard let self, self.generation == expected, self.grant == grant else { return }
                do { try await client.setTerminalPresence(device: device, sharing: true, epoch: grant) }
                catch {
                    self.reportError("Cloud terminal presence update failed: \(error.localizedDescription)")
                    if let failure = error as? CloudFailure, failure.requiresSignIn || failure.requiresDeviceReauth {
                        _ = await self.stopForCredentialChange()
                        return
                    }
                }
            }
        }
    }

    private func process(_ claim: CloudTerminalClaim, grant: UUID) async throws {
        let request = try? decodeRequest(claim.payload)
        try await client.beginTerminalRequest(claim)
        let payload: Data
        if let request, request.isValid {
            do { payload = try await bridge.sharedTerminalOperation(request: claim.payload, grant: grant) }
            catch { payload = failureReply(error, outcomeUnknown: request.isMutation) }
        } else {
            payload = failureReply(CloudFailure(message: "Invalid terminal operation."), outcomeUnknown: false)
        }
        let invalidatedGrant = invalidatedGrantCode(payload)
        do { try await client.respondTerminalRequest(claim, payload: payload) }
        catch {
            if let invalidatedGrant { await stopAfterOwnerInvalidation(invalidatedGrant) }
            throw error
        }
        if let invalidatedGrant { await stopAfterOwnerInvalidation(invalidatedGrant) }
    }

    private func invalidatedGrantCode(_ data: Data) -> String? {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        guard let reply = try? decoder.decode(CloudTerminalOperationReply.self, from: data),
              let code = reply.error?.code,
              ["sharing_disabled", "invalid_grant", "stale_namespace"].contains(code) else { return nil }
        return code
    }

    private func stopAfterOwnerInvalidation(_ code: String) async {
        reportError("The native terminal owner rejected its sharing grant (\(code)). Terminal sharing is stopping; enable it again to share current sessions.")
        if let failure = await stopForCredentialChange() {
            reportError("Terminal sharing could not finish stopping after native grant invalidation: \(failure)")
        }
    }

    private func decodeRequest(_ data: Data) throws -> CloudTerminalOperationRequest {
        guard data.count <= 128_000 else { throw CloudFailure(message: "Terminal operation exceeded its size limit.") }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(CloudTerminalOperationRequest.self, from: data)
    }

    private func failureReply(_ error: any Error, outcomeUnknown: Bool) -> Data {
        let body = CloudTerminalFailureReply(error: CloudTerminalOperationError(code: "native_error",
            message: String(error.localizedDescription.prefix(512))), outcomeUnknown: outcomeUnknown)
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        return (try? encoder.encode(body)) ?? Data("{\"ok\":false,\"outcome_unknown\":true}".utf8)
    }

    private func requireConnectable(_ device: CloudDevice) throws {
        guard device.revokedAt == nil, device.online, device.sharing else {
            throw CloudFailure(message: "This device is offline or is not sharing terminal sessions.")
        }
    }

    private func setState(_ state: CloudTerminalSharingState) {
        self.state = state
        for observer in observers.values { observer(state) }
    }
}

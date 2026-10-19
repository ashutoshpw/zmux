import Foundation
import Security
import CryptoKit

struct CloudFailure: Error, LocalizedError, Sendable {
    let message: String
    let requiresSignIn: Bool
    let requiresDeviceReauth: Bool
    let requiresServerUpgrade: Bool
    let outcomeUnknown: Bool
    let httpStatusCode: Int?
    init(message: String, requiresSignIn: Bool = false, requiresDeviceReauth: Bool = false,
         requiresServerUpgrade: Bool = false, outcomeUnknown: Bool = false, httpStatusCode: Int? = nil) {
        self.message = message
        self.requiresSignIn = requiresSignIn
        self.requiresDeviceReauth = requiresDeviceReauth
        self.requiresServerUpgrade = requiresServerUpgrade
        self.outcomeUnknown = outcomeUnknown
        self.httpStatusCode = httpStatusCode
    }
    var errorDescription: String? { message }
}

struct CloudUser: Decodable, Sendable { let id: String; let name: String; let email: String }
struct CloudIdentity: Decodable, Sendable { let user: CloudUser }
struct CloudDevice: Decodable, Sendable {
    let id: String
    let name: String
    let platform: String
    let installationID: UUID?
    let publicKey: Data?
    let online: Bool
    let sharing: Bool
    let sharingEpoch: UUID?
    let lastSeenAt: String?
    let revokedAt: String?

    private enum CodingKeys: String, CodingKey {
        case id, name, platform, installationId, publicKey, online, sharing, sharingEpoch, lastSeenAt, revokedAt
    }

    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        id = try values.decode(String.self, forKey: .id)
        name = try values.decode(String.self, forKey: .name)
        platform = try values.decode(String.self, forKey: .platform)
        installationID = try values.decodeIfPresent(UUID.self, forKey: .installationId)
        let encodedPublicKey = try values.decodeIfPresent(String.self, forKey: .publicKey)
        publicKey = encodedPublicKey.flatMap { Data.cloudBase64URLDecoded($0, maximumBytes: 32) }
        online = try values.decodeIfPresent(Bool.self, forKey: .online) ?? false
        sharing = try values.decodeIfPresent(Bool.self, forKey: .sharing) ?? false
        sharingEpoch = try values.decodeIfPresent(UUID.self, forKey: .sharingEpoch)
        lastSeenAt = try values.decodeIfPresent(String.self, forKey: .lastSeenAt)
        revokedAt = try values.decodeIfPresent(String.self, forKey: .revokedAt)
    }
}
struct CloudDevices: Decodable, Sendable { let devices: [CloudDevice] }
struct CloudAccount: Sendable { let user: CloudUser; let devices: [CloudDevice] }
private struct CloudToken: Decodable { let accessToken: String; let expiresIn: Int; let tokenType: String }
private struct StoredCloudToken: Codable, Sendable { let accessToken: String; let expiresAt: Date }
private struct CloudRegistrationBody: Encodable {
    let name: String
    let platform: String
    let installationID: String
    let deviceSecret: String
    let publicKey: String
}
private struct CloudPresenceBody: Encodable { let sharing: Bool; let epoch: String }
private struct CloudQueuedRequestBody: Encodable { let id: String; let targetDeviceID: String; let payload: String }
private struct CloudPayloadBody: Encodable { let payload: String }
private struct CloudCapabilities: Decodable {
    let stableDeviceIdentity: Bool?
    let terminalRelay: Bool?
}
private struct CloudConfiguration: Decodable { let capabilities: CloudCapabilities }
private struct CloudQueuedRequest: Decodable { let id: UUID; let state: String; let expiresAt: String }
private struct CloudBeginResponse: Decodable { let id: UUID; let state: String }
private struct CloudInbox: Decodable { let requests: [CloudInboxRequest] }
private struct CloudInboxRequest: Decodable {
    let id: UUID
    let senderDeviceId: UUID
    let senderPublicKey: String
    let payload: String
    let expiresAt: String
    let targetEpoch: UUID
}
private struct CloudRequestStatus: Decodable {
    let id: UUID
    let state: String
    let expiresAt: String
    let payload: String?
}

private struct CloudCapabilityCache {
    let generation: UUID
    let checkedAt: Date
    let stableDeviceIdentity: Bool
    let terminalRelay: Bool
}

private struct CloudDeviceDirectoryCache {
    let generation: UUID
    let accessToken: String
    let tokenExpiresAt: Date
    let checkedAt: Date
    let devices: [CloudDevice]
}

fileprivate struct CloudClaimContext {
    let scope: CloudClient.RequestScope
    let claim: CloudTerminalClaim
    let targetDeviceID: UUID
    let cryptoContext: CloudTerminalCryptoContext
    let issuedAt: Date
    let expiresAt: Date
    var executing: Bool
}

private enum CloudDate {
    static func parse(_ value: String) -> Date? {
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let date = fractional.date(from: value) { return date }
        let standard = ISO8601DateFormatter()
        standard.formatOptions = [.withInternetDateTime]
        return standard.date(from: value)
    }
}

struct CloudEndpoint: Sendable {
    let origin: URL
    static var configured: String {
        Bundle.main.object(forInfoDictionaryKey: "ZmuxCloudOrigin") as? String ?? "https://zmux.w3api.dev"
    }
    init(_ value: String) throws {
        guard let parts = URLComponents(string: value.trimmingCharacters(in: .whitespacesAndNewlines)),
              let host = parts.host, parts.user == nil, parts.password == nil,
              parts.query == nil, parts.fragment == nil, parts.path.isEmpty || parts.path == "/",
              parts.scheme == "https" || (parts.scheme == "http" && ["localhost", "127.0.0.1", "::1"].contains(host)),
              let url = parts.url else {
            throw CloudFailure(message: "Use an HTTPS cloud origin. HTTP is permitted only for a local development server.")
        }
        origin = url
    }
    func url(_ path: String) -> URL { origin.appendingPathComponent(path) }
}

struct CloudAuthorization: Sendable {
    static let clientID = "zmux-macos-dev"
    static let callback = "zmux-dev://oauth/callback"
    let state: String
    let verifier: String
    let url: URL
    init(endpoint: CloudEndpoint) throws {
        func random() throws -> String {
            var bytes = [UInt8](repeating: 0, count: 32)
            guard SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes) == errSecSuccess else {
                throw CloudFailure(message: "Unable to generate secure sign-in credentials.")
            }
            return Data(bytes).base64URLEncoded()
        }
        state = try random()
        verifier = try random()
        let challenge = Data(SHA256.hash(data: Data(verifier.utf8))).base64URLEncoded()
        var parts = URLComponents(url: endpoint.url("v1/native/authorize"), resolvingAgainstBaseURL: false)!
        parts.queryItems = [URLQueryItem(name: "client_id", value: Self.clientID),
                            URLQueryItem(name: "redirect_uri", value: Self.callback),
                            URLQueryItem(name: "response_type", value: "code"),
                            URLQueryItem(name: "code_challenge_method", value: "S256"),
                            URLQueryItem(name: "code_challenge", value: challenge),
                            URLQueryItem(name: "state", value: state)]
        guard let url = parts.url else { throw CloudFailure(message: "Unable to construct sign-in URL.") }
        self.url = url
    }
    func code(from url: URL) throws -> String {
        guard let parts = URLComponents(url: url, resolvingAgainstBaseURL: false),
              parts.scheme == "zmux-dev", parts.host == "oauth", parts.path == "/callback",
              parts.user == nil, parts.password == nil, parts.port == nil, parts.fragment == nil else {
            throw CloudFailure(message: "The sign-in callback did not match zmux.")
        }
        let query = parts.queryItems ?? []
        func single(_ name: String) -> String? {
            let items = query.filter { $0.name == name }
            return items.count == 1 ? items[0].value : nil
        }
        guard single("state") == state else { throw CloudFailure(message: "The sign-in state did not match. Start a new sign-in.") }
        guard !query.contains(where: { $0.name == "error" }), let code = single("code"), !code.isEmpty, code.utf8.count <= 1024,
              !query.contains(where: { $0.name == "access_token" }) else {
            throw CloudFailure(message: "Cloud sign-in was not completed.")
        }
        return code
    }
}

private extension Data {
    func base64URLEncoded() -> String {
        base64EncodedString().replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }
}

private final class CloudRedirectPolicy: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}

actor CloudClient {
    let endpoint: CloudEndpoint
    private let session: URLSession
    private let keychainService = "app.w3dev.zmux.dev.cloud"
    private var token: StoredCloudToken?
    private var generation = UUID()
    private var deviceIdentityCache: CloudDeviceIdentity?
    private var capabilityCache: CloudCapabilityCache?
    private var deviceDirectoryCache: CloudDeviceDirectoryCache?
    private var hostClaims: [UUID: CloudClaimContext] = [:]
    fileprivate struct RequestScope: Sendable {
        fileprivate let generation: UUID
        fileprivate let credential: StoredCloudToken?
    }

    init(endpoint: CloudEndpoint) {
        self.endpoint = endpoint
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 15
        config.timeoutIntervalForResource = 30
        config.httpCookieStorage = nil
        config.urlCache = nil
        session = URLSession(configuration: config, delegate: CloudRedirectPolicy(), delegateQueue: nil)
    }
    func account() async throws -> CloudAccount {
        try Task.checkCancellation()
        return try await account(scope: currentScope())
    }
    private func account(scope: RequestScope) async throws -> CloudAccount {
        let identity: CloudIdentity = try await request("v1/me", scope: scope)
        let list: CloudDevices = try await request("v1/devices", scope: scope)
        return CloudAccount(user: identity.user, devices: list.devices)
    }
    func localDeviceIdentity() throws -> CloudDeviceIdentityPublic {
        try deviceIdentity().publicIdentity
    }
    func localDevice() async throws -> CloudDevice? {
        let identity = try deviceIdentity().publicIdentity
        let scope = try currentScope()
        let account = try await account(scope: scope)
        try check(scope.generation)
        return account.devices.first { $0.installationID == identity.installationID }
    }
    func complete(code: String, verifier: String) async throws -> CloudAccount {
        try Task.checkCancellation()
        generation = UUID()
        hostClaims.removeAll()
        capabilityCache = nil
        deviceDirectoryCache = nil
        let current = generation
        let response: CloudToken = try await request("v1/native/token", method: "POST", body: [
            "grant_type": "authorization_code", "client_id": CloudAuthorization.clientID,
            "redirect_uri": CloudAuthorization.callback, "code": code, "code_verifier": verifier
        ], scope: RequestScope(generation: current, credential: nil))
        guard response.tokenType.lowercased() == "bearer", !response.accessToken.isEmpty,
              response.accessToken.utf8.count <= 4096, response.expiresIn > 0 else {
            throw CloudFailure(message: "Cloud returned an invalid sign-in response.")
        }
        let stored = StoredCloudToken(accessToken: response.accessToken, expiresAt: Date().addingTimeInterval(Double(response.expiresIn)))
        // Keep the candidate off Keychain until all awaited account validation
        // succeeds. A cancelled or superseded login cannot commit credentials.
        let account = try await account(scope: RequestScope(generation: current, credential: stored))
        try check(current)
        try save(stored)
        token = stored
        generation = UUID()
        hostClaims.removeAll()
        capabilityCache = nil
        deviceDirectoryCache = nil
        return account
    }
    func registerDevice(name: String) async throws -> CloudDevice {
        let scope = try currentScope()
        // Old servers ignore unknown identity fields and create another row on
        // every POST. Never send the mutation until the server advertises support.
        try await requireCapability("stable_device_identity", scope: scope, refresh: true)
        let identity = try deviceIdentity()
        let body = CloudRegistrationBody(name: name, platform: "macos",
                                         installationID: identity.publicIdentity.installationID.uuidString.lowercased(),
                                         deviceSecret: identity.deviceSecretHeader,
                                         publicKey: identity.publicIdentity.publicKey.cloudBase64URLEncoded())
        let bytes = try await rawRequestData("v1/devices", method: "POST", body: try encode(body), scope: scope)
        let device: CloudDevice = try decode(CloudDevice.self, from: bytes)
        try check(scope.generation)
        deviceDirectoryCache = nil
        guard device.installationID == identity.publicIdentity.installationID,
              device.publicKey == identity.publicIdentity.publicKey else {
            throw CloudFailure(message: "Cloud returned a device registration for a different installation.")
        }
        return device
    }
    func revokeDevice(id: String) async throws {
        guard !id.isEmpty, id.utf8.count <= 128, id.allSatisfy({ $0.isLetter || $0.isNumber || $0 == "-" || $0 == "_" }) else {
            throw CloudFailure(message: "Invalid device identifier.")
        }
        let scope = try currentScope()
        _ = try await rawRequest("v1/devices/\(id)", method: "DELETE", scope: scope)
        try check(scope.generation)
        deviceDirectoryCache = nil
    }
    func setTerminalPresence(device: CloudDevice, sharing: Bool, epoch: UUID) async throws {
        let scope: RequestScope
        let identity: CloudDeviceIdentity
        let localID: UUID
        let headers: [String: String]
        let body: Data
        do {
            scope = try currentScope()
            try await requireCapability("terminal_relay", scope: scope)
            identity = try deviceIdentity()
            let devices = try await deviceDirectory(scope: scope)
            try check(scope.generation)
            let local = try localCloudDevice(in: devices, identity: identity)
            guard local.id == device.id, local.revokedAt == nil else {
                throw CloudFailure(message: "This Mac's registered cloud device is no longer available.", requiresDeviceReauth: true)
            }
            guard let deviceID = UUID(uuidString: local.id) else {
                throw CloudFailure(message: "Cloud returned an invalid device identifier.")
            }
            if sharing, local.sharing, let currentEpoch = local.sharingEpoch, currentEpoch != epoch {
                throw CloudFailure(message: "Cloud already has a different sharing grant for this Mac.", requiresDeviceReauth: true)
            }
            localID = deviceID
            headers = try deviceHeaders(deviceID: deviceID, identity: identity)
            body = try encode(CloudPresenceBody(sharing: sharing, epoch: epoch.uuidString.lowercased()))
        } catch let failure as CloudFailure {
            throw CloudFailure(message: failure.message, requiresSignIn: failure.requiresSignIn,
                               requiresDeviceReauth: failure.requiresDeviceReauth,
                               requiresServerUpgrade: failure.requiresServerUpgrade,
                               outcomeUnknown: false, httpStatusCode: failure.httpStatusCode)
        } catch is CancellationError {
            throw CloudFailure(message: "The terminal sharing change was cancelled before it was sent.", outcomeUnknown: false)
        } catch {
            throw CloudFailure(message: "The terminal sharing change could not be prepared: \(error.localizedDescription)",
                               outcomeUnknown: false)
        }

        let path = "v1/devices/\(localID.uuidString.lowercased())/presence"
        do {
            _ = try await rawRequestData(path, method: "POST", body: body,
                                         headers: headers,
                                         deviceAuthenticated: true, missingMeansDeviceReauth: true, scope: scope)
            try check(scope.generation)
            deviceDirectoryCache = nil
        } catch let failure as CloudFailure {
            if failure.outcomeUnknown || failure.requiresSignIn || failure.requiresDeviceReauth ||
                (failure.httpStatusCode.map { (400..<500).contains($0) } ?? false) {
                throw failure
            }
            throw unknownOutcome(failure)
        } catch {
            throw CloudFailure(message: "Cloud could not confirm the terminal sharing change: \(error.localizedDescription)",
                               outcomeUnknown: true)
        }
    }

    func claimTerminalRequest(device: CloudDevice, epoch: UUID) async throws -> CloudTerminalClaim? {
        let scope = try currentScope()
        try await requireCapability("terminal_relay", scope: scope)
        let identity = try deviceIdentity()
        var devices = try await deviceDirectory(scope: scope)
        try check(scope.generation)
        let local = try localCloudDevice(in: devices, identity: identity)
        guard local.id == device.id, local.revokedAt == nil,
              local.sharing, local.sharingEpoch == epoch,
              let localID = UUID(uuidString: local.id) else {
            throw CloudFailure(message: "This Mac no longer has the active terminal sharing grant.", requiresDeviceReauth: true)
        }
        let path = "v1/terminal-relay/inbox"
        let bytes = try await rawRequestData(path, headers: try deviceHeaders(deviceID: localID, identity: identity),
                                            deviceAuthenticated: true, missingMeansDeviceReauth: true, scope: scope)
        let inbox: CloudInbox = try decode(CloudInbox.self, from: bytes)
        guard inbox.requests.count <= 1 else { throw CloudFailure(message: "Cloud returned more than one terminal request.") }
        guard let request = inbox.requests.first else { return nil }
        var sender = devices.first(where: { UUID(uuidString: $0.id) == request.senderDeviceId })
        if sender == nil {
            devices = try await deviceDirectory(scope: scope, refresh: true)
            try check(scope.generation)
            sender = devices.first(where: { UUID(uuidString: $0.id) == request.senderDeviceId })
        }
        guard request.targetEpoch == epoch, request.senderDeviceId != localID,
              let serverExpiry = CloudDate.parse(request.expiresAt), serverExpiry > Date(),
              let sender,
              sender.revokedAt == nil, let directoryKey = sender.publicKey, directoryKey.count == 32,
              let senderKey = Data.cloudBase64URLDecoded(request.senderPublicKey, maximumBytes: 32),
              senderKey.count == 32, senderKey == directoryKey else {
            throw CloudFailure(message: "Cloud returned a terminal request for an unavailable device or sharing grant.")
        }
        let now = Date()
        hostClaims = hostClaims.filter { $0.value.expiresAt > now }
        guard hostClaims.count < 16 else { throw CloudFailure(message: "This Mac already has the maximum number of active terminal requests.") }
        let cryptoContext = CloudTerminalCryptoContext(requestID: request.id, senderDeviceID: request.senderDeviceId,
                                                       targetDeviceID: localID, targetEpoch: epoch)
        let opened = try CloudTerminalTransport.open(request.payload, using: identity, peerPublicKey: senderKey,
                                                      expected: cryptoContext, direction: .request,
                                                      serverExpiresAt: serverExpiry)
        let expiry = min(serverExpiry, opened.expiresAt)
        guard expiry > Date() else { throw CloudFailure(message: "The terminal request expired before it could be handled.", outcomeUnknown: true) }
        let claim = CloudTerminalClaim(id: request.id, senderDeviceID: request.senderDeviceId,
                                      senderPublicKey: senderKey, expiresAt: expiry,
                                      targetEpoch: epoch, payload: opened.payload)
        hostClaims[request.id] = CloudClaimContext(scope: scope, claim: claim, targetDeviceID: localID,
                                                   cryptoContext: cryptoContext, issuedAt: opened.issuedAt,
                                                   expiresAt: opened.expiresAt, executing: false)
        return claim
    }

    func beginTerminalRequest(_ claim: CloudTerminalClaim) async throws {
        guard let stored = hostClaims[claim.id], stored.claim == claim else {
            throw CloudFailure(message: "This terminal request is no longer owned by this cloud session.", outcomeUnknown: true)
        }
        var mayHaveBegun = false
        do {
            try check(stored.scope.generation)
            guard stored.expiresAt > Date() else { throw CloudFailure(message: "The terminal request expired before execution.", outcomeUnknown: true) }
            try await requireCapability("terminal_relay", scope: stored.scope)
            let identity = try deviceIdentity()
            let path = "v1/terminal-relay/requests/\(claim.id.uuidString.lowercased())/begin"
            mayHaveBegun = true
            let bytes = try await rawRequestData(path, method: "POST",
                                                 headers: try deviceHeaders(deviceID: stored.targetDeviceID, identity: identity),
                                                 deviceAuthenticated: true, missingMeansDeviceReauth: true,
                                                 deadline: stored.expiresAt, scope: stored.scope)
            let response: CloudBeginResponse = try decode(CloudBeginResponse.self, from: bytes)
            try check(stored.scope.generation)
            guard response.id == claim.id, response.state == "executing" else {
                throw CloudFailure(message: "Cloud did not authorize this terminal request for execution.", outcomeUnknown: true)
            }
            hostClaims[claim.id]?.executing = true
        } catch let failure as CloudFailure {
            hostClaims.removeValue(forKey: claim.id)
            throw failureAfterMutation(failure)
        } catch is CancellationError {
            hostClaims.removeValue(forKey: claim.id)
            guard generation == stored.scope.generation else { throw CancellationError() }
            if mayHaveBegun {
                throw CloudFailure(message: "Cloud could not confirm whether the terminal request began.", outcomeUnknown: true)
            }
            throw CancellationError()
        } catch {
            hostClaims.removeValue(forKey: claim.id)
            try check(stored.scope.generation)
            throw CloudFailure(message: "Cloud could not confirm whether the terminal request began.", outcomeUnknown: true)
        }
    }

    func respondTerminalRequest(_ claim: CloudTerminalClaim, payload: Data) async throws {
        guard let stored = hostClaims[claim.id], stored.claim == claim, stored.executing else {
            throw CloudFailure(message: "This terminal request is not authorized for a response.", outcomeUnknown: true)
        }
        defer { hostClaims.removeValue(forKey: claim.id) }
        try check(stored.scope.generation)
        guard stored.expiresAt > Date() else {
            throw CloudFailure(message: "The terminal request expired before its response was sent.", outcomeUnknown: true)
        }
        guard payload.count <= CloudTerminalTransport.maximumPlaintextBytes else {
            throw CloudFailure(message: "Terminal response exceeded the 256 KiB limit.")
        }
        try await requireCapability("terminal_relay", scope: stored.scope)
        let identity = try deviceIdentity()
        let encrypted = try CloudTerminalTransport.seal(payload, using: identity, peerPublicKey: claim.senderPublicKey,
                                                        context: stored.cryptoContext, direction: .response,
                                                        issuedAt: stored.issuedAt, expiresAt: stored.expiresAt)
        let path = "v1/terminal-relay/requests/\(claim.id.uuidString.lowercased())/response"
        let body = try encode(CloudPayloadBody(payload: encrypted))
        let headers = try deviceHeaders(deviceID: stored.targetDeviceID, identity: identity)
        do {
            _ = try await rawRequestData(path, method: "POST", body: body,
                                         headers: headers,
                                         deviceAuthenticated: true, missingMeansDeviceReauth: true,
                                         deadline: stored.expiresAt, scope: stored.scope)
            try check(stored.scope.generation)
        } catch let failure as CloudFailure {
            throw failureAfterMutation(failure)
        } catch is CancellationError {
            guard generation == stored.scope.generation else { throw CancellationError() }
            throw CloudFailure(message: "Cloud could not confirm delivery of the terminal response.", outcomeUnknown: true)
        } catch {
            guard generation == stored.scope.generation else { throw CancellationError() }
            throw CloudFailure(message: "Cloud could not confirm delivery of the terminal response.", outcomeUnknown: true)
        }
    }

    func sendTerminalRequest(to targetDevice: CloudDevice, payload: Data) async throws -> Data {
        guard payload.count <= CloudTerminalTransport.maximumPlaintextBytes else {
            throw CloudFailure(message: "Terminal request exceeded the 256 KiB limit.")
        }
        let scope = try currentScope()
        try await requireCapability("terminal_relay", scope: scope)
        let identity = try deviceIdentity()
        let devices = try await deviceDirectory(scope: scope)
        try check(scope.generation)
        let local = try localCloudDevice(in: devices, identity: identity)
        guard local.revokedAt == nil, let senderID = UUID(uuidString: local.id),
              let targetID = UUID(uuidString: targetDevice.id), targetID != senderID,
              let target = devices.first(where: { UUID(uuidString: $0.id) == targetID }),
              target.revokedAt == nil, target.online, target.sharing,
              let targetEpoch = target.sharingEpoch, let targetPublicKey = target.publicKey,
              targetPublicKey.count == 32 else {
            throw CloudFailure(message: "Choose an online registered device that has enabled terminal sharing.")
        }

        let requestID = UUID()
        let issuedAt = Date()
        let expiresAt = issuedAt.addingTimeInterval(18)
        let deadline = issuedAt.addingTimeInterval(20)
        let context = CloudTerminalCryptoContext(requestID: requestID, senderDeviceID: senderID,
                                                targetDeviceID: targetID, targetEpoch: targetEpoch)
        let encrypted = try CloudTerminalTransport.seal(payload, using: identity, peerPublicKey: targetPublicKey,
                                                        context: context, direction: .request,
                                                        issuedAt: issuedAt, expiresAt: expiresAt)
        let body = try encode(CloudQueuedRequestBody(id: requestID.uuidString.lowercased(),
                                                    targetDeviceID: targetID.uuidString.lowercased(), payload: encrypted))
        guard body.count <= 384 * 1024 else { throw CloudFailure(message: "Terminal request exceeded the relay body limit.") }
        let proof = try deviceHeaders(deviceID: senderID, identity: identity)
        var effectiveDeadline = deadline
        do {
            let bytes = try await rawRequestData("v1/terminal-relay/requests", method: "POST", body: body,
                                                 headers: proof, deviceAuthenticated: true,
                                                 missingMeansDeviceReauth: true, deadline: deadline, scope: scope)
            let queued: CloudQueuedRequest = try decode(CloudQueuedRequest.self, from: bytes)
            try check(scope.generation)
            guard queued.id == requestID, let serverExpiry = CloudDate.parse(queued.expiresAt) else {
                throw CloudFailure(message: "Cloud returned an invalid terminal request receipt.", outcomeUnknown: true)
            }
            effectiveDeadline = min(min(deadline, serverExpiry), expiresAt)
        } catch let failure as CloudFailure {
            if failure.requiresSignIn || failure.requiresDeviceReauth {
                throw failure
            }
            if let status = failure.httpStatusCode, (400..<500).contains(status), status != 429 { throw failure }
            if failure.httpStatusCode == nil || failure.httpStatusCode == 429 {
                throw CloudFailure(message: failure.message, requiresSignIn: failure.requiresSignIn,
                                   requiresDeviceReauth: failure.requiresDeviceReauth,
                                   requiresServerUpgrade: failure.requiresServerUpgrade,
                                   outcomeUnknown: true, httpStatusCode: failure.httpStatusCode)
            }
            // A lost/5xx enqueue acknowledgement is ambiguous. Query this one
            // request ID; never send the mutation again.
        } catch is CancellationError {
            guard generation == scope.generation else { throw CancellationError() }
            throw CloudFailure(message: "Cloud could not confirm whether the terminal request was accepted.", outcomeUnknown: true)
        } catch {
            guard generation == scope.generation else { throw CancellationError() }
            if Task.isCancelled {
                throw CloudFailure(message: "Cloud could not confirm whether the terminal request was accepted.", outcomeUnknown: true)
            }
            // Query the original ID to distinguish a lost acknowledgement from
            // an unaccepted request without creating a duplicate mutation.
        }
        return try await waitForTerminalResponse(requestID: requestID, identity: identity,
                                                 senderID: senderID, targetID: targetID, targetEpoch: targetEpoch,
                                                 peerPublicKey: targetPublicKey, issuedAt: issuedAt,
                                                 expiresAt: expiresAt, deadline: effectiveDeadline,
                                                 headers: proof, scope: scope)
    }

    func signOut() async throws -> String {
        try Task.checkCancellation()
        generation = UUID()
        hostClaims.removeAll()
        capabilityCache = nil
        deviceDirectoryCache = nil
        let current = generation
        var remoteError: (any Error)?
        var revoke: RequestScope?
        do { revoke = try currentScope() } catch { remoteError = error }
        // Local sign-out owns this synchronous boundary. A later login must
        // never be erased by the older revocation response or its HTTP 401.
        try erase()
        token = nil
        if let revoke {
            do { _ = try await rawRequest("v1/native/revoke", method: "POST", scope: revoke) }
            catch { remoteError = error }
        }
        try check(current)
        if remoteError != nil {
            return "Signed out on this Mac. Cloud could not confirm token revocation; revoke the session after reconnecting."
        }
        return "Signed out on this Mac."
    }
    private func waitForTerminalResponse(requestID: UUID, identity: CloudDeviceIdentity,
                                         senderID: UUID, targetID: UUID, targetEpoch: UUID,
                                         peerPublicKey: Data, issuedAt: Date, expiresAt: Date,
                                         deadline: Date, headers: [String: String],
                                         scope: RequestScope) async throws -> Data {
        var backoffMilliseconds: Int64 = 100
        while Date() < deadline {
            try check(scope.generation)
            let path = "v1/terminal-relay/requests/\(requestID.uuidString.lowercased())"
            do {
                let bytes = try await rawRequestData(path, headers: headers, deviceAuthenticated: true,
                                                     missingMeansDeviceReauth: false, deadline: deadline, scope: scope)
                let status: CloudRequestStatus = try decode(CloudRequestStatus.self, from: bytes)
                try check(scope.generation)
                guard status.id == requestID else {
                    throw CloudFailure(message: "Cloud returned a different terminal request.", outcomeUnknown: true)
                }
                switch status.state {
                case "completed":
                    guard let encrypted = status.payload, let serverExpiry = CloudDate.parse(status.expiresAt) else {
                        throw CloudFailure(message: "Cloud completed the terminal request without a usable reply.", outcomeUnknown: true)
                    }
                    let context = CloudTerminalCryptoContext(requestID: requestID, senderDeviceID: senderID,
                                                             targetDeviceID: targetID, targetEpoch: targetEpoch)
                    let opened = try CloudTerminalTransport.open(encrypted, using: identity, peerPublicKey: peerPublicKey,
                                                                  expected: context, direction: .response,
                                                                  serverExpiresAt: serverExpiry)
                    guard abs(opened.issuedAt.timeIntervalSince(issuedAt)) <= 0.002,
                          abs(opened.expiresAt.timeIntervalSince(expiresAt)) <= 0.002 else {
                        throw CloudFailure(message: "Cloud returned a reply for a different terminal request lifetime.", outcomeUnknown: true)
                    }
                    try check(scope.generation)
                    return opened.payload
                case "expired":
                    throw CloudFailure(message: "The remote terminal request expired. Its outcome may be unknown.", outcomeUnknown: true)
                case "pending", "claimed", "executing":
                    try await sleepUntil(deadline, milliseconds: 100)
                default:
                    throw CloudFailure(message: "Cloud returned an unknown terminal request state.", outcomeUnknown: true)
                }
                backoffMilliseconds = 100
            } catch let failure as CloudFailure {
                if failure.outcomeUnknown { throw failure }
                if failure.requiresSignIn || failure.requiresDeviceReauth {
                    throw CloudFailure(message: failure.message, requiresSignIn: failure.requiresSignIn,
                                       requiresDeviceReauth: failure.requiresDeviceReauth,
                                       requiresServerUpgrade: failure.requiresServerUpgrade,
                                       outcomeUnknown: true, httpStatusCode: failure.httpStatusCode)
                }
                if failure.httpStatusCode == 404 {
                    throw CloudFailure(message: "Cloud could not confirm whether the terminal request was accepted.", outcomeUnknown: true, httpStatusCode: 404)
                }
                if failure.httpStatusCode == 429 {
                    try await sleepUntil(deadline, milliseconds: backoffMilliseconds)
                    backoffMilliseconds = min(backoffMilliseconds * 2, 800)
                    continue
                }
                guard let status = failure.httpStatusCode, status >= 500 else {
                    throw CloudFailure(message: failure.message, requiresSignIn: failure.requiresSignIn,
                                       requiresDeviceReauth: failure.requiresDeviceReauth,
                                       requiresServerUpgrade: failure.requiresServerUpgrade,
                                       outcomeUnknown: true, httpStatusCode: failure.httpStatusCode)
                }
                try await sleepUntil(deadline, milliseconds: backoffMilliseconds)
                backoffMilliseconds = min(backoffMilliseconds * 2, 800)
            } catch is CancellationError {
                throw CancellationError()
            } catch {
                try check(scope.generation)
                if let failure = error as? CloudFailure {
                    throw CloudFailure(message: failure.message, requiresSignIn: failure.requiresSignIn,
                                       requiresDeviceReauth: failure.requiresDeviceReauth,
                                       requiresServerUpgrade: failure.requiresServerUpgrade,
                                       outcomeUnknown: true, httpStatusCode: failure.httpStatusCode)
                }
                try await sleepUntil(deadline, milliseconds: backoffMilliseconds)
                backoffMilliseconds = min(backoffMilliseconds * 2, 800)
            }
        }
        throw CloudFailure(message: "The remote terminal request timed out. Its outcome may be unknown.", outcomeUnknown: true)
    }

    private func sleepUntil(_ deadline: Date, milliseconds: Int64) async throws {
        let remaining = deadline.timeIntervalSinceNow
        guard remaining > 0 else { return }
        let requested = Double(milliseconds) / 1_000
        let duration = min(remaining, requested)
        try await Task.sleep(for: .milliseconds(Int64((duration * 1_000).rounded(.down))))
    }

    private func requireCapability(_ capability: String, scope: RequestScope, refresh: Bool = false) async throws {
        try check(scope.generation)
        if !refresh, let cache = capabilityCache, cache.generation == scope.generation,
           Date().timeIntervalSince(cache.checkedAt) < 10 {
            guard capabilityValue(capability, cache: cache) else { throw upgradeFailure(capability) }
            return
        }
        let configuration: CloudConfiguration = try await request("v1/config", scope: scope)
        try check(scope.generation)
        let cache = CloudCapabilityCache(generation: scope.generation, checkedAt: Date(),
                                         stableDeviceIdentity: configuration.capabilities.stableDeviceIdentity == true,
                                         terminalRelay: configuration.capabilities.terminalRelay == true)
        capabilityCache = cache
        guard capabilityValue(capability, cache: cache) else { throw upgradeFailure(capability) }
    }

    private func capabilityValue(_ capability: String, cache: CloudCapabilityCache) -> Bool {
        switch capability {
        case "stable_device_identity": cache.stableDeviceIdentity
        case "terminal_relay": cache.terminalRelay
        default: false
        }
    }

    private func upgradeFailure(_ capability: String) -> CloudFailure {
        let message = capability == "stable_device_identity"
            ? "This cloud service needs an update before it can register this Mac safely."
            : "This cloud service needs an update before terminal sharing is available."
        return CloudFailure(message: message, requiresServerUpgrade: true)
    }

    private func deviceIdentity() throws -> CloudDeviceIdentity {
        if let deviceIdentityCache { return deviceIdentityCache }
        let identity = try CloudDeviceIdentity(endpoint: endpoint)
        deviceIdentityCache = identity
        return identity
    }

    private func deviceDirectory(scope: RequestScope, refresh: Bool = false) async throws -> [CloudDevice] {
        try check(scope.generation)
        guard let credential = scope.credential else {
            throw CloudFailure(message: "Sign in to connect this Mac to zmux Cloud.", requiresSignIn: true)
        }
        if !refresh, let cache = deviceDirectoryCache,
           cache.generation == scope.generation,
           cache.accessToken == credential.accessToken,
           cache.tokenExpiresAt == credential.expiresAt,
           Date().timeIntervalSince(cache.checkedAt) < 5 {
            return cache.devices
        }
        let list: CloudDevices = try await request("v1/devices", scope: scope)
        try check(scope.generation)
        deviceDirectoryCache = CloudDeviceDirectoryCache(generation: scope.generation,
                                                         accessToken: credential.accessToken,
                                                         tokenExpiresAt: credential.expiresAt,
                                                         checkedAt: Date(), devices: list.devices)
        return list.devices
    }

    private func localCloudDevice(in devices: [CloudDevice], identity: CloudDeviceIdentity) throws -> CloudDevice {
        guard let device = devices.first(where: { $0.installationID == identity.publicIdentity.installationID }) else {
            throw CloudFailure(message: "Register this Mac before using remote terminal access.", requiresDeviceReauth: true)
        }
        return device
    }

    private func deviceHeaders(deviceID: UUID, identity: CloudDeviceIdentity) throws -> [String: String] {
        ["X-Zmux-Device-Id": deviceID.uuidString.lowercased(),
         "X-Zmux-Device-Secret": identity.deviceSecretHeader]
    }

    private func encode<T: Encodable>(_ value: T) throws -> Data {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        return try encoder.encode(value)
    }

    private func decode<T: Decodable>(_ type: T.Type, from data: Data) throws -> T {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        do { return try decoder.decode(type, from: data) }
        catch { throw CloudFailure(message: "The cloud response is incompatible with this version of zmux.") }
    }

    private func check(_ expected: UUID) throws {
        try Task.checkCancellation()
        guard generation == expected else { throw CancellationError() }
    }
    private func unknownOutcome(_ failure: CloudFailure) -> CloudFailure {
        CloudFailure(message: failure.message, requiresSignIn: failure.requiresSignIn,
                     requiresDeviceReauth: failure.requiresDeviceReauth,
                     requiresServerUpgrade: failure.requiresServerUpgrade,
                     outcomeUnknown: true, httpStatusCode: failure.httpStatusCode)
    }
    private func failureAfterMutation(_ failure: CloudFailure) -> CloudFailure {
        if failure.outcomeUnknown || failure.requiresSignIn || failure.requiresDeviceReauth ||
            (failure.httpStatusCode.map { (400..<500).contains($0) && $0 != 429 } ?? false) {
            return failure
        }
        return unknownOutcome(failure)
    }
    private func currentScope() throws -> RequestScope {
        try Task.checkCancellation()
        if token == nil { token = try load() }
        guard let token, token.expiresAt > Date() else {
            throw CloudFailure(message: "Sign in to connect this Mac to zmux Cloud.", requiresSignIn: true)
        }
        return RequestScope(generation: generation, credential: token)
    }
    private func request<T: Decodable>(_ path: String, method: String = "GET", body: [String: String]? = nil,
                                       scope: RequestScope) async throws -> T {
        let data = try await rawRequest(path, method: method, body: body, scope: scope)
        return try decode(T.self, from: data)
    }
    private func rawRequest(_ path: String, method: String, body: [String: String]? = nil,
                            scope: RequestScope) async throws -> Data {
        let bodyData = try body.map { try JSONSerialization.data(withJSONObject: $0) }
        return try await rawRequestData(path, method: method, body: bodyData, scope: scope)
    }
    private func rawRequestData(_ path: String, method: String = "GET", body: Data? = nil,
                                headers: [String: String] = [:], deviceAuthenticated: Bool = false,
                                missingMeansDeviceReauth: Bool = false, deadline: Date? = nil,
                                scope: RequestScope) async throws -> Data {
        try check(scope.generation)
        var request = URLRequest(url: endpoint.url(path))
        request.httpMethod = method
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        if let body {
            guard body.count <= 1_048_576 else { throw CloudFailure(message: "Cloud request exceeded the size limit.") }
            request.httpBody = body
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        if let token = scope.credential {
            request.setValue("Bearer \(token.accessToken)", forHTTPHeaderField: "Authorization")
        }
        for (name, value) in headers { request.setValue(value, forHTTPHeaderField: name) }
        if let deadline {
            let remaining = deadline.timeIntervalSinceNow
            guard remaining > 0 else {
                throw CloudFailure(message: "The terminal request reached its deadline.", outcomeUnknown: true)
            }
            request.timeoutInterval = min(15, remaining)
        }
        let (bytes, response) = try await session.bytes(for: request)
        try check(scope.generation)
        guard let http = response as? HTTPURLResponse else { throw CloudFailure(message: "Cloud did not return an HTTP response.") }
        guard (200..<300).contains(http.statusCode) else {
            if http.statusCode == 401 {
                let ownsCredential = scope.generation == generation &&
                    (scope.credential.map { $0.accessToken == token?.accessToken } ?? false)
                if ownsCredential {
                    try erase()
                    token = nil
                    generation = UUID()
                    hostClaims.removeAll()
                    capabilityCache = nil
                    deviceDirectoryCache = nil
                }
                throw CloudFailure(message: "Your cloud session has expired or was revoked. Sign in again.", requiresSignIn: ownsCredential)
            }
            if deviceAuthenticated && http.statusCode == 403 {
                throw CloudFailure(message: "This Mac's cloud device authorization was revoked or expired.",
                                   requiresDeviceReauth: true, httpStatusCode: http.statusCode)
            }
            if deviceAuthenticated && http.statusCode == 404 && missingMeansDeviceReauth {
                throw CloudFailure(message: "This Mac's cloud device registration is no longer available.",
                                   requiresDeviceReauth: true, httpStatusCode: http.statusCode)
            }
            if path == "v1/devices" && http.statusCode == 409 {
                throw CloudFailure(message: "This Mac's cloud device registration was revoked and cannot be reused.",
                                   requiresDeviceReauth: true, httpStatusCode: http.statusCode)
            }
            throw CloudFailure(message: "Cloud request failed (HTTP \(http.statusCode)).", httpStatusCode: http.statusCode)
        }
        var data = Data()
        for try await byte in bytes {
            try check(scope.generation)
            guard data.count < 1_048_576 else { throw CloudFailure(message: "Cloud response exceeded the size limit.") }
            data.append(byte)
        }
        try check(scope.generation)
        return data
    }
    private var keychainQuery: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: keychainService,
         kSecAttrAccount as String: endpoint.origin.absoluteString]
    }
    private func load() throws -> StoredCloudToken? {
        var query = keychainQuery
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = item as? Data else {
            throw CloudFailure(message: "Unable to read the cloud credential from Keychain (\(status)).")
        }
        return try JSONDecoder().decode(StoredCloudToken.self, from: data)
    }
    private func save(_ token: StoredCloudToken) throws {
        let data = try JSONEncoder().encode(token)
        let status = SecItemUpdate(keychainQuery as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if status == errSecItemNotFound {
            var query = keychainQuery
            query[kSecValueData as String] = data
            query[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            let added = SecItemAdd(query as CFDictionary, nil)
            guard added == errSecSuccess else { throw CloudFailure(message: "Unable to save the cloud credential in Keychain (\(added)).") }
        } else if status != errSecSuccess { throw CloudFailure(message: "Unable to update the cloud credential in Keychain (\(status)).") }
    }
    private func erase() throws {
        let status = SecItemDelete(keychainQuery as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw CloudFailure(message: "Unable to remove the cloud credential from Keychain (\(status)).")
        }
    }
}

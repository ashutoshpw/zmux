import Foundation
import Security
import CryptoKit

struct CloudFailure: Error, LocalizedError, Sendable {
    let message: String
    let requiresSignIn: Bool
    init(message: String, requiresSignIn: Bool = false) { self.message = message; self.requiresSignIn = requiresSignIn }
    var errorDescription: String? { message }
}

struct CloudUser: Decodable, Sendable { let id: String; let name: String; let email: String }
struct CloudIdentity: Decodable, Sendable { let user: CloudUser }
struct CloudDevice: Decodable, Sendable {
    let id: String
    let name: String
    let platform: String
    let lastSeenAt: String?
    let revokedAt: String?
}
struct CloudDevices: Decodable, Sendable { let devices: [CloudDevice] }
struct CloudAccount: Sendable { let user: CloudUser; let devices: [CloudDevice] }
private struct CloudToken: Decodable { let accessToken: String; let expiresIn: Int; let tokenType: String }
private struct StoredCloudToken: Codable, Sendable { let accessToken: String; let expiresAt: Date }

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
    private struct RequestScope: Sendable {
        let generation: UUID
        let credential: StoredCloudToken?
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
    func complete(code: String, verifier: String) async throws -> CloudAccount {
        try Task.checkCancellation()
        generation = UUID()
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
        return account
    }
    func registerDevice(name: String) async throws {
        _ = try await rawRequest("v1/devices", method: "POST", body: ["name": name, "platform": "macos"], scope: currentScope())
    }
    func revokeDevice(id: String) async throws {
        guard !id.isEmpty, id.utf8.count <= 128, id.allSatisfy({ $0.isLetter || $0.isNumber || $0 == "-" || $0 == "_" }) else {
            throw CloudFailure(message: "Invalid device identifier.")
        }
        _ = try await rawRequest("v1/devices/\(id)", method: "DELETE", scope: currentScope())
    }
    func signOut() async throws -> String {
        try Task.checkCancellation()
        generation = UUID()
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
    private func check(_ expected: UUID) throws {
        try Task.checkCancellation()
        guard generation == expected else { throw CancellationError() }
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
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        do { return try decoder.decode(T.self, from: data) }
        catch { throw CloudFailure(message: "The cloud response is incompatible with this version of zmux.") }
    }
    private func rawRequest(_ path: String, method: String, body: [String: String]? = nil,
                            scope: RequestScope) async throws -> Data {
        try check(scope.generation)
        var request = URLRequest(url: endpoint.url(path))
        request.httpMethod = method
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        if let body {
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        if let token = scope.credential {
            request.setValue("Bearer \(token.accessToken)", forHTTPHeaderField: "Authorization")
        }
        let (bytes, response) = try await session.bytes(for: request)
        try check(scope.generation)
        guard let http = response as? HTTPURLResponse else { throw CloudFailure(message: "Cloud did not return an HTTP response.") }
        guard (200..<300).contains(http.statusCode) else {
            if http.statusCode == 401 {
                let ownsCredential = scope.credential.map { $0.accessToken == token?.accessToken } ?? false
                if ownsCredential {
                    try erase()
                    token = nil
                    generation = UUID()
                }
                throw CloudFailure(message: "Your cloud session has expired or was revoked. Sign in again.", requiresSignIn: ownsCredential)
            }
            throw CloudFailure(message: "Cloud request failed (HTTP \(http.statusCode)).")
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

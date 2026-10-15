import Foundation
import Security
import Darwin
import LocalAuthentication

/// Real HTTP and Keychain regression coverage; no fake credential store/session.
@main
enum CloudClientProbe {
    struct Failure: Error, CustomStringConvertible, LocalizedError {
        let description: String
        var errorDescription: String? { description }
    }

    static func require(_ condition: @autoclosure () -> Bool, _ label: String) throws {
        guard condition() else { throw Failure(description: label) }
    }
    static func control(_ endpoint: CloudEndpoint, _ action: String, _ body: [String: String]) async throws -> [String: Bool] {
        var request = URLRequest(url: endpoint.url("fixture/" + action))
        request.httpMethod = "POST"
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        let (data, response) = try await URLSession.shared.data(for: request)
        guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw Failure(description: "Fixture control failed") }
        return (try JSONSerialization.jsonObject(with: data) as? [String: Bool]) ?? [:]
    }
    static func hold(_ endpoint: CloudEndpoint, _ name: String, route: String, subject: String, status: Int = 200) async throws {
        _ = try await control(endpoint, "hold", ["name": name, "route": route, "subject": subject, "status": String(status)])
    }
    static func entered(_ endpoint: CloudEndpoint, _ name: String) async throws {
        let deadline = ContinuousClock.now + .seconds(5)
        while try await control(endpoint, "entered", ["name": name])["entered"] != true {
            try require(ContinuousClock.now < deadline, "Native request did not reach its HTTP gate")
            try await Task.sleep(for: .milliseconds(10))
        }
    }
    static func release(_ endpoint: CloudEndpoint, _ name: String) async throws {
        _ = try await control(endpoint, "release", ["name": name])
    }
    static func cancelled<T: Sendable>(_ task: Task<T, any Error>, _ label: String) async throws {
        do { _ = try await task.value }
        catch {
            try require(error is CancellationError || task.isCancelled, label)
            return
        }
        throw Failure(description: label)
    }
    static func signedIn(_ client: CloudClient, _ subject: String) async throws {
        let account = try await client.complete(code: subject, verifier: "fixture-verifier")
        try require(account.user.id == subject && account.devices.map(\.id) == ["device_" + subject], "Sign-in mixed account ownership")
    }
    static func saved(_ endpoint: CloudEndpoint, _ client: CloudClient, _ subject: String,
                      failure: String = "Committed account or Keychain credential changed") async throws {
        for reader in [client, CloudClient(endpoint: endpoint)] {
            let account = try await reader.account()
            try require(account.user.id == subject && account.devices.map(\.id) == ["device_" + subject], failure)
        }
    }
    static func signedOut(_ client: CloudClient) async throws {
        do { _ = try await client.account() }
        catch let error as CloudFailure {
            try require(error.requiresSignIn, "Missing credential was not reported as signed out")
            return
        }
        throw Failure(description: "Credential remained available after sign-out")
    }
    static func credentialQuery(_ endpoint: CloudEndpoint) throws -> [String: Any] {
        guard endpoint.origin.scheme == "http", endpoint.origin.host == "127.0.0.1", endpoint.origin.port != nil else {
            throw Failure(description: "Fixture refuses any non-loopback credential origin")
        }
        let context = LAContext()
        context.interactionNotAllowed = true
        return [kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: "app.w3dev.zmux.dev.cloud",
                kSecAttrAccount as String: endpoint.origin.absoluteString,
                kSecUseAuthenticationContext as String: context]
    }
    static func removeCredential(_ query: [String: Any]) throws {
        let status = SecItemDelete(query as CFDictionary)
        try require(status == errSecSuccess || status == errSecItemNotFound, "Fixture Keychain cleanup failed")
        try require(SecItemCopyMatching(query as CFDictionary, nil) == errSecItemNotFound, "Fixture Keychain cleanup was not verified")
    }
    static func validate(_ endpoint: CloudEndpoint) async throws {
        let client = CloudClient(endpoint: endpoint)
        try await signedIn(client, "baseline")
        try await hold(endpoint, "cancel", route: "me", subject: "cancelled")
        let cancel = Task { try await client.complete(code: "cancelled", verifier: "fixture-verifier") }
        try await entered(endpoint, "cancel")
        cancel.cancel()
        try await cancelled(cancel, "Cancelled sign-in completed")
        try await release(endpoint, "cancel")
        try await saved(endpoint, client, "baseline", failure: "Cancelled sign-in changed the committed credential")

        try await hold(endpoint, "older", route: "token", subject: "older")
        let older = Task { try await client.complete(code: "older", verifier: "fixture-verifier") }
        try await entered(endpoint, "older")
        try await signedIn(client, "winner")
        try await release(endpoint, "older")
        try await cancelled(older, "Superseded token exchange completed")
        try await saved(endpoint, client, "winner")

        try await hold(endpoint, "stale401", route: "me", subject: "winner", status: 401)
        let stale401 = Task { try await client.account() }
        try await entered(endpoint, "stale401")
        try await signedIn(client, "replacement")
        try await release(endpoint, "stale401")
        try await cancelled(stale401, "Stale unauthorized response remained current")
        try await saved(endpoint, client, "replacement")

        try await hold(endpoint, "oldRevoke", route: "revoke", subject: "replacement", status: 401)
        let oldSignOut = Task { try await client.signOut() }
        try await entered(endpoint, "oldRevoke")
        try await signedIn(client, "newest")
        try await release(endpoint, "oldRevoke")
        try await cancelled(oldSignOut, "Older sign-out completed against a replacement credential")
        try await saved(endpoint, client, "newest")

        do {
            _ = try await client.complete(code: "reject", verifier: "fixture-verifier")
            throw Failure(description: "Unauthorized token exchange succeeded")
        } catch let error as CloudFailure {
            try require(!error.requiresSignIn, "Unauthenticated exchange cleared the committed account")
        }
        try await saved(endpoint, client, "newest")

        try await hold(endpoint, "current401", route: "me", subject: "newest", status: 401)
        let current401 = Task { try await client.account() }
        try await entered(endpoint, "current401")
        try await release(endpoint, "current401")
        do {
            _ = try await current401.value
            throw Failure(description: "Current unauthorized credential was accepted")
        } catch let error as CloudFailure {
            try require(error.requiresSignIn, "Current unauthorized credential was not invalidated")
        }
        try await signedOut(CloudClient(endpoint: endpoint))

        try await signedIn(client, "final")
        try await hold(endpoint, "localFirst", route: "revoke", subject: "final")
        let localFirst = Task { try await client.signOut() }
        try await entered(endpoint, "localFirst")
        try await signedOut(CloudClient(endpoint: endpoint))
        try await release(endpoint, "localFirst")
        _ = try await localFirst.value
    }
    static func main() async {
        var ownedQuery: [String: Any]?
        var failure: (any Error)?
        do {
            if CommandLine.arguments.count == 3 {
                let endpoint = try CloudEndpoint(CommandLine.arguments[2])
                let query = try credentialQuery(endpoint)
                switch CommandLine.arguments[1] {
                case "--prepare":
                    try require(SecItemCopyMatching(query as CFDictionary, nil) == errSecItemNotFound,
                                "Fixture refuses a Keychain origin that already exists")
                case "--cleanup": try removeCredential(query)
                default: throw Failure(description: "Unsupported fixture lifecycle phase")
                }
                return
            }
            guard CommandLine.arguments.count == 2 else { throw Failure(description: "Fixture requires its owned loopback origin") }
            let endpoint = try CloudEndpoint(CommandLine.arguments[1])
            let query = try credentialQuery(endpoint)
            guard SecItemCopyMatching(query as CFDictionary, nil) == errSecItemNotFound else {
                throw Failure(description: "Fixture refuses a Keychain origin that already exists")
            }
            ownedQuery = query
            try await validate(endpoint)
        } catch { failure = error }
        if let ownedQuery {
            do { try removeCredential(ownedQuery) }
            catch {
                failure = Failure(description: (failure.map { $0.localizedDescription + "; " } ?? "") + error.localizedDescription)
            }
        }
        if let failure {
            FileHandle.standardError.write(Data(("Native CloudClient fixture failed: \(failure.localizedDescription)\n").utf8))
            exit(1)
        }
        print("Native CloudClient passed: cancellation, superseded exchange, stale 401/revoke, current invalidation, local-first sign-out and verified isolated Keychain cleanup.")
    }
}

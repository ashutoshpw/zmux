import CryptoKit
import Darwin
import Foundation
import Security

@main
enum CloudTerminalProbe {
    struct Failure: Error, Sendable, CustomStringConvertible, LocalizedError {
        let description: String
        var errorDescription: String? { description }
    }

    private struct TestHeader: Encodable {
        let version: Int
        let requestId: UUID
        let senderDeviceId: UUID
        let targetDeviceId: UUID
        let targetEpoch: UUID
        let issuedAtMilliseconds: Int64
        let expiresAtMilliseconds: Int64
    }

    private struct FixtureStats: Decodable {
        let deviceReads: [String: Int]
        let queueWrites: [String: Int]
    }

    static func require(_ condition: @autoclosure () -> Bool, _ label: String) throws {
        guard condition() else { throw Failure(description: label) }
    }

    static func credentialQuery(_ endpoint: CloudEndpoint) throws -> [String: Any] {
        guard endpoint.origin.scheme == "http", endpoint.origin.host == "127.0.0.1", endpoint.origin.port != nil else {
            throw Failure(description: "Fixture refuses any non-loopback credential origin")
        }
        return [kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: "app.w3dev.zmux.dev.cloud",
                kSecAttrAccount as String: endpoint.origin.absoluteString]
    }

    static func queries(_ endpoints: [CloudEndpoint]) throws -> [[String: Any]] {
        try endpoints.flatMap { endpoint in
            [try credentialQuery(endpoint), CloudDeviceIdentity.keychainQuery(endpoint: endpoint)]
        }
    }

    static func verifyAbsent(_ queries: [[String: Any]]) throws {
        for query in queries {
            guard SecItemCopyMatching(query as CFDictionary, nil) == errSecItemNotFound else {
                throw Failure(description: "Fixture refuses a Keychain origin that already exists")
            }
        }
    }

    static func directoryReads(_ endpoint: CloudEndpoint, subject: String) async throws -> Int {
        let stats = try await fixtureStats(endpoint)
        return stats.deviceReads[subject] ?? 0
    }

    static func fixtureStats(_ endpoint: CloudEndpoint) async throws -> FixtureStats {
        let request = URLRequest(url: endpoint.url("fixture/stats"))
        let (data, response) = try await URLSession.shared.data(for: request)
        guard (response as? HTTPURLResponse)?.statusCode == 200 else {
            throw Failure(description: "Fixture did not return request counters")
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(FixtureStats.self, from: data)
    }

    static func failNextStatusRead(_ endpoint: CloudEndpoint, deviceID: String, status: Int) async throws {
        var request = URLRequest(url: endpoint.url("fixture/fail-status"))
        request.httpMethod = "POST"
        request.httpBody = try JSONSerialization.data(withJSONObject: ["device_id": deviceID, "status": status])
        let (_, response) = try await URLSession.shared.data(for: request)
        guard (response as? HTTPURLResponse)?.statusCode == 200 else {
            throw Failure(description: "Fixture refused a controlled status response")
        }
    }

    static func removeOwned(_ queries: [[String: Any]]) throws {
        var failure: (any Error)?
        for query in queries {
            let status = SecItemDelete(query as CFDictionary)
            if status != errSecSuccess && status != errSecItemNotFound {
                failure = Failure(description: "Fixture Keychain cleanup failed")
                continue
            }
            if SecItemCopyMatching(query as CFDictionary, nil) != errSecItemNotFound {
                failure = Failure(description: "Fixture Keychain cleanup was not verified")
            }
        }
        if let failure { throw failure }
    }

    static func expectCloudFailure(_ operation: () throws -> Void, _ label: String) throws {
        do { try operation() }
        catch is CloudFailure { return }
        catch { throw Failure(description: label + ": unexpected error type") }
        throw Failure(description: label)
    }

    static func malformedEnvelope(context: CloudTerminalCryptoContext, issuedAt: Int64, expiresAt: Int64) throws -> String {
        let header = TestHeader(version: 1, requestId: context.requestID, senderDeviceId: context.senderDeviceID,
                                targetDeviceId: context.targetDeviceID, targetEpoch: context.targetEpoch,
                                issuedAtMilliseconds: issuedAt, expiresAtMilliseconds: expiresAt)
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        let headerData = try encoder.encode(header)
        var frame = Data()
        var headerLength = UInt32(headerData.count).bigEndian
        Swift.withUnsafeBytes(of: &headerLength) { frame.append(contentsOf: $0) }
        frame.append(headerData)
        frame.append(Data(repeating: 0, count: 28))
        return frame.cloudBase64URLEncoded()
    }

    static func exerciseCrypto(sender: CloudDeviceIdentity, receiver: CloudDeviceIdentity,
                               senderDevice: CloudDevice, receiverDevice: CloudDevice,
                               epoch: UUID) throws {
        guard let senderID = UUID(uuidString: senderDevice.id),
              let receiverID = UUID(uuidString: receiverDevice.id) else {
            throw Failure(description: "Fixture received a non-UUID registered device identifier")
        }
        let context = CloudTerminalCryptoContext(requestID: UUID(), senderDeviceID: senderID,
                                                 targetDeviceID: receiverID, targetEpoch: epoch)
        let issuedAt = Date()
        let expiresAt = issuedAt.addingTimeInterval(18)
        let payload = Data(repeating: 0x9a, count: CloudTerminalTransport.maximumPlaintextBytes)
        let encoded = try CloudTerminalTransport.seal(payload, using: sender,
                                                      peerPublicKey: receiver.publicIdentity.publicKey,
                                                      context: context, direction: .request,
                                                      issuedAt: issuedAt, expiresAt: expiresAt)
        try require(encoded.utf8.count <= CloudTerminalTransport.maximumEncodedEnvelopeCharacters,
                    "Exact maximum plaintext did not fit the relay envelope bound")
        let opened = try CloudTerminalTransport.open(encoded, using: receiver,
                                                     peerPublicKey: sender.publicIdentity.publicKey,
                                                     expected: context, direction: .request,
                                                     serverExpiresAt: issuedAt.addingTimeInterval(20))
        try require(opened.payload == payload, "Exact maximum plaintext did not round-trip")
        try expectCloudFailure({
            _ = try CloudTerminalTransport.seal(Data(repeating: 0, count: CloudTerminalTransport.maximumPlaintextBytes + 1),
                                                using: sender, peerPublicKey: receiver.publicIdentity.publicKey,
                                                context: context, direction: .request,
                                                issuedAt: issuedAt, expiresAt: expiresAt)
        }, "Oversize terminal plaintext was accepted")
        try expectCloudFailure({
            _ = try CloudTerminalTransport.open(encoded, using: receiver,
                                                peerPublicKey: sender.publicIdentity.publicKey,
                                                expected: context, direction: .response)
        }, "Request envelope authenticated as a response")

        for (issue, expiry) in [(Int64.min, Int64.max), (Int64.max, Int64.min)] {
            let malformed = try malformedEnvelope(context: context, issuedAt: issue, expiresAt: expiry)
            try expectCloudFailure({
                _ = try CloudTerminalTransport.open(malformed, using: receiver,
                                                    peerPublicKey: sender.publicIdentity.publicKey,
                                                    expected: context, direction: .request)
            }, "Malformed extreme terminal timestamp was accepted")
        }
    }

    static func exerciseRelay(senderEndpoint: CloudEndpoint, receiverEndpoint: CloudEndpoint) async throws {
        let sender = CloudClient(endpoint: senderEndpoint)
        let receiver = CloudClient(endpoint: receiverEndpoint)
        _ = try await sender.complete(code: "relay-sender", verifier: "fixture-verifier")
        _ = try await receiver.complete(code: "relay-host", verifier: "fixture-verifier")

        let senderDevice = try await sender.registerDevice(name: "Sender")
        let duplicate = try await sender.registerDevice(name: "Sender renamed")
        try require(senderDevice.id == duplicate.id, "Repeated registration created a duplicate device")
        let receiverDevice = try await receiver.registerDevice(name: "Host")
        let epoch = UUID()
        try await receiver.setTerminalPresence(device: receiverDevice, sharing: true, epoch: epoch)
        let senderDirectory = try await sender.account()
        guard let target = senderDirectory.devices.first(where: { $0.id == receiverDevice.id }) else {
            throw Failure(description: "Sender directory did not return the shared host")
        }
        let senderIdentity = try CloudDeviceIdentity(endpoint: senderEndpoint)
        let receiverIdentity = try CloudDeviceIdentity(endpoint: receiverEndpoint)
        try exerciseCrypto(sender: senderIdentity, receiver: receiverIdentity,
                           senderDevice: senderDevice, receiverDevice: receiverDevice, epoch: epoch)

        for _ in 0..<8 {
            let idleClaim = try await receiver.claimTerminalRequest(device: receiverDevice, epoch: epoch)
            try require(idleClaim == nil, "Idle host unexpectedly claimed a terminal request")
            try await Task.sleep(for: .milliseconds(40))
        }
        let hostDirectoryReads = try await directoryReads(receiverEndpoint, subject: "relay-host")
        try require(hostDirectoryReads <= 3, "Terminal inbox polling repeatedly fetched the device directory")

        let beforeStats = try await fixtureStats(senderEndpoint)
        let queueCountBefore = beforeStats.queueWrites[senderDevice.id] ?? 0
        try await failNextStatusRead(senderEndpoint, deviceID: senderDevice.id, status: 403)
        do {
            _ = try await sender.sendTerminalRequest(to: target, payload: Data("uncertain-request".utf8))
            throw Failure(description: "A forbidden post-enqueue status read was reported as success")
        } catch let cloudFailure as CloudFailure {
            try require(cloudFailure.requiresDeviceReauth && cloudFailure.outcomeUnknown,
                        "A forbidden status read lost its device-reauth or unknown-outcome flag")
        }
        let afterStats = try await fixtureStats(senderEndpoint)
        let queueCountAfter = afterStats.queueWrites[senderDevice.id] ?? 0
        try require(queueCountAfter == queueCountBefore + 1, "An uncertain terminal request was submitted more than once")
        guard let uncertainClaim = try await receiver.claimTerminalRequest(device: receiverDevice, epoch: epoch) else {
            throw Failure(description: "The accepted uncertain request was not available to the host")
        }
        try require(uncertainClaim.payload == Data("uncertain-request".utf8),
                    "The host could not decrypt the accepted uncertain request")
        try await receiver.beginTerminalRequest(uncertainClaim)
        try await receiver.respondTerminalRequest(uncertainClaim, payload: Data("uncertain-request-completed".utf8))

        let requestPayload = Data(#"{"operation":"list","scope":"existing-session"}"#.utf8)
        let responsePayload = Data(#"{"ok":true,"sessions":[]}"#.utf8)
        let host = Task {
            let deadline = ContinuousClock.now + .seconds(5)
            while ContinuousClock.now < deadline {
                if let claim = try await receiver.claimTerminalRequest(device: receiverDevice, epoch: epoch) {
                    try require(claim.payload == requestPayload, "Host received different plaintext from the encrypted request")
                    try await receiver.beginTerminalRequest(claim)
                    try await receiver.respondTerminalRequest(claim, payload: responsePayload)
                    return
                }
                try await Task.sleep(for: .milliseconds(40))
            }
            throw Failure(description: "Host did not receive the encrypted relay request")
        }
        defer { host.cancel() }
        let response = try await sender.sendTerminalRequest(to: target, payload: requestPayload)
        try await host.value
        try require(response == responsePayload, "Sender did not authenticate and decrypt the terminal response")
        try await receiver.setTerminalPresence(device: receiverDevice, sharing: false, epoch: epoch)
    }

    static func main() async {
        var ownedQueries: [[String: Any]]?
        var failure: (any Error)?
        do {
            guard CommandLine.arguments.count == 3 || CommandLine.arguments.count == 4 else {
                throw Failure(description: "Fixture requires two owned loopback origins")
            }
            let offset = CommandLine.arguments.count == 4 ? 2 : 1
            let endpoints = try [CloudEndpoint(CommandLine.arguments[offset]), CloudEndpoint(CommandLine.arguments[offset + 1])]
            let queries = try queries(endpoints)
            if CommandLine.arguments.count == 4 {
                switch CommandLine.arguments[1] {
                case "--prepare": try verifyAbsent(queries)
                case "--cleanup": try removeOwned(queries)
                default: throw Failure(description: "Unsupported fixture lifecycle phase")
                }
                return
            }
            try verifyAbsent(queries)
            ownedQueries = queries
            try await exerciseRelay(senderEndpoint: endpoints[0], receiverEndpoint: endpoints[1])
        } catch { failure = error }
        if let ownedQueries {
            do { try removeOwned(ownedQueries) }
            catch {
                failure = Failure(description: (failure.map { $0.localizedDescription + "; " } ?? "") + error.localizedDescription)
            }
        }
        if let failure {
            FileHandle.standardError.write(Data(("Native terminal fixture failed: \(failure.localizedDescription)\n").utf8))
            exit(1)
        }
        print("Native terminal fixture passed: proof-authenticated registration and relay, request/response encryption, context binding, exact maximum payload, oversized rejection and isolated Keychain cleanup.")
    }
}

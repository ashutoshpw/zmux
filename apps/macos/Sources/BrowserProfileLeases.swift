import Foundation
import WebKit

enum BrowserProfileLeaseDirectory {
    static func resolve() throws -> String {
        // The same OS Library namespace backs WebKit's UUID stores. This fixed
        // app-owned root is independent of configurable Desktop catalog paths
        // and conservatively shared across zmux bundle identifiers.
        let library = try FileManager.default.url(for: .libraryDirectory, in: .userDomainMask,
                                                   appropriateFor: nil, create: true)
        return try NativeStoragePath.resolve(library
            .appendingPathComponent("Application Support", isDirectory: true)
            .appendingPathComponent("app.w3dev.zmux.profile-leases-v1", isDirectory: true).path)
    }
}

struct BrowserProfileLeaseOwner: Codable, Sendable, Equatable {
    let epoch: UUID
    let store: UUID
}
struct BrowserProfileLeaseToken: Codable, Sendable, Equatable {
    let serviceEpoch: UUID
    let id: UInt64
    let owner: BrowserProfileLeaseOwner
    let profile: UUID
    let mode: String
}

struct BrowserProfileConstructionPermit: Codable, Sendable, Equatable {
    let construction: UUID
    let lease: BrowserProfileLeaseToken
}

enum BrowserProfileLeaseClient {
    private struct Request: Encodable, Sendable {
        let operation: String
        var owner: BrowserProfileLeaseOwner?
        var profile: UUID?
        var token: BrowserProfileLeaseToken?
        var construction: UUID?
        var permit: BrowserProfileConstructionPermit?
    }
    private struct Failure: Decodable, Sendable { let message: String }
    private struct Response: Decodable, Sendable {
        let ok: Bool
        let token: BrowserProfileLeaseToken?
        let permit: BrowserProfileConstructionPermit?
        let error: Failure?
        let outcomeUnknown: Bool?
    }

    static func acquire(desktop: UInt64, owner: BrowserProfileLeaseOwner, profile: UUID) async throws -> BrowserProfileLeaseToken {
        let deadline = ContinuousClock.now + .seconds(20)
        var response = try await exchange(desktop: desktop,
            Request(operation: "acquire", owner: owner, profile: profile))
        while true {
            if response.ok, let token = response.token,
               token.owner == owner, token.profile == profile, token.mode == "shared", token.id > 0 {
                return token
            }
            if response.outcomeUnknown != true {
                throw BrowserProfileNativeFailure(message: response.error?.message ?? "Profile lease was not acquired.", outcomeUnknown: false)
            }
            guard ContinuousClock.now < deadline else {
                throw BrowserProfileNativeFailure(message: "Profile lease ownership remains unknown. No website store was constructed; the original owner is retained.", outcomeUnknown: true)
            }
            // Status reconciles the original accepted owner. Never replay an
            // acquisition after a timeout, transport error or lost response.
            try await Task.sleep(for: .milliseconds(32))
            response = try await exchange(desktop: 0, Request(operation: "status", owner: owner))
        }
    }

    static func reuse(desktop: UInt64, token: BrowserProfileLeaseToken, construction: UUID) async throws -> BrowserProfileConstructionPermit {
        var response = try await exchange(desktop: desktop, Request(operation: "reuse", token: token, construction: construction))
        if response.outcomeUnknown == true {
            response = try await exchange(desktop: 0, Request(operation: "construction_status", construction: construction))
        }
        guard response.ok, let permit = response.permit, permit.construction == construction, permit.lease == token else {
            throw BrowserProfileNativeFailure(message: response.error?.message ?? "Original profile construction permit is unknown; acquisition was not replayed.", outcomeUnknown: response.outcomeUnknown ?? !response.ok)
        }
        return permit
    }
    static func constructed(_ token: BrowserProfileLeaseToken) async throws {
        let response = try await exchange(desktop: 0, Request(operation: "constructed", token: token))
        guard response.ok else { throw BrowserProfileNativeFailure(message: response.error?.message ?? "Original profile construction acknowledgment is unknown; owner retained.", outcomeUnknown: true) }
    }
    static func constructionDone(_ permit: BrowserProfileConstructionPermit) async throws {
        var response = try await exchange(desktop: 0, Request(operation: "construction_done", permit: permit))
        if response.outcomeUnknown == true {
            response = try await exchange(desktop: 0, Request(operation: "construction_status", construction: permit.construction))
            if response.ok, response.permit == nil { return }
        }
        guard response.ok else { throw BrowserProfileNativeFailure(message: response.error?.message ?? "Original construction permit remains retained.", outcomeUnknown: true) }
    }

    static func releaseUnconstructed(_ token: BrowserProfileLeaseToken) async throws {
        let response = try await exchange(desktop: 0,
            Request(operation: "release_unconstructed", token: token))
        if response.ok { return }
        if response.outcomeUnknown == true {
            let status = try await exchange(desktop: 0, Request(operation: "status", owner: token.owner))
            if status.ok, status.token == nil { return }
        }
        throw BrowserProfileNativeFailure(message: response.error?.message ?? "Unconstructed profile lease release is unknown; ownership is retained.", outcomeUnknown: true)
    }

    private static func exchange(desktop: UInt64, _ request: Request) async throws -> Response {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        let data = try encoder.encode(request)
        return await Task.detached {
            let result = data.withUnsafeBytes { bytes in
                zmux_desktop_profile_lease(desktop, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
            }
            do {
                guard result.status == 0 else { throw BridgeFailure(message: "Profile lease transport failed (\(result.status)).") }
                let decoder = JSONDecoder()
                decoder.keyDecodingStrategy = .convertFromSnakeCase
                return try decoder.decode(Response.self, from: NativeBridge.consume(result.value))
            } catch {
                // The worker may own a lock even when its response cannot be
                // copied/decoded. Only original-owner status may reconcile it.
                return Response(ok: false, token: nil, permit: nil,
                    error: Failure(message: error.localizedDescription), outcomeUnknown: true)
            }
        }.value
    }
}

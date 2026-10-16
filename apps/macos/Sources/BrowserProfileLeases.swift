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

enum BrowserProfileLeaseClient {
    private struct Request: Encodable, Sendable {
        let operation: String
        var owner: BrowserProfileLeaseOwner?
        var profile: UUID?
        var token: BrowserProfileLeaseToken?
    }
    private struct Failure: Decodable, Sendable { let message: String }
    private struct Response: Decodable, Sendable {
        let ok: Bool
        let token: BrowserProfileLeaseToken?
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
                throw BridgeFailure(message: response.error?.message ?? "Profile lease was not acquired.")
            }
            guard ContinuousClock.now < deadline else {
                throw BridgeFailure(message: "Profile lease ownership remains unknown. No website store was constructed; the original owner is retained.")
            }
            // Status reconciles the original accepted owner. Never replay an
            // acquisition after a timeout, transport error or lost response.
            try await Task.sleep(for: .milliseconds(32))
            response = try await exchange(desktop: 0, Request(operation: "status", owner: owner))
        }
    }

    static func releaseUnconstructed(_ token: BrowserProfileLeaseToken) async throws {
        let response = try await exchange(desktop: 0,
            Request(operation: "release_unconstructed", token: token))
        if response.ok { return }
        if response.outcomeUnknown == true {
            let status = try await exchange(desktop: 0, Request(operation: "status", owner: token.owner))
            if status.ok, status.token == nil { return }
        }
        throw BridgeFailure(message: response.error?.message ?? "Unconstructed profile lease release is unknown; ownership is retained.")
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
                return Response(ok: false, token: nil,
                    error: Failure(message: error.localizedDescription), outcomeUnknown: true)
            }
        }.value
    }
}

/// Construction happens only on MainActor after acknowledged shared ownership.
/// Constructed stores' Rust tokens remain held for the entire process lifetime.
@MainActor
final class BrowserProfileStores {
    private let bridge: NativeBridge
    private var stores: [UUID: WKWebsiteDataStore] = [:]
    private var pending: [UUID: UUID] = [:]
    private var failures: [UUID: UUID] = [:]

    init(bridge: NativeBridge) { self.bridge = bridge }

    func store(for profile: UUID, isNeeded: @escaping @MainActor () -> Bool,
               refresh: @escaping @MainActor () -> Void,
               reportError: @escaping @MainActor (String) -> Void) -> WKWebsiteDataStore? {
        if let store = stores[profile] { return store }
        guard pending[profile] == nil, failures[profile] == nil else { return nil }
        guard stores.count + pending.count + failures.count < 32 else {
            reportError("Browser profile data store capacity reached")
            return nil
        }
        let storeOwner = UUID()
        pending[profile] = storeOwner
        let bridge = bridge
        Task { [weak self] in
            do {
                let token = try await bridge.acquireBrowserProfileLease(profile: profile, store: storeOwner)
                guard let self, self.pending[profile] == storeOwner else {
                    try await bridge.releaseUnconstructedBrowserProfileLease(token)
                    return
                }
                if isNeeded() {
                    // No suspension between the final need check and actual
                    // construction. There is no release path after this point.
                    self.stores[profile] = WKWebsiteDataStore(forIdentifier: profile)
                    self.pending.removeValue(forKey: profile)
                    refresh()
                } else {
                    // Cancellation before any store construction can reclaim
                    // the exact acquired token, including after Desktop close.
                    try await bridge.releaseUnconstructedBrowserProfileLease(token)
                    self.pending.removeValue(forKey: profile)
                    refresh()
                }
            } catch {
                guard let self else {
                    FileHandle.standardError.write(Data(("Profile lease cleanup: " + error.localizedDescription + "\n").utf8))
                    return
                }
                self.pending.removeValue(forKey: profile)
                self.failures[profile] = storeOwner
                reportError("Browser profile store unavailable: " + error.localizedDescription)
            }
        }
        return nil
    }
}

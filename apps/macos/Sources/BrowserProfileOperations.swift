import Foundation
import WebKit

struct BrowserProfileMutationRequest: Decodable, Sendable {
    let controller: UUID
    let intent: UUID
    let profile: UUID
    let kind: String
}
struct BrowserProfileMutationToken: Codable, Sendable, Equatable {
    let serviceEpoch: UUID
    let controller: UUID
    let intent: UUID
    let profile: UUID
}
struct BrowserProfileNativeFailure: LocalizedError, Sendable {
    let message: String
    let outcomeUnknown: Bool
    var errorDescription: String? { message }
}
enum BrowserProfileNativeClient {
    struct Command: Encodable, Sendable {
        let operation: String
        var controller: UUID?
        var intent: UUID?
        var stores: [BrowserProfileLeaseToken]?
        var token: BrowserProfileMutationToken?
        var phase: String?
        var result: Outcome?
        var message: String?
        var profile: UUID?
        var id: UUID?
        var error: String?
    }
    struct Outcome: Encodable, Sendable { let outcome: String; var message: String? }
    struct Empty: Decodable, Sendable {}
    struct RecoveryBatch: Decodable, Sendable { let reconciliations: [Reconciliation] }
    struct Batch: Decodable, Sendable { let requests: [BrowserProfileMutationRequest]; let reconciliations: [Reconciliation] }
    struct Reconciliation: Decodable, Sendable {
        let id: UUID
        let profile: UUID
        let intent: UUID
        let state: String
        let error: String?
        let originalPhase: String?
    }
    struct IntentStatus: Decodable, Sendable {
        struct Record: Decodable, Sendable {
            let serviceEpoch: UUID
            let controller: UUID
            let profile: UUID
            let intent: UUID
            let phase: String
            let error: String?
        }
        let record: Record
        let outcomeUnknown: Bool
        let physicalCleanupPending: Bool
    }
    struct Began: Decodable, Sendable { let token: BrowserProfileMutationToken; let fallback: BrowserProfileLeaseToken? }
    private struct Failure: Decodable, Sendable { let message: String }
    private struct Response<Value: Decodable & Sendable>: Decodable, Sendable {
        let ok: Bool
        let value: Value?
        let error: Failure?
        let outcomeUnknown: Bool?
    }
    static func call<Value: Decodable & Sendable>(_ command: Command, as type: Value.Type) async throws -> Value {
        let encoder = JSONEncoder(); encoder.keyEncodingStrategy = .convertToSnakeCase
        let data = try encoder.encode(command)
        return try await Task.detached {
            let result = data.withUnsafeBytes { storage in
                zmux_desktop_profile_native(storage.bindMemory(to: UInt8.self).baseAddress, storage.count)
            }
            guard result.status == 0 else {
                throw BrowserProfileNativeFailure(message: "Profile native \(command.operation) transport failed (\(result.status)); original intent \(command.intent?.uuidString ?? command.token?.intent.uuidString ?? "none") retained.", outcomeUnknown: ![-2, -4, -5].contains(result.status))
            }
            do {
                let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
                let response = try decoder.decode(Response<Value>.self, from: NativeBridge.consume(result.value))
                guard response.ok, let value = response.value else {
                    throw BrowserProfileNativeFailure(message: response.error?.message ?? "Profile native \(command.operation) rejected.", outcomeUnknown: response.outcomeUnknown ?? true)
                }
                return value
            } catch let failure as BrowserProfileNativeFailure { throw failure }
            catch { throw BrowserProfileNativeFailure(message: "Profile native \(command.operation) response is unknown for original intent \(command.intent?.uuidString ?? command.token?.intent.uuidString ?? "none"): \(error.localizedDescription)", outcomeUnknown: true) }
        }.value
    }
    static func advance(_ token: BrowserProfileMutationToken, _ phase: String) async throws {
        _ = try await call(Command(operation: "advance", token: token, phase: phase), as: Empty.self)
    }
    static func finish(_ token: BrowserProfileMutationToken, _ outcome: Outcome) async throws {
        _ = try await call(Command(operation: "finish", token: token, result: outcome), as: Empty.self)
    }
}

/// Every actual persistent view construction owns an initial or reuse admission
/// permit until its real WKWebView has been registered. Jobs and uncertain permits
/// remain charged through shutdown; no cancellation drops physical authority.
@MainActor
final class BrowserProfileStores {
    struct ConstructionOwner: Sendable, Hashable {
        let namespace: UUID
        let browserEpoch: UUID
        let surface: UInt64
        let profile: UUID
    }
    struct Construction {
        let owner: ConstructionOwner
        let store: WKWebsiteDataStore
        let token: BrowserProfileLeaseToken
        let reuse: BrowserProfileConstructionPermit?
    }
    private final class Entry {
        let store: WKWebsiteDataStore
        let token: BrowserProfileLeaseToken
        var ready = false
        init(store: WKWebsiteDataStore, token: BrowserProfileLeaseToken) { self.store = store; self.token = token }
    }
    private final class WeakView {
        weak var view: WKWebView?
        let profile: UUID
        init(_ view: WKWebView, profile: UUID) { self.view = view; self.profile = profile }
    }
    private enum RemovalResult { case succeeded; case failed(String) }
    private final class Job {
        let request: BrowserProfileMutationRequest
        var store: WKWebsiteDataStore?
        var stores: [BrowserProfileLeaseToken]
        var token: BrowserProfileMutationToken?
        var task: Task<Void, Never>?
        var running = true
        var reconciling = false
        var cleared = false
        var callerRetired = false
        var removal: RemovalResult?
        init(request: BrowserProfileMutationRequest, store: WKWebsiteDataStore?, stores: [BrowserProfileLeaseToken]) {
            self.request = request; self.store = store; self.stores = stores
        }
    }
    private let bridge: NativeBridge
    private var entries: [UUID: Entry] = [:]
    private var retainedTokens: [UUID: [BrowserProfileLeaseToken]] = [:]
    private var pending: [UInt64: (ConstructionOwner, UUID)] = [:]
    private var prepared: [UInt64: Construction] = [:]
    private var failures: [UInt64: ConstructionOwner] = [:]
    private var views: [WeakView] = []
    private var jobs: [UUID: Job] = [:]
    private var recoveryTasks: [UUID: Task<Void, Never>] = [:]
    private var dispatchUnknown = false
    private var acknowledgments = 0
    init(bridge: NativeBridge) { self.bridge = bridge }

    func store(for profile: UUID, surface: UInt64, namespace: UUID, browserEpoch: UUID, isNeeded: @escaping @MainActor () -> Bool,
               refresh: @escaping @MainActor () -> Void,
               reportError: @escaping @MainActor (String) -> Void) -> Construction? {
        let owner = ConstructionOwner(namespace: namespace, browserEpoch: browserEpoch, surface: surface, profile: profile)
        guard !jobs.values.contains(where: { $0.request.profile == profile }) else { return nil }
        if let construction = prepared.removeValue(forKey: surface) {
            if construction.owner == owner { return construction }
            releaseUnused(construction, refresh: refresh, reportError: reportError)
            return nil
        }
        guard pending[surface] == nil, failures[surface] != owner, pending.count + prepared.count + failures.count < 32 else { return nil }
        if entries[profile]?.ready == false || (entries[profile] == nil && pending.values.contains(where: { $0.0.profile == profile })) { return nil }
        let request = UUID()
        let existing = entries[profile]?.token ?? retainedTokens[profile]?.first
        pending[surface] = (owner, request)
        Task { [self] in
            do {
                let token: BrowserProfileLeaseToken
                let reuse: BrowserProfileConstructionPermit?
                if let existing {
                    token = existing
                    reuse = try await bridge.reuseBrowserProfileLease(existing, construction: request)
                } else {
                    token = try await bridge.acquireBrowserProfileLease(profile: profile, store: request)
                    reuse = nil
                }
                guard pending[surface]?.0 == owner, pending[surface]?.1 == request, isNeeded() else {
                    if let reuse { try await bridge.completeBrowserProfileConstruction(reuse) }
                    else { try await bridge.releaseUnconstructedBrowserProfileLease(token) }
                    pending.removeValue(forKey: surface)
                    refresh()
                    return
                }
                // No suspension between the exact need check and native store creation.
                if entries[profile] == nil {
                    entries[profile] = Entry(store: WKWebsiteDataStore(forIdentifier: profile), token: token)
                }
                guard let entry = entries[profile] else { return }
                prepared[surface] = Construction(owner: owner, store: entry.store, token: token, reuse: reuse)
                pending.removeValue(forKey: surface)
                refresh()
            } catch {
                // Keep uncertain accepted acquisition/permit charged, including
                // after cancellation or shutdown. Never reacquire this request.
                failures[surface] = owner
                if let failure = error as? BrowserProfileNativeFailure, !failure.outcomeUnknown { pending.removeValue(forKey: surface) }
                reportError("Profile construction \(profile)/surface \(surface)/request \(request): \(error.localizedDescription)")
            }
        }
        return nil
    }
    func constructed(_ construction: Construction, profile: UUID, view: WKWebView,
                     refresh: @escaping @MainActor () -> Void,
                     reportError: @escaping @MainActor (String) -> Void) {
        views.removeAll { $0.view == nil }
        views.append(WeakView(view, profile: profile))
        acknowledgments += 1
        Task { [self] in
            do {
                if let reuse = construction.reuse { try await bridge.completeBrowserProfileConstruction(reuse) }
                else { try await bridge.acknowledgeBrowserProfileConstruction(construction.token) }
                entries[profile]?.ready = true
                acknowledgments -= 1
                refresh()
            } catch {
                reportError("Profile construction acknowledgment \(profile)/lease \(construction.token.id) is unknown: \(error.localizedDescription)")
            }
        }
    }
    func poll(downloadsBusy: @MainActor (UUID) -> Bool, modelBusy: @MainActor (UUID) -> Bool,
              reportError: @escaping @MainActor (String) -> Void) async throws {
        let controller = await bridge.profileController()
        let batch: BrowserProfileNativeClient.Batch
        do { batch = try await BrowserProfileNativeClient.call(.init(operation: "drain", controller: controller), as: BrowserProfileNativeClient.Batch.self) }
        catch {
            if (error as? BrowserProfileNativeFailure)?.outcomeUnknown != false { dispatchUnknown = true }
            throw error
        }
        dispatchReconciliations(batch.reconciliations, controller: controller, reportError: reportError)
        for request in batch.requests {
            guard request.controller == controller, jobs[request.intent] == nil, jobs.count < 8 else {
                dispatchUnknown = true
                throw BridgeFailure(message: "Native profile dispatch capacity/identity mismatch; original requests retained.")
            }
            views.removeAll { $0.view == nil }
            if modelBusy(request.profile) || downloadsBusy(request.profile) || views.contains(where: { $0.profile == request.profile }) || pending.values.contains(where: { $0.0.profile == request.profile }) || prepared.values.contains(where: { $0.token.profile == request.profile }) || entries[request.profile]?.ready == false {
                _ = try await BrowserProfileNativeClient.call(.init(operation: "reject", controller: controller, intent: request.intent, message: "Browser profile is busy in a view, constructor, download or conversion; close it explicitly before retrying."), as: BrowserProfileNativeClient.Empty.self)
                continue
            }
            let job = reserveJob(request)
            jobs[request.intent] = job
            // The exact job is reserved before any await/authorization. This
            // effect task is created once; status and late callbacks never create another.
            job.task = Task { [self, job] in await run(job, reportError: reportError) }
        }
    }
    private func reserveJob(_ request: BrowserProfileMutationRequest) -> Job {
        // Return before the effect task can run. No caller-side cache-entry
        // reference may survive into the clear/remove sequence.
        let tokens = entries[request.profile].map { [$0.token] } ?? retainedTokens[request.profile] ?? []
        return Job(request: request, store: entries[request.profile]?.store, stores: tokens)
    }
    func pollReconciliations(reportError: @escaping @MainActor (String) -> Void) async throws {
        let controller = await bridge.profileController()
        let batch = try await BrowserProfileNativeClient.call(.init(operation: "drain_reconciliations", controller: controller), as: BrowserProfileNativeClient.RecoveryBatch.self)
        dispatchReconciliations(batch.reconciliations, controller: controller, reportError: reportError)
    }
    private func dispatchReconciliations(_ receipts: [BrowserProfileNativeClient.Reconciliation], controller: UUID, reportError: @escaping @MainActor (String) -> Void) {
        for receipt in receipts {
            if recoveryTasks[receipt.id] != nil { continue }
            guard recoveryTasks.count < 8 else { reportError("Profile reconciliation capacity reached; original receipt retained."); return }
            recoveryTasks[receipt.id] = Task { [self] in
                defer { recoveryTasks.removeValue(forKey: receipt.id) }
                await reconcile(receipt, controller: controller, reportError: reportError)
            }
        }
    }
    private func run(_ job: Job, reportError: @escaping @MainActor (String) -> Void) async {
        defer { job.running = false }
        do {
            let began = try await BrowserProfileNativeClient.call(.init(operation: "begin", controller: job.request.controller, intent: job.request.intent, stores: job.stores), as: BrowserProfileNativeClient.Began.self)
            guard began.token.controller == job.request.controller, began.token.intent == job.request.intent, began.token.profile == job.request.profile else { throw BrowserProfileNativeFailure(message: "Mismatched native profile writer token", outcomeUnknown: true) }
            job.token = began.token
            if let fallback = began.fallback { job.stores.append(fallback) }
            if job.store == nil { job.store = WKWebsiteDataStore(forIdentifier: job.request.profile) }
            try await BrowserProfileNativeClient.advance(began.token, "clear_dispatched")
            try await clearExactStore(job)
            job.cleared = true
            try await BrowserProfileNativeClient.advance(began.token, "clear_completed")
            retireCallerStore(job)
            job.callerRetired = true
            try await BrowserProfileNativeClient.advance(began.token, "remove_dispatched")
            let removalError = await removeUUID(job.request.profile)
            if let removalError {
                job.removal = .failed(removalError)
                try await BrowserProfileNativeClient.finish(began.token, .init(outcome: "removal_failed", message: removalError))
                retainedTokens[job.request.profile] = job.stores
            } else {
                job.removal = .succeeded
                try await BrowserProfileNativeClient.advance(began.token, "remove_completed")
                try await BrowserProfileNativeClient.finish(began.token, .init(outcome: "completed"))
                retainedTokens.removeValue(forKey: job.request.profile)
            }
            jobs.removeValue(forKey: job.request.intent)
        } catch {
            if job.token == nil, let failure = error as? BrowserProfileNativeFailure, !failure.outcomeUnknown {
                do {
                    _ = try await BrowserProfileNativeClient.call(.init(operation: "reject", controller: job.request.controller, intent: job.request.intent, message: String(failure.message.prefix(512))), as: BrowserProfileNativeClient.Empty.self)
                    jobs.removeValue(forKey: job.request.intent)
                    reportError("Profile \(job.request.kind) was refused before native dispatch: \(failure.message)")
                    return
                } catch { reportError("Profile preflight acknowledgment unknown: \(error.localizedDescription)") }
            }
            reportError("Profile \(job.request.kind) intent \(job.request.intent), UUID \(job.request.profile) requires status/recovery; no native effect was replayed: \(error.localizedDescription)")
        }
    }
    /// Explicit original-intent reconciliation. The only callable effects here
    /// are receipt acknowledgment, catalog commit and physical lock cleanup.
    /// It never creates a store/view or calls WebKit clear/UUID removal.
    private func reconcile(_ receipt: BrowserProfileNativeClient.Reconciliation, controller: UUID,
                           reportError: @escaping @MainActor (String) -> Void) async {
        do {
            if let job = jobs[receipt.intent] {
                guard job.request.profile == receipt.profile, job.request.controller == controller,
                      let token = job.token, !job.running, !job.reconciling,
                      job.cleared, job.callerRetired, job.store == nil, let removal = job.removal else {
                    throw BridgeFailure(message: "Original native callback/caller-retirement proof is unavailable; reconciliation is unsupported and quarantine remains.")
                }
                job.reconciling = true
                defer { job.reconciling = false }
                let outcome: BrowserProfileNativeClient.Outcome
                switch removal {
                case .succeeded:
                    let status = try await BrowserProfileNativeClient.call(.init(operation: "status", controller: controller, intent: receipt.intent, profile: receipt.profile), as: BrowserProfileNativeClient.IntentStatus.self)
                    if status.record.phase == "remove_dispatched" {
                        // Re-acknowledge this job's captured original callback.
                        // This is not another native removal authorization.
                        try await BrowserProfileNativeClient.advance(token, "remove_completed")
                    }
                    outcome = .init(outcome: "completed")
                case .failed(let message): outcome = .init(outcome: "removal_failed", message: message)
                }
                let status = try await BrowserProfileNativeClient.call(.init(operation: "reconcile", token: token, result: outcome), as: BrowserProfileNativeClient.IntentStatus.self)
                guard status.record.serviceEpoch == token.serviceEpoch, status.record.controller == token.controller,
                      status.record.profile == token.profile, status.record.intent == token.intent,
                      ["completed", "failed_partial"].contains(status.record.phase),
                      !status.outcomeUnknown, !status.physicalCleanupPending else {
                    throw BridgeFailure(message: "Original profile terminal/physical cleanup proof is unavailable; job remains retained.")
                }
                if status.record.phase == "failed_partial" { retainedTokens[receipt.profile] = job.stores }
                else { retainedTokens.removeValue(forKey: receipt.profile) }
                jobs.removeValue(forKey: receipt.intent)
            } else {
                var status = try await BrowserProfileNativeClient.call(.init(operation: "status", controller: controller, intent: receipt.intent, profile: receipt.profile), as: BrowserProfileNativeClient.IntentStatus.self)
                if ["completed", "failed_partial", "failed_before_clear"].contains(status.record.phase), !status.outcomeUnknown, status.physicalCleanupPending {
                    status = try await BrowserProfileNativeClient.call(.init(operation: "recover_terminal", controller: controller, intent: receipt.intent, profile: receipt.profile), as: BrowserProfileNativeClient.IntentStatus.self)
                }
                guard ["completed", "failed_partial", "failed_before_clear"].contains(status.record.phase), !status.outcomeUnknown, !status.physicalCleanupPending else {
                    throw BridgeFailure(message: "No original native job/callback evidence is available; unresolved restart reconciliation is unsupported and quarantine remains.")
                }
            }
            _ = try await BrowserProfileNativeClient.call(.init(operation: "reconciled", controller: controller, id: receipt.id), as: BrowserProfileNativeClient.Empty.self)
        } catch {
            do { _ = try await BrowserProfileNativeClient.call(.init(operation: "reconciled", controller: controller, id: receipt.id, error: String(error.localizedDescription.prefix(512))), as: BrowserProfileNativeClient.Empty.self) }
            catch { reportError("Profile reconciliation receipt \(receipt.id) remains unknown: \(error.localizedDescription)") }
            reportError("Original profile reconciliation \(receipt.intent) did not replay native effects: \(error.localizedDescription)")
        }
    }
    private func clearExactStore(_ job: Job) async throws {
        guard let store = job.store else { throw BrowserProfileNativeFailure(message: "Exact destructive store is absent; clear callback is unknown.", outcomeUnknown: true) }
        let _: Void = await withCheckedContinuation { continuation in
            store.removeData(ofTypes: WKWebsiteDataStore.allWebsiteDataTypes(), modifiedSince: .distantPast) { continuation.resume() }
        }
        // Returning releases this helper's exact caller reference before removal.
    }
    private func retireCallerStore(_ job: Job) {
        entries.removeValue(forKey: job.request.profile)
        job.store = nil
    }
    private func removeUUID(_ profile: UUID) async -> String? {
        await withCheckedContinuation { continuation in
            WKWebsiteDataStore.remove(forIdentifier: profile) { error in
                continuation.resume(returning: error.map { String($0.localizedDescription.prefix(512)) })
            }
        }
    }
    func cancelUnused(activeOwners: Set<ConstructionOwner>, refresh: @escaping @MainActor () -> Void = {}, reportError: @escaping @MainActor (String) -> Void) {
        failures = failures.filter { activeOwners.contains($0.value) }
        for surface in Array(prepared.keys) {
            guard let construction = prepared[surface], !activeOwners.contains(construction.owner) else { continue }
            prepared.removeValue(forKey: surface)
            releaseUnused(construction, refresh: refresh, reportError: reportError)
        }
    }
    private func releaseUnused(_ construction: Construction, refresh: @escaping @MainActor () -> Void, reportError: @escaping @MainActor (String) -> Void) {
        acknowledgments += 1
        Task { [self] in
            do {
                if let reuse = construction.reuse { try await bridge.completeBrowserProfileConstruction(reuse) }
                else { try await bridge.acknowledgeBrowserProfileConstruction(construction.token) }
                entries[construction.token.profile]?.ready = true
                acknowledgments -= 1
                refresh()
            } catch { reportError("Unused profile construction permit retained for surface \(construction.owner.surface): \(error.localizedDescription)") }
        }
    }
    var cleanupComplete: Bool { jobs.isEmpty && recoveryTasks.isEmpty && pending.isEmpty && prepared.isEmpty && acknowledgments == 0 && !dispatchUnknown }
}

import Foundation

/// Exact provider/run owner used to reject alerts that no longer describe a current row.
struct AgentAlertIdentity: Hashable, Sendable {
    let provider: String
    let identityNamespace: UUID
    let windowId: UInt64
    let windowUuid: UUID
    let workspaceId: UInt64
    let workspaceUuid: UUID
    let paneId: UInt64
    let paneUuid: UUID
    let surfaceId: UInt64
    let surfaceUuid: UUID
    let sessionId: UInt64
    let providerSessionId: UUID
    let runtimeId: UUID
    let generation: UUID
    let promptId: UUID?

    var isAlertable: Bool {
        let zero = UUID(uuid: (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0))
        guard let promptId else { return false }
        return !provider.isEmpty
            && identityNamespace != zero
            && windowId > 0
            && windowUuid != zero
            && workspaceId > 0
            && workspaceUuid != zero
            && paneId > 0
            && paneUuid != zero
            && surfaceId > 0
            && surfaceUuid != zero
            && sessionId > 0
            && providerSessionId != zero
            && runtimeId != zero
            && generation != zero
            && promptId != zero
    }
}

extension AgentAlertIdentity {
    /// Only connected provider evidence can authorize a pending alert in the ring.
    init?(current record: AgentLifecycleRecord) {
        guard record.connection == "connected" else { return nil }
        self.init(provider: record.provider, identityNamespace: record.identityNamespace,
                  windowId: record.windowId, windowUuid: record.windowUuid,
                  workspaceId: record.workspaceId, workspaceUuid: record.workspaceUuid,
                  paneId: record.paneId, paneUuid: record.paneUuid,
                  surfaceId: record.surfaceId, surfaceUuid: record.surfaceUuid,
                  sessionId: record.sessionId, providerSessionId: record.providerSessionId,
                  runtimeId: record.runtimeId, generation: record.generation,
                  promptId: record.promptId)
    }
}

/// One admitted provider event. The adapter-local sequence is not a provider replay cursor.
struct NativeAgentAlertEvent: Decodable, Sendable, Equatable {
    let cursor: UInt64
    let sequence: UInt64
    let provider: String
    let identityNamespace: UUID
    let windowId: UInt64
    let windowUuid: UUID
    let workspaceId: UInt64
    let workspaceUuid: UUID
    let paneId: UInt64
    let paneUuid: UUID
    let surfaceId: UInt64
    let surfaceUuid: UUID
    let sessionId: UInt64
    let providerSessionId: UUID
    let runtimeId: UUID
    let generation: UUID
    let promptId: UUID
    let kind: String

    var identity: AgentAlertIdentity {
        AgentAlertIdentity(provider: provider, identityNamespace: identityNamespace,
                           windowId: windowId, windowUuid: windowUuid,
                           workspaceId: workspaceId, workspaceUuid: workspaceUuid,
                           paneId: paneId, paneUuid: paneUuid,
                           surfaceId: surfaceId, surfaceUuid: surfaceUuid,
                           sessionId: sessionId, providerSessionId: providerSessionId,
                           runtimeId: runtimeId, generation: generation, promptId: promptId)
    }

    var supportedKind: AgentAlertKind? { AgentAlertKind(rawValue: kind) }
}

/// Bounded event history carried by the Rust-owned desktop snapshot.
struct NativeAgentAlertSnapshot: Sendable {
    let epoch: UUID
    let cursor: UInt64
    let droppedThroughCursor: UInt64
    let events: [NativeAgentAlertEvent]
}

private struct AgentAlertEventKey: Hashable {
    let identity: AgentAlertIdentity
    let sequence: UInt64
    let kind: String
}

enum AgentAlertSnapshotMode: Equatable, Sendable {
    /// Advance from the last observed cursor and play only newly admitted events.
    case live
    /// Reconcile the current snapshot without replaying events that predate recovery.
    case reconcile
}

/// Detects an authenticated transport generation change independently of provider evidence freshness.
struct AgentAlertConnectionTracker {
    private struct ProviderOwner: Hashable {
        let provider: String
        let identityNamespace: UUID
        let surfaceId: UInt64
        let surfaceUuid: UUID
        let sessionId: UInt64
        let providerSessionId: UUID

        init(_ record: AgentLifecycleRecord) {
            provider = record.provider
            identityNamespace = record.identityNamespace
            surfaceId = record.surfaceId
            surfaceUuid = record.surfaceUuid
            sessionId = record.sessionId
            providerSessionId = record.providerSessionId
        }
    }

    private static let rememberedOwnerLimit = 128
    private var transportEpochs: [ProviderOwner: UUID] = [:]
    private var ownerOrder: [ProviderOwner] = []

    /// Returns true only when an observed provider transport epoch changes.
    mutating func observe(_ records: [AgentLifecycleRecord]) -> Bool {
        var reconnected = false
        for record in records {
            guard let transportEpoch = record.transportEpoch else { continue }
            let owner = ProviderOwner(record)
            if let previous = transportEpochs[owner] {
                if previous != transportEpoch { reconnected = true }
            } else {
                if ownerOrder.count == Self.rememberedOwnerLimit {
                    let evicted = ownerOrder.removeFirst()
                    transportEpochs.removeValue(forKey: evicted)
                }
                ownerOrder.append(owner)
            }
            transportEpochs[owner] = transportEpoch
        }
        return reconnected
    }
}

/// Cursor-based dedupe for repeated snapshots and reconnects. Event identity is also matched
/// against the current exact provider/run owner before it can reach the local sound player.
struct AgentAlertEventGate {
    private static let rememberedEventLimit = 512
    private(set) var epoch: UUID?
    private(set) var cursor: UInt64 = 0
    private(set) var droppedEventsSinceLastUpdate = false
    private(set) var lastUpdateAccepted = false
    private var rememberedEvents: [AgentAlertEventKey] = []
    private var rememberedEventSet: Set<AgentAlertEventKey> = []

    mutating func update(
        _ snapshot: NativeAgentAlertSnapshot,
        current: Set<AgentAlertIdentity>,
        mode: AgentAlertSnapshotMode = .live
    ) -> [NativeAgentAlertEvent] {
        lastUpdateAccepted = false
        droppedEventsSinceLastUpdate = epoch == snapshot.epoch && snapshot.droppedThroughCursor > cursor
        if epoch == snapshot.epoch && snapshot.cursor < cursor {
            // A cursor rollback within one epoch is stale state, not a new lifecycle.
            return []
        }
        guard snapshot.events.count <= 128 else {
            baseline(to: snapshot, resetIdentityCache: epoch != snapshot.epoch)
            lastUpdateAccepted = true
            return []
        }
        let eventCursors = Set(snapshot.events.map(\.cursor))
        guard snapshot.droppedThroughCursor <= snapshot.cursor,
              eventCursors.count == snapshot.events.count,
              snapshot.events.allSatisfy({ $0.cursor > 0 && $0.cursor <= snapshot.cursor && $0.sequence > 0 }) else {
            // Malformed or oversized event history is never a sound source. Reconcile to its
            // declared head when possible so a later snapshot cannot replay the same payload.
            baseline(to: snapshot, resetIdentityCache: epoch != snapshot.epoch)
            lastUpdateAccepted = true
            return []
        }

        if epoch != snapshot.epoch {
            baseline(to: snapshot, resetIdentityCache: true)
            lastUpdateAccepted = true
            return []
        }
        if mode == .reconcile {
            baseline(to: snapshot, resetIdentityCache: false)
            lastUpdateAccepted = true
            return []
        }

        let start = max(cursor, snapshot.droppedThroughCursor)
        var admitted: [NativeAgentAlertEvent] = []
        for event in snapshot.events.sorted(by: { $0.cursor < $1.cursor }) where event.cursor > start {
            let key = AgentAlertEventKey(identity: event.identity, sequence: event.sequence, kind: event.kind)
            guard remember(key) else { continue }
            guard event.supportedKind != nil, event.identity.isAlertable,
                  current.contains(event.identity) else { continue }
            admitted.append(event)
        }
        // Every snapshot acknowledges its head, including any dropped or stale events.
        epoch = snapshot.epoch
        cursor = snapshot.cursor
        lastUpdateAccepted = true
        return admitted
    }

    private mutating func baseline(to snapshot: NativeAgentAlertSnapshot, resetIdentityCache: Bool) {
        if resetIdentityCache {
            rememberedEvents.removeAll(keepingCapacity: true)
            rememberedEventSet.removeAll(keepingCapacity: true)
        }
        for event in snapshot.events.prefix(128) {
            remember(AgentAlertEventKey(identity: event.identity, sequence: event.sequence, kind: event.kind))
        }
        epoch = snapshot.epoch
        cursor = snapshot.cursor
    }

    private mutating func remember(_ event: AgentAlertEventKey) -> Bool {
        guard !rememberedEventSet.contains(event) else { return false }
        if rememberedEvents.count == Self.rememberedEventLimit {
            let evicted = rememberedEvents.removeFirst()
            rememberedEventSet.remove(evicted)
        }
        rememberedEvents.append(event)
        rememberedEventSet.insert(event)
        return true
    }
}

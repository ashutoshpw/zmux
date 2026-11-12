import Foundation

enum ProgramAlertSnapshotMode: Sendable {
    case live
    case reconcile
}

extension ProgramStatusAlertKind {
    var localSoundKind: AgentAlertKind {
        switch self {
        case .needsUser: .needsUser
        case .completed: .completed
        }
    }
}

struct ProgramAlertGateUpdate: Sendable {
    let events: [ProgramStatusAlertEvent]
    let accepted: Bool
    let gapDetected: Bool
}

/// Cursor and recovery policy for Rust-admitted OSC alerts. This gate never
/// infers alert eligibility from provider state or raw protocol transitions.
struct ProgramAlertEventGate {
    private struct OwnerCheckpoint: Equatable {
        let sessionEpoch: UInt64
        let commandEpoch: UInt64
        let availability: ProgramStatusAvailability
    }

    private(set) var epoch: UUID?
    private(set) var cursor: UInt64 = 0
    private var ownerCheckpoints: [ProgramStatusTerminalIdentity: OwnerCheckpoint] = [:]
    private(set) var lastUpdateAccepted = true
    private(set) var gapDetected = false
    static let maximumEventCount = 128
    static let maximumTerminalCount = 64

    mutating func update(_ snapshot: ProgramStatusAlertSnapshot,
                         current terminals: [ProgramStatusTerminal],
                         mode: ProgramAlertSnapshotMode = .live) -> ProgramAlertGateUpdate {
        lastUpdateAccepted = true
        gapDetected = false
        guard terminals.count <= Self.maximumTerminalCount else {
            // Do not retain checkpoints for an oversized owner set. The
            // coordinator will reconcile from a later bounded snapshot.
            reset()
            lastUpdateAccepted = false
            gapDetected = true
            return ProgramAlertGateUpdate(events: [], accepted: false, gapDetected: true)
        }
        guard snapshot.events.count <= Self.maximumEventCount,
              Self.hasValidEventCursors(snapshot) else {
            baseline(to: snapshot, terminals: terminals)
            lastUpdateAccepted = false
            gapDetected = true
            return ProgramAlertGateUpdate(events: [], accepted: false, gapDetected: true)
        }

        if epoch != snapshot.epoch {
            baseline(to: snapshot, terminals: terminals)
            return ProgramAlertGateUpdate(events: [], accepted: true, gapDetected: false)
        }
        guard snapshot.cursor >= cursor else {
            lastUpdateAccepted = false
            return ProgramAlertGateUpdate(events: [], accepted: false, gapDetected: false)
        }

        if case .reconcile = mode {
            baseline(to: snapshot, terminals: terminals)
            return ProgramAlertGateUpdate(events: [], accepted: true, gapDetected: false)
        }

        if let droppedThrough = snapshot.droppedThroughCursor, droppedThrough > cursor {
            baseline(to: snapshot, terminals: terminals)
            gapDetected = true
            return ProgramAlertGateUpdate(events: [], accepted: true, gapDetected: true)
        }

        let currentByOwner = Dictionary(terminals.map { ($0.identity, $0) }, uniquingKeysWith: { first, _ in first })
        let recoveredOwners = Set(terminals.compactMap { terminal -> ProgramStatusTerminalIdentity? in
            guard let previous = ownerCheckpoints[terminal.identity],
                  previous.availability == .unavailable,
                  terminal.availability == .available else { return nil }
            return terminal.identity
        })
        var admitted: [ProgramStatusAlertEvent] = []
        var seenCursors = Set<UInt64>()
        for event in snapshot.events.sorted(by: { $0.cursor < $1.cursor }) where event.cursor > cursor && event.cursor <= snapshot.cursor {
            guard seenCursors.insert(event.cursor).inserted,
                  !recoveredOwners.contains(event.owner),
                  let terminal = currentByOwner[event.owner],
                  terminal.source == .osc7501,
                  terminal.availability == .available,
                  terminal.sessionEpoch == event.sessionEpoch,
                  event.commandEpoch <= terminal.commandEpoch else { continue }
            let previousCommandEpoch = ownerCheckpoints[event.owner].flatMap { checkpoint in
                checkpoint.sessionEpoch == event.sessionEpoch ? checkpoint.commandEpoch : nil
            }
            guard previousCommandEpoch.map({ event.commandEpoch >= $0 }) ?? true else { continue }
            // `record_id` is event presentation, not a liveness check. A
            // same-read done -> clear still owns its admitted completion event.
            // Rust admits alerts at the transition epoch, so the terminal may
            // advance to a prompt before AppKit polls. Bound that delayed event
            // by the current epoch and the prior same-session checkpoint.
            admitted.append(event)
        }
        cursor = snapshot.cursor
        checkpoint(terminals)
        return ProgramAlertGateUpdate(events: admitted, accepted: true, gapDetected: false)
    }

    mutating func reset() {
        epoch = nil
        cursor = 0
        ownerCheckpoints.removeAll(keepingCapacity: false)
        lastUpdateAccepted = true
        gapDetected = false
    }

    private mutating func baseline(to snapshot: ProgramStatusAlertSnapshot,
                                   terminals: [ProgramStatusTerminal]) {
        epoch = snapshot.epoch
        cursor = snapshot.cursor
        checkpoint(terminals)
    }

    private mutating func checkpoint(_ terminals: [ProgramStatusTerminal]) {
        ownerCheckpoints = Dictionary(terminals.map { terminal in
            (terminal.identity, OwnerCheckpoint(sessionEpoch: terminal.sessionEpoch,
                commandEpoch: terminal.commandEpoch, availability: terminal.availability))
        }, uniquingKeysWith: { _, latest in latest })
    }

    private static func hasValidEventCursors(_ snapshot: ProgramStatusAlertSnapshot) -> Bool {
        guard snapshot.events.allSatisfy({ $0.cursor <= snapshot.cursor }) else { return false }
        let cursors = snapshot.events.map(\.cursor)
        return Set(cursors).count == cursors.count
    }
}

enum ProgramAlertSmoke {
    static func validate() throws {
        let initialTerminal = identity()
        let owner = initialTerminal.identity
        let first = event(cursor: 1, owner: initialTerminal, id: "", kind: .needsUser)
        let epoch = UUID()
        var gate = ProgramAlertEventGate()
        let initial = snapshot(epoch: epoch, cursor: 1, events: [first])
        guard gate.update(initial, current: [initialTerminal], mode: .reconcile).events.isEmpty,
              gate.update(initial, current: [initialTerminal]).events.isEmpty else {
            throw failure("initial or repeated snapshot replayed an OSC program sound")
        }

        let promptAdvancedTerminal = terminal(owner: owner, sessionEpoch: 8, commandEpoch: 4)
        // Rust admitted done before OSC 133 A advanced this same owner. The
        // record is already cleared from the terminal snapshot by this poll.
        let doneThenClearedBeforePrompt = event(cursor: 2, owner: initialTerminal,
                                                id: "build/test", kind: .completed)
        let live = snapshot(epoch: epoch, cursor: 2, events: [first, doneThenClearedBeforePrompt])
        guard gate.update(live, current: [promptAdvancedTerminal]).events == [doneThenClearedBeforePrompt] else {
            throw failure("a same-owner completion admitted before prompt advancement was dropped")
        }
        guard gate.update(live, current: [promptAdvancedTerminal]).events.isEmpty else {
            throw failure("an already consumed OSC event replayed")
        }

        let replacedSession = terminal(owner: owner, sessionEpoch: 9, commandEpoch: 4)
        let oldSessionEvent = event(cursor: 3, owner: initialTerminal, id: "build/test", kind: .needsUser)
        guard gate.update(snapshot(epoch: epoch, cursor: 3, events: [doneThenClearedBeforePrompt, oldSessionEvent]),
                          current: [replacedSession]).events.isEmpty else {
            throw failure("an event from a replaced terminal session reached audio")
        }
        let oldCommandEvent = event(cursor: 4, owner: replacedSession, id: "build/test", kind: .completed,
                                    commandEpoch: 3)
        guard gate.update(snapshot(epoch: epoch, cursor: 4, events: [oldSessionEvent, oldCommandEvent]),
                          current: [replacedSession]).events.isEmpty else {
            throw failure("an event older than the prior same-session checkpoint reached audio")
        }
        let futureCommandEvent = event(cursor: 5, owner: replacedSession, id: "build/test", kind: .completed,
                                       commandEpoch: 5)
        guard gate.update(snapshot(epoch: epoch, cursor: 5, events: [oldCommandEvent, futureCommandEvent]),
                          current: [replacedSession]).events.isEmpty else {
            throw failure("an event from a future command epoch reached audio")
        }
        let wrongOwner = identity()
        let wrongOwnerEvent = event(cursor: 6, owner: wrongOwner, id: "build/test", kind: .needsUser)
        guard gate.update(snapshot(epoch: epoch, cursor: 6, events: [futureCommandEvent, wrongOwnerEvent]),
                          current: [replacedSession]).events.isEmpty else {
            throw failure("an event from a different terminal owner reached audio")
        }

        let unavailable = terminal(owner: owner, sessionEpoch: 9, commandEpoch: 4, availability: .unavailable)
        let unavailableEvent = event(cursor: 7, owner: unavailable, id: "build/test", kind: .needsUser)
        guard gate.update(snapshot(epoch: epoch, cursor: 7, events: [wrongOwnerEvent, unavailableEvent]),
                          current: [unavailable]).events.isEmpty else {
            throw failure("an unavailable terminal replayed an OSC sound")
        }
        let afterRecovery = event(cursor: 8, owner: replacedSession, id: "build/test", kind: .completed)
        guard gate.update(snapshot(epoch: epoch, cursor: 8, events: [unavailableEvent, afterRecovery]),
                          current: [replacedSession]).events.isEmpty else {
            throw failure("a recovered terminal replayed an event from its unavailable interval")
        }
        let freshAfterRecovery = event(cursor: 9, owner: replacedSession, id: "build/test", kind: .needsUser)
        guard gate.update(snapshot(epoch: epoch, cursor: 9, events: [afterRecovery, freshAfterRecovery]),
                          current: [replacedSession]).events == [freshAfterRecovery] else {
            throw failure("a fresh OSC alert after transport recovery was not admitted")
        }

        let gap = ProgramStatusAlertSnapshot(epoch: epoch, cursor: 14, droppedThroughCursor: 13,
                                             events: [event(cursor: 14, owner: replacedSession,
                                                            id: "build/test", kind: .completed)])
        let gapUpdate = gate.update(gap, current: [replacedSession])
        guard gapUpdate.events.isEmpty, gapUpdate.gapDetected, gate.cursor == 14 else {
            throw failure("a dropped alert journal replayed retained events instead of baselining")
        }
        let newEpochEvent = event(cursor: 1, owner: replacedSession, id: "build/test", kind: .completed)
        guard gate.update(snapshot(epoch: UUID(), cursor: 1, events: [newEpochEvent]),
                          current: [replacedSession]).events.isEmpty else {
            throw failure("a new alert feed epoch replayed an existing event")
        }

        let oversizedOwners = Array(repeating: replacedSession, count: ProgramAlertEventGate.maximumTerminalCount + 1)
        let oversized = gate.update(snapshot(epoch: UUID(), cursor: 1, events: []), current: oversizedOwners)
        guard !oversized.accepted, oversized.events.isEmpty, gate.epoch == nil else {
            throw failure("an oversized terminal snapshot was accepted or retained owner checkpoints")
        }
        let boundedBaseline = gate.update(snapshot(epoch: UUID(), cursor: 1, events: [newEpochEvent]),
                                          current: [replacedSession], mode: .reconcile)
        guard boundedBaseline.accepted, boundedBaseline.events.isEmpty else {
            throw failure("a bounded recovery snapshot replayed events after an oversized owner set")
        }

        var eventArrayGate = ProgramAlertEventGate()
        let eventArrayEpoch = UUID()
        _ = eventArrayGate.update(snapshot(epoch: eventArrayEpoch, cursor: 0, events: []),
                                  current: [replacedSession], mode: .reconcile)
        let oversizedEvents = (1...(ProgramAlertEventGate.maximumEventCount + 1)).map { cursor in
            event(cursor: UInt64(cursor), owner: replacedSession, id: "build/item-\(cursor)", kind: .completed)
        }
        let oversizedEventUpdate = eventArrayGate.update(
            snapshot(epoch: eventArrayEpoch, cursor: UInt64(oversizedEvents.count), events: oversizedEvents),
            current: [replacedSession]
        )
        guard !oversizedEventUpdate.accepted, oversizedEventUpdate.events.isEmpty,
              oversizedEventUpdate.gapDetected, eventArrayGate.epoch == eventArrayEpoch,
              eventArrayGate.cursor == UInt64(oversizedEvents.count) else {
            throw failure("an oversized event array was accepted or failed to establish a bounded baseline")
        }
        let recoveringEvent = event(cursor: UInt64(oversizedEvents.count + 1), owner: replacedSession,
                                    id: "build/recovering", kind: .completed)
        let latestOversizedEvent = oversizedEvents[oversizedEvents.count - 1]
        let recovery = eventArrayGate.update(
            snapshot(epoch: eventArrayEpoch, cursor: recoveringEvent.cursor,
                     events: [latestOversizedEvent, recoveringEvent]),
            current: [replacedSession], mode: .reconcile
        )
        guard recovery.accepted, recovery.events.isEmpty else {
            throw failure("a bounded reconciliation replayed events retained from an oversized array")
        }
        let firstLiveAfterRecovery = event(cursor: recoveringEvent.cursor + 1, owner: replacedSession,
                                           id: "build/live", kind: .completed)
        let liveAfterRecovery = eventArrayGate.update(
            snapshot(epoch: eventArrayEpoch, cursor: firstLiveAfterRecovery.cursor,
                     events: [recoveringEvent, firstLiveAfterRecovery]),
            current: [replacedSession]
        )
        guard liveAfterRecovery.accepted, liveAfterRecovery.events == [firstLiveAfterRecovery] else {
            throw failure("a bounded recovery did not resume delivery for a fresh event")
        }
    }

    private static func identity() -> ProgramStatusTerminal {
        let owner = ProgramStatusTerminalIdentity(identityNamespace: UUID(),
            windowId: 7, windowUuid: UUID(), workspaceId: 9, workspaceUuid: UUID(),
            paneId: 11, paneUuid: UUID(), surfaceId: 3, surfaceUuid: UUID(), sessionId: 5)
        return terminal(owner: owner, sessionEpoch: 8, commandEpoch: 3)
    }

    private static func terminal(owner: ProgramStatusTerminalIdentity,
                                 sessionEpoch: UInt64, commandEpoch: UInt64,
                                 availability: ProgramStatusAvailability = .available) -> ProgramStatusTerminal {
        ProgramStatusTerminal(source: .osc7501, availability: availability,
            identityNamespace: owner.identityNamespace, windowId: owner.windowId, windowUuid: owner.windowUuid,
            workspaceId: owner.workspaceId, workspaceUuid: owner.workspaceUuid, paneId: owner.paneId, paneUuid: owner.paneUuid,
            surfaceId: owner.surfaceId, surfaceUuid: owner.surfaceUuid, sessionId: owner.sessionId,
            sessionEpoch: sessionEpoch, commandEpoch: commandEpoch,
            recordCount: 0, recordsComplete: true, omittedRecords: 0, records: [])
    }

    private static func event(cursor: UInt64, owner: ProgramStatusTerminal, id: String,
                              kind: ProgramStatusAlertKind, commandEpoch: UInt64? = nil) -> ProgramStatusAlertEvent {
        ProgramStatusAlertEvent(cursor: cursor,
            identityNamespace: owner.identityNamespace,
            windowId: owner.windowId, windowUuid: owner.windowUuid,
            workspaceId: owner.workspaceId, workspaceUuid: owner.workspaceUuid,
            paneId: owner.paneId, paneUuid: owner.paneUuid,
            surfaceId: owner.surfaceId, surfaceUuid: owner.surfaceUuid,
            sessionId: owner.sessionId, sessionEpoch: owner.sessionEpoch,
            commandEpoch: commandEpoch ?? owner.commandEpoch,
            recordId: id, kind: kind, app: "fixture", title: nil, message: nil)
    }

    private static func snapshot(epoch: UUID, cursor: UInt64,
                                 events: [ProgramStatusAlertEvent]) -> ProgramStatusAlertSnapshot {
        ProgramStatusAlertSnapshot(epoch: epoch, cursor: cursor, droppedThroughCursor: nil, events: events)
    }

    private static func failure(_ message: String) -> BridgeFailure {
        BridgeFailure(message: "Program alert smoke: " + message)
    }
}

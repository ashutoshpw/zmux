import Foundation
import UserNotifications

/// Exercises the native alert preference and snapshot-dedupe owner without requesting
/// notification permission or emitting real audio.
enum AgentAlertSmoke {
    @MainActor
    static func validate() throws {
        try validatePreferences()
        try validateEventDelivery()
        try validateProviderReconnect()
        try validateWireCodec()
    }

    @MainActor
    private static func validatePreferences() throws {
        let suite = "zmux.agent-alert-smoke.\(UUID().uuidString)"
        guard let defaults = UserDefaults(suiteName: suite) else {
            throw failure("could not create an isolated alert-preferences domain")
        }
        defer { defaults.removePersistentDomain(forName: suite) }

        let initial = AgentAlertPreferences(defaults: defaults)
        guard initial.playSounds,
              initial.sound(for: .needsUser) == .defaultSound(for: .needsUser),
              initial.sound(for: .completed) == .defaultSound(for: .completed) else {
            throw failure("new alert preferences did not use the enabled and per-event defaults")
        }
        initial.playSounds = false
        initial.needsUserSound = .pop
        initial.completedSound = .purr

        let reopened = AgentAlertPreferences(defaults: defaults)
        guard !reopened.playSounds,
              reopened.needsUserSound == .pop,
              reopened.completedSound == .purr,
              reopened.sound(for: .needsUser) != reopened.sound(for: .completed) else {
            throw failure("alert preferences did not persist independent event sounds and the master switch")
        }
        let audio = AgentAlertAudioPlayer(preferences: reopened)
        guard audio.playEvent(.needsUser) == .suppressed,
              audio.playEvent(.completed) == .suppressed else {
            throw failure("muted agent events were not suppressed before system audio playback")
        }
    }

    private static func validateEventDelivery() throws {
        let epoch = UUID()
        let first = identity(prompt: UUID())
        let eventOne = event(cursor: 1, sequence: 8, identity: first, kind: "needs_user")
        var gate = AgentAlertEventGate()

        let startup = snapshot(epoch: epoch, cursor: 1, events: [eventOne])
        guard gate.update(startup, current: [first], mode: .reconcile).isEmpty else {
            throw failure("initial snapshot reconciliation replayed an old agent sound")
        }
        guard gate.update(startup, current: [first]).isEmpty else {
            throw failure("repeated snapshot replayed an already reconciled event")
        }

        let liveEvent = event(cursor: 2, sequence: 9, identity: first, kind: "needs_user")
        let live = snapshot(epoch: epoch, cursor: 2, events: [eventOne, liveEvent])
        guard gate.update(live, current: [first]) == [liveEvent] else {
            throw failure("a new, current needs-user event was not admitted exactly once")
        }
        let staleRecoverySnapshot = snapshot(epoch: epoch, cursor: 1, events: [eventOne])
        guard gate.update(staleRecoverySnapshot, current: [first], mode: .reconcile).isEmpty,
              !gate.lastUpdateAccepted,
              gate.cursor == 2 else {
            throw failure("a stale recovery snapshot rolled back the acknowledged event cursor")
        }
        guard gate.update(live, current: [first]).isEmpty else {
            throw failure("repeated live snapshot replayed agent audio")
        }

        let replayedEvent = event(cursor: 3, sequence: 9, identity: first, kind: "needs_user")
        let replayed = snapshot(epoch: epoch, cursor: 3, events: [liveEvent, replayedEvent])
        guard gate.update(replayed, current: [first]).isEmpty else {
            throw failure("a replayed provider event ID with a new ring cursor played twice")
        }

        let nextRun = identity(prompt: UUID(), generation: UUID())
        let staleRunEvent = event(cursor: 4, sequence: 10, identity: first, kind: "completed")
        let staleRunSnapshot = snapshot(epoch: epoch, cursor: 4, events: [replayedEvent, staleRunEvent])
        guard gate.update(staleRunSnapshot, current: [nextRun]).isEmpty else {
            throw failure("an alert from a stale provider run matched the replacement run")
        }

        let currentRunEvent = event(cursor: 5, sequence: 1, identity: nextRun, kind: "completed")
        let currentRunSnapshot = snapshot(epoch: epoch, cursor: 5, events: [staleRunEvent, currentRunEvent])
        guard gate.update(currentRunSnapshot, current: [nextRun]) == [currentRunEvent] else {
            throw failure("a completion for the current run was not admitted")
        }

        let reconnectEvent = event(cursor: 7, sequence: 2, identity: nextRun, kind: "completed")
        let reconnect = snapshot(epoch: epoch, cursor: 7, events: [currentRunEvent, reconnectEvent])
        guard gate.update(reconnect, current: [nextRun], mode: .reconcile).isEmpty else {
            throw failure("reconnect reconciliation replayed historical agent audio")
        }
        let futureEvent = event(cursor: 8, sequence: 3, identity: nextRun, kind: "needs_user")
        let afterReconnect = snapshot(epoch: epoch, cursor: 8, events: [reconnectEvent, futureEvent])
        guard gate.update(afterReconnect, current: [nextRun]) == [futureEvent] else {
            throw failure("a new event after reconnect reconciliation was not admitted")
        }

        let retainedEvent = event(cursor: 11, sequence: 4, identity: nextRun, kind: "needs_user")
        let overflow = NativeAgentAlertSnapshot(epoch: epoch, cursor: 11, droppedThroughCursor: 10,
                                                events: [retainedEvent])
        guard gate.update(overflow, current: [nextRun]) == [retainedEvent],
              gate.droppedEventsSinceLastUpdate else {
            throw failure("bounded ring loss was not surfaced while retaining a newer event")
        }

        let unknown = event(cursor: 12, sequence: 5, identity: nextRun, kind: "unsupported")
        let unknownSnapshot = snapshot(epoch: epoch, cursor: 12, events: [retainedEvent, unknown])
        guard gate.update(unknownSnapshot, current: [nextRun]).isEmpty else {
            throw failure("an unsupported provider event was treated as an alert")
        }

        let newEpochEvent = event(cursor: 1, sequence: 1, identity: nextRun, kind: "completed")
        let newEpoch = snapshot(epoch: UUID(), cursor: 1, events: [newEpochEvent])
        guard gate.update(newEpoch, current: [nextRun]).isEmpty else {
            throw failure("a new Rust event epoch replayed preexisting ring entries")
        }
    }

    private static func validateWireCodec() throws {
        let owner = identity(prompt: UUID())
        let eventPayload: [String: Any] = [
            "cursor": 12,
            "sequence": 8,
            "provider": owner.provider,
            "identity_namespace": owner.identityNamespace.uuidString.lowercased(),
            "window_id": owner.windowId,
            "window_uuid": owner.windowUuid.uuidString.lowercased(),
            "workspace_id": owner.workspaceId,
            "workspace_uuid": owner.workspaceUuid.uuidString.lowercased(),
            "pane_id": owner.paneId,
            "pane_uuid": owner.paneUuid.uuidString.lowercased(),
            "surface_id": owner.surfaceId,
            "surface_uuid": owner.surfaceUuid.uuidString.lowercased(),
            "session_id": owner.sessionId,
            "provider_session_id": owner.providerSessionId.uuidString.lowercased(),
            "runtime_id": owner.runtimeId.uuidString.lowercased(),
            "generation": owner.generation.uuidString.lowercased(),
            "prompt_id": owner.promptId?.uuidString.lowercased() ?? "",
            "kind": "needs_user"
        ]
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let eventData = try JSONSerialization.data(withJSONObject: eventPayload)
        let event = try decoder.decode(NativeAgentAlertEvent.self, from: eventData)
        guard event.identity == owner, event.sequence == 8, event.supportedKind == .needsUser else {
            throw failure("flat Rust alert event fields did not decode to the exact provider/run identity")
        }

        let epoch = UUID()
        let requestPayload: [String: Any] = [
            "identifier": "zmux.notification.\(epoch.uuidString.lowercased()).1",
            "target": [
                "epoch": epoch.uuidString.lowercased(),
                "identity_namespace": owner.identityNamespace.uuidString.lowercased(),
                "notification_id": 1,
                "destination": ["kind": "surface", "surface_id": owner.surfaceId, "session_id": owner.sessionId]
            ],
            "authorization_generation": 1,
            "title": "Agent needs you",
            "subtitle": "",
            "body": "Input is needed",
            "reply_enabled": false,
            "agent_alert_kind": "needs_user",
            "alert_sound_source": "agent_lifecycle",
            "alert_sound_kind": "needs_user"
        ]
        let requestData = try JSONSerialization.data(withJSONObject: requestPayload)
        let request = try decoder.decode(NativeNotificationRequest.self, from: requestData)
        guard request.agentAlertKind == "needs_user",
              request.alertSoundSource == .agentLifecycle,
              request.alertSoundKind == .needsUser,
              request.soundRoutingIsValid,
              request.localSoundKind == .needsUser,
              request.suppressesSystemSound,
              !NotificationDelivery.presentationOptions(for: request).contains(.sound) else {
            throw failure("Rust lifecycle notification omitted its silent-audio discriminator")
        }
        var oscPayload = requestPayload
        oscPayload.removeValue(forKey: "agent_alert_kind")
        oscPayload["alert_sound_source"] = "osc7501"
        oscPayload["alert_sound_kind"] = "needs_user"
        let oscData = try JSONSerialization.data(withJSONObject: oscPayload)
        let oscRequest = try decoder.decode(NativeNotificationRequest.self, from: oscData)
        guard oscRequest.alertSoundSource == .osc7501,
              oscRequest.alertSoundKind == .needsUser,
              oscRequest.soundRoutingIsValid,
              oscRequest.localSoundKind == .needsUser,
              oscRequest.suppressesSystemSound,
              !NotificationDelivery.presentationOptions(for: oscRequest).contains(.sound) else {
            throw failure("OSC-origin notification did not select local needs-user audio and suppress OS sound")
        }
        var legacyPayload = requestPayload
        legacyPayload.removeValue(forKey: "agent_alert_kind")
        legacyPayload.removeValue(forKey: "alert_sound_source")
        legacyPayload.removeValue(forKey: "alert_sound_kind")
        let legacyData = try JSONSerialization.data(withJSONObject: legacyPayload)
        let legacy = try decoder.decode(NativeNotificationRequest.self, from: legacyData)
        guard legacy.agentAlertKind == nil,
              legacy.alertSoundSource == nil,
              legacy.alertSoundKind == nil,
              legacy.soundRoutingIsValid,
              !legacy.suppressesSystemSound,
              NotificationDelivery.presentationOptions(for: legacy).contains(.sound) else {
            throw failure("legacy terminal notifications did not retain their default system sound behavior")
        }

        for invalid in [
            ["alert_sound_source": "osc7501"],
            ["alert_sound_kind": "needs_user"],
            ["alert_sound_source": "osc7501", "alert_sound_kind": "needs_user", "agent_alert_kind": "needs_user"]
        ] {
            var payload = legacyPayload
            for (key, value) in invalid { payload[key] = value }
            let malformedData = try JSONSerialization.data(withJSONObject: payload)
            guard let malformed = try? decoder.decode(NativeNotificationRequest.self, from: malformedData),
                  !malformed.soundRoutingIsValid else {
                throw failure("native notification accepted an incomplete or conflicting local-sound source tag")
            }
        }
    }

    private static func validateProviderReconnect() throws {
        let owner = identity(prompt: UUID(), provider: "claude_remote")
        let transportEpoch = UUID()
        var tracker = AgentAlertConnectionTracker()
        let connected = record(owner, connection: "connected", transportEpoch: transportEpoch)
        guard !tracker.observe([connected]) else {
            throw failure("the first observed stream epoch was treated as a reconnect")
        }

        let epoch = UUID()
        var gate = AgentAlertEventGate()
        guard gate.update(snapshot(epoch: epoch, cursor: 0, events: []), current: [owner],
                          mode: .reconcile).isEmpty else {
            throw failure("transport interleaving fixture did not establish an initial cursor")
        }
        let staleEvidence = record(owner, connection: "unknown", transportEpoch: transportEpoch)
        guard !tracker.observe([staleEvidence]),
              gate.update(snapshot(epoch: epoch, cursor: 0, events: []), current: [], mode: .live).isEmpty,
              !tracker.observe([connected]) else {
            throw failure("fresh provider evidence was mistaken for a transport reconnection")
        }

        let freshAfterExpiry = event(cursor: 1, sequence: 10, identity: owner, kind: "needs_user")
        guard gate.update(snapshot(epoch: epoch, cursor: 1, events: [freshAfterExpiry]),
                          current: [owner], mode: .live) == [freshAfterExpiry] else {
            throw failure("a new alert after freshness recovery was silently reconciled")
        }

        let oldDuringReconnect = event(cursor: 2, sequence: 11, identity: owner, kind: "completed")
        let reconnectedTransport = UUID()
        let reconnectRecord = record(owner, connection: "unknown", transportEpoch: reconnectedTransport)
        guard tracker.observe([reconnectRecord]) else {
            throw failure("a validated transport epoch change did not request silent reconciliation")
        }
        let reconnectSnapshot = snapshot(epoch: epoch, cursor: 2, events: [freshAfterExpiry, oldDuringReconnect])
        guard gate.update(reconnectSnapshot, current: [], mode: .reconcile).isEmpty else {
            throw failure("a pending event from before transport recovery replayed on reconnect")
        }

        let freshAfterReconnect = event(cursor: 3, sequence: 12, identity: owner, kind: "needs_user")
        let currentRecord = record(owner, connection: "connected", transportEpoch: reconnectedTransport)
        guard !tracker.observe([currentRecord]),
              gate.update(snapshot(epoch: epoch, cursor: 3, events: [oldDuringReconnect, freshAfterReconnect]),
                          current: [owner], mode: .live) == [freshAfterReconnect] else {
            throw failure("a new alert after transport recovery was not admitted")
        }
    }

    private static func identity(prompt: UUID, generation: UUID = UUID(),
                                  provider: String = "claude_local") -> AgentAlertIdentity {
        AgentAlertIdentity(provider: provider, identityNamespace: UUID(),
                           windowId: 7, windowUuid: UUID(), workspaceId: 9, workspaceUuid: UUID(),
                           paneId: 11, paneUuid: UUID(), surfaceId: 3, surfaceUuid: UUID(),
                           sessionId: 5, providerSessionId: UUID(), runtimeId: UUID(),
                           generation: generation, promptId: prompt)
    }

    private static func event(cursor: UInt64, sequence: UInt64, identity: AgentAlertIdentity,
                              kind: String) -> NativeAgentAlertEvent {
        NativeAgentAlertEvent(cursor: cursor, sequence: sequence, provider: identity.provider,
                              identityNamespace: identity.identityNamespace, windowId: identity.windowId,
                              windowUuid: identity.windowUuid, workspaceId: identity.workspaceId,
                              workspaceUuid: identity.workspaceUuid, paneId: identity.paneId,
                              paneUuid: identity.paneUuid, surfaceId: identity.surfaceId,
                              surfaceUuid: identity.surfaceUuid, sessionId: identity.sessionId,
                              providerSessionId: identity.providerSessionId, runtimeId: identity.runtimeId,
                              generation: identity.generation,
                              promptId: identity.promptId ?? UUID(uuid: (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)),
                              kind: kind)
    }

    private static func record(_ identity: AgentAlertIdentity, connection: String,
                               transportEpoch: UUID? = nil) -> AgentLifecycleRecord {
        AgentLifecycleRecord(provider: identity.provider, identityNamespace: identity.identityNamespace,
                             windowId: identity.windowId, windowUuid: identity.windowUuid,
                             workspaceId: identity.workspaceId, workspaceUuid: identity.workspaceUuid,
                             paneId: identity.paneId, paneUuid: identity.paneUuid,
                             surfaceId: identity.surfaceId, surfaceUuid: identity.surfaceUuid,
                             sessionId: identity.sessionId, runtimeId: identity.runtimeId,
                             generation: identity.generation, providerSessionId: identity.providerSessionId,
                             transportEpoch: transportEpoch, promptId: identity.promptId,
                             activity: "waiting", connection: connection)
    }

    private static func snapshot(epoch: UUID, cursor: UInt64,
                                 events: [NativeAgentAlertEvent]) -> NativeAgentAlertSnapshot {
        NativeAgentAlertSnapshot(epoch: epoch, cursor: cursor, droppedThroughCursor: 0, events: events)
    }

    private static func failure(_ message: String) -> BridgeFailure {
        BridgeFailure(message: "Agent alert smoke: " + message)
    }
}

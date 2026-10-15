import Foundation

/// Production permission ordering against a separate Rust runtime. These are protocol
/// observations only: this fixture never requests OS permission or schedules a banner.
enum NotificationPermissionSmoke {
    static func validate() async throws {
        let bridge = NativeBridge()
        try await bridge.start(persistent: false)
        do {
            try await validateOrdering(bridge)
        } catch {
            let primary = error
            do { try await bridge.close() }
            catch {
                throw BridgeFailure(message: "Notification permission smoke: \(primary.localizedDescription); runtime cleanup also failed: \(error.localizedDescription)")
            }
            throw primary
        }
        try await bridge.close()
    }

    private static func validateOrdering(_ bridge: NativeBridge) async throws {
        let initial = try await wait(bridge, context: "start the isolated shell") { snapshot in
            snapshot.frames.contains { $0.snapshot?.state == "running" }
        }
        guard let session = initial.frames.first(where: { $0.snapshot?.state == "running" })?.sessionId,
              initial.nativeNotifications.authorization == .unknown,
              initial.nativeNotifications.authorizationGeneration == 0 else {
            throw failure("isolated runtime did not begin with unknown notification authorization")
        }
        let epoch = initial.nativeNotifications.epoch
        var observations = PermissionObservations()
        observations.updateSnapshot(epoch: epoch, generation: 0)
        observations.observe(epoch: epoch, generation: 1, state: .authorized)
        let initialGrant = try next(observations, generation: 1, state: .authorized)
        try await bridge.notificationEvent(initialGrant.event)
        observations.confirm(epoch: epoch, generation: initialGrant.generation)
        let authorized = try await authorization(bridge, generation: 1, state: .authorized)
        observations.updateSnapshot(epoch: authorized.nativeNotifications.epoch,
                                    generation: authorized.nativeNotifications.authorizationGeneration)
        guard observations.next == nil, !observations.blocksDelivery else {
            throw failure("initial permission observation did not reach a confirmed snapshot")
        }

        let nonce = UUID().uuidString.lowercased()
        let beforeDenial = try await notify(bridge, session: session, title: "Permission-before-denial-\(nonce)")
        guard beforeDenial.authorizationGeneration == 1 else {
            throw failure("the first PTY notification was not bound to the initial permission observation")
        }

        // Both settings observations arrive before the serial sender drains. Rust
        // must see revocation first, even though the most recent observation grants.
        observations.observe(epoch: epoch, generation: 2, state: .denied)
        observations.observe(epoch: epoch, generation: 3, state: .authorized)
        let denial = try next(observations, generation: 2, state: .denied)
        guard observations.blocksDelivery else { throw failure("queued revocation did not pause delivery") }
        try await bridge.notificationEvent(denial.event)
        observations.confirm(epoch: epoch, generation: denial.generation)
        _ = try next(observations, generation: 3, state: .authorized)
        guard observations.blocksDelivery else {
            throw failure("a send acknowledgement released revocation before an authoritative snapshot")
        }
        let denied = try await authorization(bridge, generation: 2, state: .denied)
        try requireWithdrawn(beforeDenial, from: denied, context: "before the queued grant")
        observations.updateSnapshot(epoch: denied.nativeNotifications.epoch,
                                    generation: denied.nativeNotifications.authorizationGeneration)
        let grant = try next(observations, generation: 3, state: .authorized)
        try await bridge.notificationEvent(grant.event)
        observations.confirm(epoch: epoch, generation: grant.generation)
        let granted = try await authorization(bridge, generation: 3, state: .authorized)
        observations.updateSnapshot(epoch: granted.nativeNotifications.epoch,
                                    generation: granted.nativeNotifications.authorizationGeneration)
        try requireWithdrawn(beforeDenial, from: granted, context: "after permission was granted again")
        guard observations.next == nil, !observations.blocksDelivery else {
            throw failure("revocation followed by a grant left the permission queue blocked")
        }

        let beforeLostReply = try await notify(bridge, session: session, title: "Permission-lost-reply-\(nonce)")
        guard beforeLostReply.authorizationGeneration == 3 else {
            throw failure("new PTY notification did not use the restored authorization")
        }
        observations.observe(epoch: epoch, generation: 4, state: .denied)
        observations.observe(epoch: epoch, generation: 5, state: .authorized)
        let lostReply = try next(observations, generation: 4, state: .denied)
        try await bridge.notificationEvent(lostReply.event)
        // Rust applied the real event. Deliberately omit confirm() to represent the
        // adapter losing its successful send reply, then recover from Rust's snapshot.
        _ = try next(observations, generation: 4, state: .denied)
        guard observations.blocksDelivery else { throw failure("lost acknowledgement discarded the pending revocation") }
        let applied = try await authorization(bridge, generation: 4, state: .denied)
        try requireWithdrawn(beforeLostReply, from: applied, context: "after the applied revocation with a lost reply")
        var duplicateRejected = false
        do { try await bridge.notificationEvent(lostReply.event) }
        catch { duplicateRejected = true }
        guard duplicateRejected else { throw failure("Rust unexpectedly accepted a duplicate permission generation") }
        observations.updateSnapshot(epoch: applied.nativeNotifications.epoch,
                                    generation: applied.nativeNotifications.authorizationGeneration)
        let recoveredGrant = try next(observations, generation: 5, state: .authorized)
        try await bridge.notificationEvent(recoveredGrant.event)
        observations.confirm(epoch: epoch, generation: recoveredGrant.generation)
        let recovered = try await authorization(bridge, generation: 5, state: .authorized)
        observations.updateSnapshot(epoch: recovered.nativeNotifications.epoch,
                                    generation: recovered.nativeNotifications.authorizationGeneration)
        try requireWithdrawn(beforeLostReply, from: recovered, context: "after snapshot acknowledgement and the subsequent grant")
        guard observations.next == nil, !observations.blocksDelivery else {
            throw failure("an authoritative snapshot failed to recover a lost send acknowledgement")
        }
        let fresh = try await notify(bridge, session: session, title: "Permission-after-recovery-\(nonce)")
        guard fresh.authorizationGeneration == 5 else {
            throw failure("new notifications did not progress after snapshot acknowledgement")
        }
    }

    private static func next(_ observations: PermissionObservations, generation: UInt64,
                             state: NotificationAuthorization) throws -> PermissionObservations.Observation {
        guard let observation = observations.next,
              observation.generation == generation, observation.state == state else {
            throw failure("expected queued \(state.rawValue) generation \(generation), got \(String(describing: observations.next))")
        }
        return observation
    }

    private static func authorization(_ bridge: NativeBridge, generation: UInt64,
                                      state: NotificationAuthorization) async throws -> DesktopSnapshot {
        try await wait(bridge, context: "observe Rust authorization \(state.rawValue) generation \(generation)") { snapshot in
            snapshot.nativeNotifications.authorization == state
                && snapshot.nativeNotifications.authorizationGeneration == generation
        }
    }

    private static func notify(_ bridge: NativeBridge, session: UInt64,
                               title: String) async throws -> NativeNotificationRequest {
        try await bridge.input(session: session, text: "printf '\\033]777;notify;\(title);Permission ordering fixture\\007'\r")
        let snapshot = try await wait(bridge, context: "receive the real PTY notification \(title)") { snapshot in
            snapshot.nativeNotifications.desired.contains { $0.title == title && $0.target.sessionId == session }
        }
        guard let request = snapshot.nativeNotifications.desired.first(where: { $0.title == title && $0.target.sessionId == session }),
              snapshot.notifications.contains(where: { $0.id == request.target.notificationId && !$0.read }) else {
            throw failure("PTY notification was not unread in the authoritative inbox")
        }
        return request
    }

    private static func requireWithdrawn(_ request: NativeNotificationRequest, from snapshot: DesktopSnapshot,
                                         context: String) throws {
        guard !snapshot.nativeNotifications.desired.contains(where: { $0.target == request.target }),
              snapshot.notifications.contains(where: { $0.id == request.target.notificationId && !$0.read }) else {
            throw failure("the old OS desire was not withdrawn while preserving its unread inbox row \(context)")
        }
    }

    private static func wait(_ bridge: NativeBridge, context: String,
                             predicate: @Sendable (DesktopSnapshot) -> Bool) async throws -> DesktopSnapshot {
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: .seconds(5))
        while clock.now < deadline {
            if let snapshot = try await bridge.poll(), predicate(snapshot) { return snapshot }
            try await Task.sleep(for: .milliseconds(10))
        }
        throw failure("timed out waiting to \(context)")
    }

    private static func failure(_ message: String) -> BridgeFailure {
        BridgeFailure(message: "Notification permission smoke: " + message)
    }
}

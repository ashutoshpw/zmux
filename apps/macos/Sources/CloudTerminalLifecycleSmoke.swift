import Foundation
import Security

private actor CloudTerminalTaskCompletionFlag {
    private(set) var completed = false
    func markComplete() { completed = true }
}

@MainActor
enum CloudTerminalLifecycleSmoke {
    private enum StopPath {
        case explicitDisable
        case credentialChange
    }

    private struct FixtureState: Decodable, Sendable {
        let presenceUpdates: [Bool]
        let presenceAttempts: [PresenceAttempt]
    }

    private struct PresenceAttempt: Codable, Equatable, Sendable {
        let sharing: Bool
        let status: Int
    }

    private struct Scenario: Encodable, Sendable {
        let name: String
        let hostState: String
        let presenceUpdates: [Bool]
        let presenceAttempts: [PresenceAttempt]
        let localGateConfirmedDisabled: Bool
        let postStopGateErrorCode: String
    }

    private struct RelayEvidence: Encodable, Sendable {
        let sameDeviceAfterRepeatedRegistration: Bool
        let listedSessions: Int
        let readExistingFrame: Bool
        let inputMarker: String
        let inputMarkerObserved: Bool
        let resizeRows: Int
        let resizeCols: Int
        let viewportRows: Int
        let viewportCols: Int
        let kernelSizeOutput: String
        let presenceCleared: Bool
        let postStopGateErrorCode: String
    }

    private struct RejectedStartEvidence: Encodable, Sendable {
        let presenceUpdates: [Bool]
        let presenceAttempts: [PresenceAttempt]
        let ownerOffAfterRejection: Bool
        let localGateClosedAfterRejection: Bool
        let retryEnabled: Bool
        let retryStopped: Bool
        let postStopGateErrorCode: String
    }

    private struct AccountSwitchEvidence: Encodable, Sendable {
        let presenceUpdates: [Bool]
        let presenceAttempts: [PresenceAttempt]
        let oldStopWarningReturned: Bool
        let oldPresenceClearConfirmed: Bool
        let oldDeviceStillSharingAfterExpiredSession: Bool
        let oldDeviceStillSharingAfterAccountSwitch: Bool
        let localGateClosedAfter401: Bool
        let oldCleanupContextRetiredWithUnresolvedWarning: Bool
        let sameInstallationAfterAccountSwitch: Bool
        let newAccountDeviceIsDifferent: Bool
        let firstNewAccountEnableRequiredSeparateAction: Bool
        let localGateClosedAfterRejectedEnable: Bool
        let newAccountEnabled: Bool
        let newAccountStopConfirmed: Bool
        let postOldStopGateErrorCode: String
        let postNewStopGateErrorCode: String
    }

    private struct OwnedKeychainItems {
        let queries: [[String: Any]]
    }

    private struct Evidence: Encodable, Sendable {
        let scope = "cloud_terminal_lifecycle_two_peer_access_and_account_switch"
        let state = "passed"
        let source = "CloudClient + CryptoKit + CloudTerminalHost + NativeBridge + Rust owner + existing shell + two loopback origins"
        let scenarios: [Scenario]
        let relay: RelayEvidence
        let rejectedStart: RejectedStartEvidence
        let accountSwitch: AccountSwitchEvidence
    }

    static func run(report: URL, hostEndpoint: CloudEndpoint, viewerEndpoint: CloudEndpoint,
                    bridge: NativeBridge) async throws {
        guard isOwnedLoopbackEndpoint(hostEndpoint), isOwnedLoopbackEndpoint(viewerEndpoint),
              hostEndpoint.origin.absoluteString != viewerEndpoint.origin.absoluteString else {
            throw BridgeFailure(message: "Cloud terminal smoke requires two distinct explicit http://127.0.0.1:<port> origins.")
        }

        // Check every exact-origin credential and identity slot before either client can
        // create a device identity or save an OAuth token.
        let ownedKeychainItems = try preflightKeychainOwnership(endpoints: [hostEndpoint, viewerEndpoint])
        let hostClient = CloudClient(endpoint: hostEndpoint)
        let viewerClient = CloudClient(endpoint: viewerEndpoint)
        let owner = CloudTerminalHost(client: hostClient, bridge: bridge, reportError: { _ in })
        var signedInClients: [CloudClient] = []
        var scenarios: [Scenario] = []
        var relayEvidence: RelayEvidence?
        var rejectedStartEvidence: RejectedStartEvidence?
        var accountSwitchEvidence: AccountSwitchEvidence?
        var primaryFailure: (any Error)?

        do {
            _ = try await hostClient.complete(code: "relay-host", verifier: "fixture-verifier")
            signedInClients.append(hostClient)
            _ = try await viewerClient.complete(code: "relay-sender", verifier: "fixture-verifier")
            signedInClients.append(viewerClient)

            let hostDevice = try await hostClient.registerDevice(name: "Loopback host")
            let duplicateHost = try await hostClient.registerDevice(name: "Loopback host")
            let hostIdentity = try await hostClient.localDeviceIdentity()
            guard hostDevice.id == duplicateHost.id,
                  hostDevice.installationID == hostIdentity.installationID,
                  hostDevice.publicKey == hostIdentity.publicKey else {
                throw BridgeFailure(message: "Repeated host registration did not preserve one installation and device.")
            }

            let viewerDevice = try await viewerClient.registerDevice(name: "Loopback viewer")
            let duplicateViewer = try await viewerClient.registerDevice(name: "Loopback viewer")
            let viewerIdentity = try await viewerClient.localDeviceIdentity()
            guard viewerDevice.id == duplicateViewer.id,
                  viewerDevice.installationID == viewerIdentity.installationID,
                  viewerDevice.publicKey == viewerIdentity.publicKey,
                  hostIdentity.installationID != viewerIdentity.installationID else {
                throw BridgeFailure(message: "Repeated registration changed an installation or the two origins shared one identity.")
            }

            scenarios.append(try await runScenario(name: "toggle_off_during_local_device_lookup",
                                                    path: .explicitDisable, holdPoint: "devices",
                                                    expectedPresence: [], endpoint: hostEndpoint,
                                                    device: hostDevice, client: hostClient, bridge: bridge))
            scenarios.append(try await runScenario(name: "credential_stop_during_presence_publish",
                                                    path: .credentialChange, holdPoint: "presence",
                                                    expectedPresence: [true, false], endpoint: hostEndpoint,
                                                    device: hostDevice, client: hostClient, bridge: bridge))
            scenarios.append(try await runNamespaceInvalidationScenario(endpoint: hostEndpoint,
                                                                         device: hostDevice,
                                                                         client: hostClient,
                                                                         bridge: bridge))

            relayEvidence = try await runRelayIntegration(hostEndpoint: hostEndpoint,
                                                          viewerClient: viewerClient,
                                                          hostDevice: hostDevice,
                                                          owner: owner,
                                                          bridge: bridge)
            rejectedStartEvidence = try await runRejectedStartCanRetry(endpoint: hostEndpoint,
                                                                       client: hostClient,
                                                                       owner: owner,
                                                                       bridge: bridge)
            accountSwitchEvidence = try await runAccountSwitch(endpoint: hostEndpoint,
                                                                hostClient: hostClient,
                                                                viewerClient: viewerClient,
                                                                oldHostDevice: hostDevice,
                                                                owner: owner,
                                                                bridge: bridge)
        } catch {
            primaryFailure = error
        }

        var cleanupFailures: [String] = []
        if let stopFailure = await owner.stopForCredentialChange() {
            cleanupFailures.append("stop local sharing: " + stopFailure)
        }
        for client in signedInClients {
            do { _ = try await client.signOut() }
            catch { cleanupFailures.append("sign out fixture client: " + error.localizedDescription) }
        }
        do { try removeOwnedKeychainItems(ownedKeychainItems) }
        catch { cleanupFailures.append("remove fixture Keychain items: " + error.localizedDescription) }

        if !cleanupFailures.isEmpty {
            let cleanup = "Cloud terminal smoke cleanup failed: " + cleanupFailures.joined(separator: "; ")
            if let primaryFailure {
                throw BridgeFailure(message: primaryFailure.localizedDescription + "; " + cleanup)
            }
            throw BridgeFailure(message: cleanup)
        }
        if let primaryFailure { throw primaryFailure }
        guard let relayEvidence else {
            throw BridgeFailure(message: "Cloud terminal smoke did not produce two-peer evidence.")
        }
        guard let rejectedStartEvidence, let accountSwitchEvidence else {
            throw BridgeFailure(message: "Cloud terminal smoke did not produce account lifecycle evidence.")
        }

        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        let data = try encoder.encode(Evidence(scenarios: scenarios, relay: relayEvidence,
                                               rejectedStart: rejectedStartEvidence,
                                               accountSwitch: accountSwitchEvidence))
        guard data.count <= 32_768 else {
            throw BridgeFailure(message: "Cloud terminal smoke evidence exceeded its bound.")
        }
        try FileManager.default.createDirectory(at: report.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: report, options: .atomic)
    }

    private static func runRelayIntegration(hostEndpoint: CloudEndpoint, viewerClient: CloudClient,
                                            hostDevice: CloudDevice, owner: CloudTerminalHost,
                                            bridge: NativeBridge) async throws -> RelayEvidence {
        try await owner.setSharingEnabled(true)
        guard owner.sharingState.isSharing else {
            throw BridgeFailure(message: "The real Rust sharing grant did not become active.")
        }

        let viewerDirectory = try await viewerClient.account()
        guard let target = viewerDirectory.devices.first(where: { $0.id == hostDevice.id }),
              target.revokedAt == nil, target.online, target.sharing,
              target.sharingEpoch != nil, target.publicKey?.count == 32 else {
            throw BridgeFailure(message: "The second origin could not discover the actively sharing host device.")
        }

        let viewer = CloudTerminalHost(client: viewerClient, bridge: bridge, reportError: { _ in })
        let sessions = try await waitForSessions(using: viewer, on: target)
        guard let session = sessions.first else {
            throw BridgeFailure(message: "The sharing host did not expose an existing terminal session.")
        }
        _ = try await waitForFrame(using: viewer, for: session, containing: [], rows: nil, cols: nil)

        let inputMarker = "ZMUX_CLOUD_INPUT_" + UUID().uuidString.replacingOccurrences(of: "-", with: "")
        try await viewer.sendInput(shellPrintCommand(marker: inputMarker), to: session)
        _ = try await waitForFrame(using: viewer, for: session, containing: [inputMarker], rows: nil, cols: nil)

        let resizeRows = 18
        let resizeCols = 72
        try await viewer.sendResize(rows: resizeRows, cols: resizeCols, to: session)
        _ = try await waitForFrame(using: viewer, for: session, containing: [], rows: resizeRows, cols: resizeCols)
        let resizeMarker = "ZMUX_CLOUD_SIZE_" + UUID().uuidString.replacingOccurrences(of: "-", with: "")
        try await viewer.sendInput("stty size; " + shellPrintCommand(marker: resizeMarker), to: session)
        let resizedFrame = try await waitForFrame(using: viewer, for: session,
                                                  containing: ["18 72", resizeMarker],
                                                  rows: resizeRows, cols: resizeCols)
        guard let viewport = resizedFrame.snapshot?.viewport,
              viewport.size.rows == resizeRows, viewport.size.cols == resizeCols else {
            throw BridgeFailure(message: "The remote frame did not retain the requested terminal size.")
        }

        try await owner.stopSharing()
        guard case .off = owner.sharingState else {
            throw BridgeFailure(message: "Cloud terminal sharing did not return to off after the relay exercise.")
        }
        let stoppedDirectory = try await viewerClient.account()
        guard let stoppedHost = stoppedDirectory.devices.first(where: { $0.id == hostDevice.id }),
              !stoppedHost.sharing else {
            throw BridgeFailure(message: "The host presence did not clear after stopping terminal sharing.")
        }
        let fixtureState: FixtureState = try await decodeFixtureState(endpoint: hostEndpoint)
        guard fixtureState.presenceUpdates.last == false else {
            throw BridgeFailure(message: "The loopback service did not observe the final sharing=false presence update.")
        }

        let disabledReply = try await bridge.sharedTerminalOperation(
            request: Data(#"{"operation":"list"}"#.utf8), grant: UUID())
        let errorCode = try decodeDisabledGateCode(disabledReply)

        return RelayEvidence(sameDeviceAfterRepeatedRegistration: true,
                             listedSessions: sessions.count, readExistingFrame: true,
                             inputMarker: inputMarker, inputMarkerObserved: true,
                             resizeRows: resizeRows, resizeCols: resizeCols,
                             viewportRows: viewport.size.rows, viewportCols: viewport.size.cols,
                             kernelSizeOutput: "18 72", presenceCleared: true,
                             postStopGateErrorCode: errorCode)
    }

    private static func runRejectedStartCanRetry(endpoint: CloudEndpoint, client: CloudClient,
                                                 owner: CloudTerminalHost, bridge: NativeBridge) async throws
        -> RejectedStartEvidence {
        _ = try await fixtureRequest(endpoint: endpoint, path: "/__fixture/reset", method: "POST")
        _ = try await fixtureRequest(endpoint: endpoint, path: "/__fixture/fail-next-presence", method: "POST",
                                     body: ["sharing": true, "status": 409])

        var rejection: String?
        do { try await owner.setSharingEnabled(true) }
        catch { rejection = error.localizedDescription }
        guard rejection != nil else {
            throw BridgeFailure(message: "The fixture's presence=true 409 did not reject the first sharing start.")
        }

        let rejectedState: FixtureState = try await decodeFixtureState(endpoint: endpoint)
        guard rejectedState.presenceUpdates == [true],
              rejectedState.presenceAttempts == [PresenceAttempt(sharing: true, status: 409)],
              case .off = owner.sharingState else {
            throw BridgeFailure(message: "A rejected sharing start left an active gate or sent a wrong-epoch presence=false cleanup.")
        }
        _ = try await rustDisabledGateCode(bridge)
        let identityAfterRejection = try await client.localDeviceIdentity()
        let rejectedDirectory = try await client.account()
        guard let deviceAfterRejection = rejectedDirectory.devices.first(where: { $0.installationID == identityAfterRejection.installationID }),
              !deviceAfterRejection.sharing else {
            throw BridgeFailure(message: "The rejected start changed the fixture's authoritative Cloud presence.")
        }

        try await owner.setSharingEnabled(true)
        guard owner.sharingState.isSharing else {
            throw BridgeFailure(message: "A rejected sharing start blocked a separate retry.")
        }
        let retryDirectory = try await client.account()
        guard let retryDevice = retryDirectory.devices.first(where: { $0.installationID == deviceAfterRejection.installationID }),
              retryDevice.sharing else {
            throw BridgeFailure(message: "The separate retry did not publish sharing=true under the current account.")
        }

        try await owner.stopSharing()
        guard case .off = owner.sharingState else {
            throw BridgeFailure(message: "Sharing did not stop after the rejected-start retry.")
        }
        let stoppedDirectory = try await client.account()
        guard let stoppedDevice = stoppedDirectory.devices.first(where: { $0.id == retryDevice.id }),
              !stoppedDevice.sharing else {
            throw BridgeFailure(message: "The successful retry did not clear current-account sharing presence.")
        }
        let fixtureState: FixtureState = try await decodeFixtureState(endpoint: endpoint)
        guard fixtureState.presenceUpdates == [true, true, false],
              fixtureState.presenceAttempts == [PresenceAttempt(sharing: true, status: 409),
                                                PresenceAttempt(sharing: true, status: 200),
                                                PresenceAttempt(sharing: false, status: 200)] else {
            throw BridgeFailure(message: "Rejected-start recovery did not preserve the expected presence attempts.")
        }
        let disabledCode = try await rustDisabledGateCode(bridge)
        return RejectedStartEvidence(presenceUpdates: fixtureState.presenceUpdates,
                                     presenceAttempts: fixtureState.presenceAttempts,
                                     ownerOffAfterRejection: true, localGateClosedAfterRejection: true,
                                     retryEnabled: true, retryStopped: true,
                                     postStopGateErrorCode: disabledCode)
    }

    private static func runAccountSwitch(endpoint: CloudEndpoint, hostClient: CloudClient,
                                         viewerClient: CloudClient, oldHostDevice: CloudDevice,
                                         owner: CloudTerminalHost, bridge: NativeBridge) async throws
        -> AccountSwitchEvidence {
        _ = try await fixtureRequest(endpoint: endpoint, path: "/__fixture/reset", method: "POST")
        try await owner.setSharingEnabled(true)
        guard owner.sharingState.isSharing else {
            throw BridgeFailure(message: "The old account could not enable sharing before the account-switch exercise.")
        }

        let oldAccount = try await hostClient.account()
        let oldIdentity = try await hostClient.localDeviceIdentity()
        guard let oldAccountDevice = oldAccount.devices.first(where: { $0.id == oldHostDevice.id }),
              oldAccountDevice.installationID == oldIdentity.installationID else {
            throw BridgeFailure(message: "The old account's registered host did not match this installation.")
        }

        _ = try await fixtureRequest(endpoint: endpoint, path: "/__fixture/fail-next-presence", method: "POST",
                                     body: ["sharing": false, "status": 401])
        var failedStop: String?
        do { try await owner.setSharingEnabled(false) }
        catch { failedStop = error.localizedDescription }
        guard let failedStop, !failedStop.isEmpty else {
            throw BridgeFailure(message: "The injected expired-session presence failure did not reach the stop caller.")
        }
        let oldGateCode = try await rustDisabledGateCode(bridge)
        guard case .failed = owner.sharingState else {
            throw BridgeFailure(message: "An unconfirmed old-account Cloud clear was reported as a successful host stop.")
        }
        let oldDirectoryAfter401 = try await viewerClient.account()
        guard let oldDeviceAfter401 = oldDirectoryAfter401.devices.first(where: { $0.id == oldHostDevice.id }),
              oldDeviceAfter401.sharing else {
            throw BridgeFailure(message: "The 401 fixture did not retain the old account's unresolved sharing presence.")
        }

        let signOutWarning = await owner.stopForCredentialChange()
        guard let signOutWarning, !signOutWarning.isEmpty else {
            throw BridgeFailure(message: "Sign-out did not surface the unresolved old-account cleanup warning.")
        }
        _ = try await hostClient.signOut()

        let newAccount = try await hostClient.complete(code: "relay-account-two", verifier: "fixture-verifier")
        guard newAccount.user.id != oldAccount.user.id else {
            throw BridgeFailure(message: "The account-switch fixture signed back into the old account.")
        }
        let newHostDevice = try await hostClient.registerDevice(name: "Loopback host under second account")
        let newIdentity = try await hostClient.localDeviceIdentity()
        guard newHostDevice.installationID == oldIdentity.installationID,
              newHostDevice.publicKey == oldIdentity.publicKey,
              newIdentity.installationID == oldIdentity.installationID,
              newHostDevice.id != oldHostDevice.id else {
            throw BridgeFailure(message: "The second account did not register the same installation as its distinct device.")
        }

        var retirementWarning: String?
        do { try await owner.setSharingEnabled(true) }
        catch { retirementWarning = error.localizedDescription }
        guard let retirementWarning else {
            throw BridgeFailure(message: "The first enable under a different account did not retire the old cleanup context.")
        }
        let loweredWarning = retirementWarning.lowercased()
        guard loweredWarning.contains("presence"), loweredWarning.contains("not cleared"),
              loweredWarning.contains("different account"), case .off = owner.sharingState else {
            throw BridgeFailure(message: "Account switching did not clearly report unresolved old presence while keeping the host off.")
        }
        let newAccountRetryGateCode = try await rustDisabledGateCode(bridge)

        let oldDirectoryAfterSwitch = try await viewerClient.account()
        guard let oldDeviceAfterSwitch = oldDirectoryAfterSwitch.devices.first(where: { $0.id == oldHostDevice.id }),
              oldDeviceAfterSwitch.sharing else {
            throw BridgeFailure(message: "Retiring the old cleanup context falsely cleared or hid old-account Cloud sharing.")
        }

        try await owner.setSharingEnabled(true)
        guard owner.sharingState.isSharing else {
            throw BridgeFailure(message: "A separate enable action could not start sharing under the second account.")
        }
        let enabledDirectory = try await hostClient.account()
        guard let enabledDevice = enabledDirectory.devices.first(where: { $0.id == newHostDevice.id }),
              enabledDevice.sharing else {
            throw BridgeFailure(message: "The second account did not publish its own sharing presence.")
        }

        try await owner.stopSharing()
        guard case .off = owner.sharingState else {
            throw BridgeFailure(message: "The second account's sharing grant did not stop.")
        }
        let stoppedDirectory = try await hostClient.account()
        guard let stoppedDevice = stoppedDirectory.devices.first(where: { $0.id == newHostDevice.id }),
              !stoppedDevice.sharing else {
            throw BridgeFailure(message: "The second account's stop did not confirm sharing=false.")
        }
        let fixtureState: FixtureState = try await decodeFixtureState(endpoint: endpoint)
        let expectedAttempts = [PresenceAttempt(sharing: true, status: 200),
                                PresenceAttempt(sharing: false, status: 401),
                                PresenceAttempt(sharing: true, status: 200),
                                PresenceAttempt(sharing: false, status: 200)]
        guard fixtureState.presenceUpdates == [true, false, true, false],
              fixtureState.presenceAttempts == expectedAttempts else {
            throw BridgeFailure(message: "The account switch replayed an old cleanup or lost the new account's presence sequence.")
        }
        let finalGateCode = try await rustDisabledGateCode(bridge)
        return AccountSwitchEvidence(presenceUpdates: fixtureState.presenceUpdates,
                                     presenceAttempts: fixtureState.presenceAttempts,
                                     oldStopWarningReturned: true,
                                     oldPresenceClearConfirmed: false,
                                     oldDeviceStillSharingAfterExpiredSession: oldDeviceAfter401.sharing,
                                     oldDeviceStillSharingAfterAccountSwitch: oldDeviceAfterSwitch.sharing,
                                     localGateClosedAfter401: oldGateCode == "sharing_disabled",
                                     oldCleanupContextRetiredWithUnresolvedWarning: true,
                                     sameInstallationAfterAccountSwitch: true,
                                     newAccountDeviceIsDifferent: true,
                                     firstNewAccountEnableRequiredSeparateAction: true,
                                     localGateClosedAfterRejectedEnable: newAccountRetryGateCode == "sharing_disabled",
                                     newAccountEnabled: enabledDevice.sharing,
                                     newAccountStopConfirmed: !stoppedDevice.sharing,
                                     postOldStopGateErrorCode: oldGateCode,
                                     postNewStopGateErrorCode: finalGateCode)
    }

    private static func waitForSessions(using host: CloudTerminalHost, on device: CloudDevice) async throws
        -> [CloudTerminalSessionDescriptor] {
        let deadline = ContinuousClock.now + .seconds(10)
        while ContinuousClock.now < deadline {
            let sessions = try await host.listSessions(on: device)
            if !sessions.isEmpty { return sessions }
            try await Task.sleep(for: .milliseconds(200))
        }
        throw BridgeFailure(message: "The remote list request did not find an existing host terminal before its deadline.")
    }

    private static func waitForFrame(using host: CloudTerminalHost, for session: CloudTerminalSessionDescriptor,
                                     containing strings: [String], rows: Int?, cols: Int?) async throws -> Frame {
        let deadline = ContinuousClock.now + .seconds(10)
        while ContinuousClock.now < deadline {
            let frame = try await host.readFrame(for: session)
            if let viewport = frame.snapshot?.viewport,
               strings.allSatisfy({ viewport.contents.contains($0) }),
               rows.map({ viewport.size.rows == $0 }) ?? true,
               cols.map({ viewport.size.cols == $0 }) ?? true {
                return frame
            }
            try await Task.sleep(for: .milliseconds(150))
        }
        throw BridgeFailure(message: "The remote frame did not show the expected shell output or terminal size before its deadline.")
    }

    private static func shellPrintCommand(marker: String) -> String {
        let split = marker.index(marker.startIndex, offsetBy: marker.count / 2)
        let first = String(marker[..<split])
        let second = String(marker[split...])
        // Adjacent shell-quoted pieces become one argument, while the complete marker
        // cannot appear in the PTY's echoed input.
        return "printf '%s\\n' '\(first)''\(second)'\n"
    }

    private static func runScenario(name: String, path: StopPath, holdPoint: String,
                                    expectedPresence: [Bool], endpoint: CloudEndpoint,
                                    device: CloudDevice, client: CloudClient,
                                    bridge: NativeBridge) async throws -> Scenario {
        let host = CloudTerminalHost(client: client, bridge: bridge, reportError: { _ in })
        var start: Task<Void, Error>?
        var stop: Task<String?, Error>?
        do {
            _ = try await fixtureRequest(endpoint: endpoint, path: "/__fixture/reset", method: "POST")
            _ = try await fixtureRequest(endpoint: endpoint, path: "/__fixture/arm", method: "POST",
                                         body: ["point": holdPoint])
            let starting = Task { try await host.setSharingEnabled(true) }
            start = starting
            try await waitForHeldRequest(endpoint: endpoint)
            guard case .starting = host.sharingState else {
                throw BridgeFailure(message: "Sharing was not starting while Cloud held the \(holdPoint) request.")
            }

            let stopping = Task<String?, Error> { @MainActor () throws -> String? in
                switch path {
                case .explicitDisable:
                    try await host.setSharingEnabled(false)
                    return nil
                case .credentialChange:
                    return await host.stopForCredentialChange()
                }
            }
            stop = stopping
            try await waitForStopping(host)
            try await assertPending(stopping)
            _ = try await fixtureRequest(endpoint: endpoint, path: "/__fixture/release", method: "POST")

            switch await starting.result {
            case .failure(let error) where error is CancellationError: break
            case .failure(let error):
                throw BridgeFailure(message: "Cloud lookup did not cancel after stop intent: \(error.localizedDescription)")
            case .success:
                throw BridgeFailure(message: "Cloud lookup completed a sharing start after the stop intent.")
            }
            guard case .success(nil) = await stopping.result else {
                throw BridgeFailure(message: "Stopping a suspended local-device lookup did not complete cleanly.")
            }
            guard case .off = host.sharingState else {
                throw BridgeFailure(message: "Sharing did not return to off after stopping during \(holdPoint).")
            }
            let fixtureState: FixtureState = try await decodeFixtureState(endpoint: endpoint)
            let expectedAttempts = expectedPresence.map { PresenceAttempt(sharing: $0, status: 200) }
            guard fixtureState.presenceUpdates == expectedPresence,
                  fixtureState.presenceAttempts == expectedAttempts else {
                throw BridgeFailure(message: "Sharing presence did not match the expected stop-race sequence for \(name).")
            }
            let disabledCode = try await rustDisabledGateCode(bridge)
            return Scenario(name: name, hostState: "off", presenceUpdates: fixtureState.presenceUpdates,
                            presenceAttempts: fixtureState.presenceAttempts,
                            localGateConfirmedDisabled: true, postStopGateErrorCode: disabledCode)
        } catch {
            _ = try? await fixtureRequest(endpoint: endpoint, path: "/__fixture/release", method: "POST")
            _ = await host.stopForCredentialChange()
            if let stop { _ = await stop.result }
            if let start { _ = await start.result }
            throw error
        }
    }

    private static func runNamespaceInvalidationScenario(endpoint: CloudEndpoint, device: CloudDevice,
                                                         client: CloudClient, bridge: NativeBridge) async throws -> Scenario {
        let host = CloudTerminalHost(client: client, bridge: bridge, reportError: { _ in })
        var starting: Task<Void, Error>?
        do {
            _ = try await fixtureRequest(endpoint: endpoint, path: "/__fixture/reset", method: "POST")
            _ = try await fixtureRequest(endpoint: endpoint, path: "/__fixture/arm", method: "POST",
                                         body: ["point": "presence"])
            let task = Task { try await host.setSharingEnabled(true) }
            starting = task
            try await waitForHeldRequest(endpoint: endpoint)
            guard case .starting = host.sharingState,
                  let registered = try await client.localDevice(), registered.id == device.id else {
                throw BridgeFailure(message: "Namespace invalidation did not begin from the registered host device.")
            }

            // Presence=true has reached the fixture but remains unacknowledged. Make its first
            // presence=false cleanup attempt fail so the queued namespace stop must retry it.
            _ = try await fixtureRequest(endpoint: endpoint, path: "/__fixture/fail-next-presence-clear", method: "POST")
            host.desktopNamespaceDidChange()
            try await waitForStopping(host)
            _ = try await fixtureRequest(endpoint: endpoint, path: "/__fixture/release", method: "POST")

            switch await task.result {
            case .success:
                throw BridgeFailure(message: "Sharing startup completed after its desktop namespace was invalidated.")
            case .failure:
                break
            }

            let deadline = ContinuousClock.now + .seconds(10)
            var fixtureState: FixtureState?
            while ContinuousClock.now < deadline {
                let current = try await decodeFixtureState(endpoint: endpoint)
                if case .off = host.sharingState, current.presenceAttempts.count >= 3 {
                    fixtureState = current
                    break
                }
                try await Task.sleep(for: .milliseconds(50))
            }
            guard case .off = host.sharingState, let fixtureState,
                  fixtureState.presenceUpdates == [true, false, false],
                  fixtureState.presenceAttempts == [PresenceAttempt(sharing: true, status: 200),
                                                    PresenceAttempt(sharing: false, status: 503),
                                                    PresenceAttempt(sharing: false, status: 200)] else {
                throw BridgeFailure(message: "Namespace invalidation did not retry and confirm presence=false after the injected 503.")
            }

            let disabledCode = try await rustDisabledGateCode(bridge)
            return Scenario(name: "namespace_change_retries_failed_presence_clear", hostState: "off",
                            presenceUpdates: fixtureState.presenceUpdates,
                            presenceAttempts: fixtureState.presenceAttempts,
                            localGateConfirmedDisabled: true, postStopGateErrorCode: disabledCode)
        } catch {
            _ = try? await fixtureRequest(endpoint: endpoint, path: "/__fixture/release", method: "POST")
            _ = await host.stopForCredentialChange()
            if let starting { _ = await starting.result }
            throw error
        }
    }

    private static func rustDisabledGateCode(_ bridge: NativeBridge) async throws -> String {
        let reply = try await bridge.sharedTerminalOperation(
            request: Data(#"{"operation":"list"}"#.utf8), grant: UUID())
        return try decodeDisabledGateCode(reply)
    }

    private static func decodeDisabledGateCode(_ reply: Data) throws -> String {
        guard let object = try JSONSerialization.jsonObject(with: reply) as? [String: Any],
              object["ok"] as? Bool == false,
              let error = object["error"] as? [String: Any],
              let errorCode = error["code"] as? String,
              errorCode == "sharing_disabled" else {
            throw BridgeFailure(message: "The stopped Rust terminal owner did not return sharing_disabled for a raw list request.")
        }
        return errorCode
    }

    private static func waitForHeldRequest(endpoint: CloudEndpoint) async throws {
        let deadline = ContinuousClock.now + .seconds(8)
        while ContinuousClock.now < deadline {
            let data = try await fixtureRequest(endpoint: endpoint, path: "/__fixture/held")
            if try JSONDecoder().decode([String: Bool].self, from: data)["held"] == true { return }
            try await Task.sleep(for: .milliseconds(20))
        }
        throw BridgeFailure(message: "The loopback service did not suspend the expected Cloud request.")
    }

    private static func assertPending(_ task: Task<String?, Error>) async throws {
        let flag = CloudTerminalTaskCompletionFlag()
        let monitor = Task {
            _ = await task.result
            await flag.markComplete()
        }
        try await Task.sleep(for: .milliseconds(100))
        let completedEarly = await flag.completed
        _ = monitor
        guard !completedEarly else {
            throw BridgeFailure(message: "The stop returned while startup's Cloud request was still suspended.")
        }
    }

    private static func waitForStopping(_ host: CloudTerminalHost) async throws {
        let deadline = ContinuousClock.now + .seconds(2)
        while ContinuousClock.now < deadline {
            if case .stopping = host.sharingState { return }
            try await Task.sleep(for: .milliseconds(1))
        }
        throw BridgeFailure(message: "The stop request did not supersede the suspended sharing start.")
    }

    private static func decodeFixtureState(endpoint: CloudEndpoint) async throws -> FixtureState {
        let data = try await fixtureRequest(endpoint: endpoint, path: "/__fixture/state")
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(FixtureState.self, from: data)
    }

    private static func isOwnedLoopbackEndpoint(_ endpoint: CloudEndpoint) -> Bool {
        guard endpoint.origin.scheme == "http",
              endpoint.origin.host == "127.0.0.1",
              let port = endpoint.origin.port, (1...65_535).contains(port),
              endpoint.origin.user == nil, endpoint.origin.password == nil,
              let components = URLComponents(url: endpoint.origin, resolvingAgainstBaseURL: false),
              components.path.isEmpty || components.path == "/",
              components.query == nil, components.fragment == nil else { return false }
        return true
    }

    private static func preflightKeychainOwnership(endpoints: [CloudEndpoint]) throws -> OwnedKeychainItems {
        let queries = endpoints.flatMap { endpoint in
            [
                [kSecClass as String: kSecClassGenericPassword,
                 kSecAttrService as String: "app.w3dev.zmux.dev.cloud",
                 kSecAttrAccount as String: endpoint.origin.absoluteString],
                CloudDeviceIdentity.keychainQuery(endpoint: endpoint),
            ]
        }
        let statuses = queries.map { SecItemCopyMatching($0 as CFDictionary, nil) }
        guard statuses.allSatisfy({ $0 == errSecItemNotFound }) else {
            if statuses.contains(errSecSuccess) {
                throw BridgeFailure(message: "Cloud terminal smoke refuses either origin if an OAuth credential or device identity already exists.")
            }
            let status = statuses.first(where: { $0 != errSecItemNotFound }) ?? errSecInternalError
            throw CloudFailure(message: "Unable to verify both Cloud fixture origins are absent from Keychain (\(status)).")
        }
        return OwnedKeychainItems(queries: queries)
    }

    private static func removeOwnedKeychainItems(_ owned: OwnedKeychainItems) throws {
        var failureStatus: OSStatus?
        for query in owned.queries {
            let status = SecItemDelete(query as CFDictionary)
            guard status == errSecSuccess || status == errSecItemNotFound else {
                failureStatus = failureStatus ?? status
                continue
            }
            let verification = SecItemCopyMatching(query as CFDictionary, nil)
            if verification != errSecItemNotFound {
                failureStatus = failureStatus ?? verification
            }
        }
        if let failureStatus {
            throw CloudFailure(message: "Unable to remove or verify the owned Cloud fixture Keychain items (\(failureStatus)).")
        }
    }

    private static func fixtureRequest(endpoint: CloudEndpoint, path: String, method: String = "GET",
                                       body: [String: Any]? = nil) async throws -> Data {
        let relativePath = path.hasPrefix("/") ? String(path.dropFirst()) : path
        var request = URLRequest(url: endpoint.url(relativePath))
        request.httpMethod = method
        request.timeoutInterval = 3
        if let body {
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let response = response as? HTTPURLResponse, (200..<300).contains(response.statusCode) else {
            throw BridgeFailure(message: "Loopback Cloud terminal fixture control request failed (\((response as? HTTPURLResponse)?.statusCode ?? 0)).")
        }
        return data
    }
}

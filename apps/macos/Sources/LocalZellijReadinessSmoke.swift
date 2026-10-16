import AppKit

/// Actual AppKit responder, coordinator and retained final-bundle CLI journey.
/// Only typed Rust InputReady admits the first write; workload output is an oracle.
@MainActor
enum LocalZellijReadinessSmoke {
    private struct Fixture: Decodable, Sendable {
        let silentCommand: String
        let firstLine: String
        let secondLine: String
    }
    private struct ProcessIdentity: Codable, Equatable, Sendable {
        let pid: UInt32
        let uid: UInt32
        let parentPid: UInt32
        let start: [Int64]
        let boot: String
        let executable: String
    }
    private struct Owner: Codable, Equatable, Sendable {
        struct Completion: Codable, Equatable, Sendable { let ownerGeneration: UUID }
        let id: UUID
        let server: ProcessIdentity
        let supervisor: ProcessIdentity
        let workload: ProcessIdentity
        let completion: Completion
    }
    private struct Observed: Encodable, Sendable {
        let stage: String
        let operation: NativeLocalZellijOperation
    }
    private struct Attachment: Encodable, Sendable {
        let operationID: UInt64
        let surfaceID: UInt64
        let surfaceUUID: UUID
        let sessionID: UInt64
        let sessionPID: UInt32
        let windowID: UInt64
        let windowNumber: Int
        let actualInputResponder: Bool
        let line: String
    }
    private struct Completed: Encodable, Sendable {
        let operationID: UInt64
        let owner: UUID
        let exitCode: Int
        let surfaceID: UInt64
        let readOnlyMounted: Bool
        let typingRejected: Bool
        let statusLabel: String
        let savedDocument: String
    }
    private struct Evidence: Encodable, Sendable {
        let scope = "actual_relocated_AppKit_readiness_start_attach_completion"
        let sourceSHA: String
        let appPID: Int32
        let bundlePath: String
        let cliPath: String
        let cliSHA256: String
        let zellijPath: String
        let binarySHA256: String
        let manifestSHA256: String
        let installationEpoch: UUID
        let profileID: UUID
        var state = "starting"
        var activationPID: Int32?
        var operations: [Observed] = []
        var attachments: [Attachment] = []
        var owners: [Owner] = []
        var completions: [Completed] = []
        var failure: String?
    }

    static func run(coordinator: ApplicationCoordinator, admitted: NativeLocalZellijBundle,
                    report: URL, stateDirectory: URL) async throws {
        let root = report.deletingLastPathComponent()
        var evidence = Evidence(sourceSHA: admitted.sourceSHA, appPID: ProcessInfo.processInfo.processIdentifier,
            bundlePath: admitted.bundleRoot.path, cliPath: admitted.configuration.installation.cli,
            cliSHA256: admitted.cliSHA256, zellijPath: admitted.configuration.installation.zellij,
            binarySHA256: admitted.binarySHA256, manifestSHA256: admitted.manifestSHA256,
            installationEpoch: admitted.configuration.installation.epoch, profileID: admitted.profileID)
        do {
            let fixture = try await Task.detached {
                let data = try NativeLocalZellijBundle.bytes(report.appendingPathExtension("input.json"), limit: 8192)
                let value = try JSONDecoder().decode(Fixture.self, from: data)
                guard !value.silentCommand.isEmpty, value.silentCommand.utf8.count <= 4096,
                      [value.firstLine, value.secondLine].allSatisfy({ !$0.isEmpty && $0.utf8.count <= 128 && !$0.contains("\n") && !$0.contains("\r") }),
                      value.firstLine != value.secondLine else { throw failure("Invalid bounded native readiness fixture") }
                return value
            }.value
            try await wait(coordinator, "initial native persistent workspace") { state in
                state.persistence?.enabled == true && state.persistence?.startupPending == false &&
                state.persistence?.restoring == false && state.model.workspaces.count == 1 &&
                coordinator.mainWindowHosts.values.allSatisfy(\.nativeReady)
            }
            guard let initial = coordinator.snapshot, let pane = initial.model.workspaces.first?.root.panes.first else {
                throw failure("No canonical initial native pane")
            }
            evidence.activationPID = try await activateOwnedApplication()
            guard evidence.activationPID == evidence.appPID else { throw failure("App activation substituted a different PID") }
            let existing = Set(initial.localZellijOperations.map(\.operationId))
            coordinator.enqueue(.localZellij(.start(profile: admitted.profileID, pane: pane.id,
                name: "native-silent-readiness", cwd: root.path, command: fixture.silentCommand)))
            let first = try await operation(coordinator, excluding: existing) { $0.inputReady }
            try requireReady(first, admitted: admitted, coordinator: coordinator)
            evidence.operations.append(Observed(stage: "first_typed_ready", operation: first))
            let firstOwner = try await owner(admitted, id: first.ownerUuid!)
            evidence.owners.append(firstOwner)
            evidence.attachments.append(try await input(coordinator, operation: first, pane: pane.id, line: fixture.firstLine))
            try await file(root.appendingPathComponent("silent-lines"), equals: fixture.firstLine + "\n")
            try await file(root.appendingPathComponent("silent-count"), equals: "x")
            try await save(evidence, report)
            coordinator.enqueue(.surface("close_surface", evidence.attachments[0].surfaceID, nil, nil, nil))
            let detached = try await operation(coordinator, id: first.operationId) { $0.sessionJoined }
            try requireJoined(detached, completed: false)
            evidence.operations.append(Observed(stage: "first_frontend_joined", operation: detached))
            guard try await owner(admitted, id: firstOwner.id) == firstOwner else { throw failure("Detach replaced the retained owner") }
            let beforeAttach = Set(coordinator.snapshot!.localZellijOperations.map(\.operationId))
            coordinator.enqueue(.localZellij(.attach(profile: admitted.profileID, owner: firstOwner.id, pane: pane.id)))
            let second = try await operation(coordinator, excluding: beforeAttach) { $0.inputReady }
            try requireReady(second, admitted: admitted, coordinator: coordinator)
            guard second.ownerUuid == first.ownerUuid, second.attachmentId != first.attachmentId,
                  second.frontendPid != first.frontendPid else { throw failure("Reattach reused the old frontend or changed owner") }
            let secondOwner = try await owner(admitted, id: firstOwner.id)
            guard secondOwner == firstOwner else { throw failure("Reattach changed original backend/supervisor/workload identity") }
            evidence.owners.append(secondOwner)
            evidence.operations.append(Observed(stage: "second_typed_ready", operation: second))
            evidence.attachments.append(try await input(coordinator, operation: second, pane: pane.id, line: fixture.secondLine))
            let silentDone = try await operation(coordinator, id: second.operationId) { $0.sessionJoined }
            try requireJoined(silentDone, completed: true, exit: 7)
            evidence.operations.append(Observed(stage: "silent_workload_completed", operation: silentDone))
            try await file(root.appendingPathComponent("silent-lines"), equals: fixture.firstLine + "\n" + fixture.secondLine + "\n")
            try await file(root.appendingPathComponent("silent-count"), equals: "x")
            evidence.completions.append(try await completion(coordinator, operation: silentDone, code: 7,
                label: "silent", stateDirectory: stateDirectory, root: root))
            try await save(evidence, report)
            coordinator.enqueue(.surface("close_surface", evidence.completions.last!.surfaceID, nil, nil, nil))

            for code in [0, 7] {
                let before = Set(coordinator.snapshot!.localZellijOperations.map(\.operationId))
                let count = root.appendingPathComponent("finite-\(code)-count")
                coordinator.enqueue(.localZellij(.start(profile: admitted.profileID, pane: pane.id,
                    name: "native-before-ready-\(code)", cwd: root.path,
                    command: "printf x >> " + quote(count.path) + "; exit \(code)")))
                let done = try await operation(coordinator, excluding: before) { $0.sessionJoined }
                guard !done.inputReady else { throw failure("Finite completion manufactured interactive input readiness") }
                try requireJoined(done, completed: true, exit: code)
                evidence.operations.append(Observed(stage: "terminal_before_ready_\(code)", operation: done))
                evidence.owners.append(try await owner(admitted, id: done.ownerUuid!))
                try await file(count, equals: "x")
                evidence.completions.append(try await completion(coordinator, operation: done, code: code,
                    label: "before-ready-\(code)", stateDirectory: stateDirectory, root: root))
                try await save(evidence, report)
                coordinator.enqueue(.surface("close_surface", evidence.completions.last!.surfaceID, nil, nil, nil))
            }
            evidence.state = "assertions_complete"
            try await save(evidence, report)
            try await coordinator.shutdown()
            evidence.state = "complete"
            try await save(evidence, report)
        } catch {
            evidence.state = "failed"; evidence.failure = String(describing: error)
            try await save(evidence, report)
            throw error
        }
    }

    private static func input(_ coordinator: ApplicationCoordinator, operation: NativeLocalZellijOperation,
                              pane: UInt64, line: String) async throws -> Attachment {
        try await wait(coordinator, "mounted typed-ready native terminal") { state in
            surfaces(state).contains { $0.localZellij?.ownerUuid == operation.ownerUuid &&
                $0.sessionId.flatMap { coordinator.terminals[$0]?.window } != nil }
        }
        guard let state = coordinator.snapshot,
              let surface = surfaces(state).first(where: { $0.localZellij?.ownerUuid == operation.ownerUuid }),
              let session = surface.sessionId, let view = coordinator.terminals[session],
              let frame = state.frames.first(where: { $0.sessionId == session }), !frame.starting,
              frame.failure == nil, let pid = frame.processId, let window = view.window,
              let host = coordinator.mainWindowHosts.values.first(where: { $0.window === window }),
              let windowID = host.identity?.windowId else { throw failure("Typed-ready native Session/view identity is missing") }
        // Bundled CLI enters the same real control socket and exposes the public
        // model UUID, independently correlating the operation with its mounted tab.
        let data = try await host.nativeCLI(["list-pane-surfaces", String(pane)], externalCaller: true)
        guard let listing = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let listed = listing["surfaces"] as? [[String: Any]],
              listed.contains(where: { ($0["surface_id"] as? NSNumber)?.uint64Value == surface.id &&
                  ($0["surface_uuid"] as? String).flatMap(UUID.init(uuidString:)) == operation.surface }) else {
            throw failure("Final bundled CLI model identity differs from native readiness operation")
        }
        host.installMenus(); window.makeKeyAndOrderFront(nil); NSApp.activate()
        guard window.makeFirstResponder(view) else { throw failure("Native terminal refused first responder") }
        try await wait(coordinator, "actual terminal input responder") { _ in
            NSApp.isActive && NSApp.keyWindow === window && window.firstResponder === view
        }
        // One actual NSTextInputClient call, through the retained onInput route.
        // No setup key, queue, input replay or workload output admits this write.
        view.insertText(line + "\r", replacementRange: NSRange(location: NSNotFound, length: 0))
        return Attachment(operationID: operation.operationId, surfaceID: surface.id, surfaceUUID: operation.surface,
            sessionID: session, sessionPID: pid, windowID: windowID, windowNumber: window.windowNumber,
            actualInputResponder: true, line: line)
    }

    private static func completion(_ coordinator: ApplicationCoordinator, operation: NativeLocalZellijOperation,
                                   code: Int, label: String, stateDirectory: URL, root: URL) async throws -> Completed {
        try await wait(coordinator, "mounted authenticated saved completion") { state in
            surfaces(state).contains { surface in
                surface.localZellij?.ownerUuid == operation.ownerUuid && surface.sessionId == nil &&
                surface.localZellijCompletion != nil && coordinator.completedHistories[surface.id]?.window != nil
            }
        }
        guard let state = coordinator.snapshot,
              let surface = surfaces(state).first(where: { $0.localZellij?.ownerUuid == operation.ownerUuid }),
              surface.sessionId == nil, let history = surface.localZellijCompletion,
              history.receipt.contractVersion == 2, history.receipt.workloadExit.kind == .exited,
              history.receipt.workloadExit.value == code,
              let view = coordinator.completedHistories[surface.id], let window = view.window,
              let output = view.outputResponder as? NSTextView,
              output.isSelectable, !output.isEditable,
              output.string == (history.viewport?.contents ?? "") else { throw failure("Completed owner is not exact read-only mounted history") }
        let status = "Process exited with status \(code)"
        guard labels(view).contains(status) else { throw failure("Saved native completion omitted the actual exit status label") }
        let original = output.string
        window.makeKeyAndOrderFront(nil); NSApp.activate()
        guard window.makeFirstResponder(output), let event = NSEvent.keyEvent(with: .keyDown, location: .zero,
            modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, characters: "X", charactersIgnoringModifiers: "X", isARepeat: false, keyCode: 7) else {
            throw failure("Completed-history native responder is unavailable")
        }
        window.sendEvent(event)
        guard output.string == original, surface.sessionId == nil else { throw failure("Completed native typing changed saved history") }
        let saved = "saved-" + label + ".json"
        try await Task.detached {
            let data = try NativeLocalZellijBundle.bytes(stateDirectory.appendingPathComponent("session.json"), limit: 4 * 1024 * 1024)
            try data.write(to: root.appendingPathComponent(saved), options: .atomic)
        }.value
        return Completed(operationID: operation.operationId, owner: operation.ownerUuid!, exitCode: code,
            surfaceID: surface.id, readOnlyMounted: true, typingRejected: true, statusLabel: status, savedDocument: saved)
    }
    private static func labels(_ view: NSView) -> [String] {
        (view as? NSTextField).map { [$0.stringValue] } ?? view.subviews.flatMap { labels($0) }
    }
    private static func requireReady(_ operation: NativeLocalZellijOperation, admitted: NativeLocalZellijBundle,
                                     coordinator: ApplicationCoordinator) throws {
        guard operation.inputReady, operation.state == "input_ready", operation.error == nil,
              operation.profileId == admitted.profileID, operation.epoch == coordinator.snapshot?.localZellijEpoch,
              operation.ownerUuid != nil, operation.attachmentId != nil, operation.frontendPid != nil,
              operation.checkpointRevision != nil, operation.executionContractVersion == 2 else {
            throw failure("Actual native owner omitted typed readiness/checkpoint correlation")
        }
    }
    private static func requireJoined(_ operation: NativeLocalZellijOperation, completed: Bool, exit code: Int? = nil) throws {
        guard operation.sessionJoined, operation.monitorJoined, operation.namespaceCleaned, operation.error == nil else {
            throw failure("Actual native frontend did not physically join and clean its namespace")
        }
        if completed {
            guard operation.finalFrameReceived, operation.completionSavedRevision != nil, operation.receiptDigest != nil,
                  operation.executionContractVersion == 2, operation.workloadExit?.kind == .exited,
                  operation.workloadExit?.value == code else { throw failure("Actual native completion was not saved with its authentic exit") }
        }
    }
    private static func owner(_ admitted: NativeLocalZellijBundle, id: UUID) async throws -> Owner {
        try await Task.detached {
            let data = try NativeLocalZellijBundle.bytes(admitted.profileRoot.appendingPathComponent("registry.json"), limit: 262_144)
            let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
            guard let value = try decoder.decode([Owner].self, from: data).first(where: { $0.id == id }),
                  value.server.executable == admitted.configuration.installation.zellij,
                  value.supervisor.executable == admitted.configuration.installation.cli else {
                throw failure("Actual owner does not bind final-bundle dependency and supervisor images")
            }
            return value
        }.value
    }
    private static func operation(_ coordinator: ApplicationCoordinator, excluding: Set<UInt64> = [], id: UInt64? = nil,
                                  until predicate: (NativeLocalZellijOperation) -> Bool) async throws -> NativeLocalZellijOperation {
        var result: NativeLocalZellijOperation?
        try await wait(coordinator, "correlated native local Zellij operation") { state in
            result = state.localZellijOperations.last { !excluding.contains($0.operationId) && (id == nil || $0.operationId == id) }
            return result.map(predicate) == true
        }
        guard let result else { throw failure("Native operation disappeared") }
        return result
    }
    private static func surfaces(_ state: DesktopSnapshot) -> [Surface] { state.model.workspaces.flatMap { $0.root.panes }.flatMap(\.surfaces) }
    private static func wait(_ coordinator: ApplicationCoordinator, _ label: String,
                             until predicate: (DesktopSnapshot) -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(15)
        while ContinuousClock.now < deadline {
            if let state = coordinator.snapshot, predicate(state) { return }
            try await Task.sleep(for: .milliseconds(20))
        }
        let detail = coordinator.snapshot?.localZellijOperations.map { "\($0.operationId):\($0.state):\($0.error ?? "")" }.joined(separator: "; ") ?? "no snapshot"
        throw failure("Timed out awaiting " + label + ": " + detail)
    }
    private static func file(_ url: URL, equals expected: String) async throws {
        try await Task.detached {
            let deadline = ContinuousClock.now + .seconds(10)
            while ContinuousClock.now < deadline {
                if FileManager.default.fileExists(atPath: url.path),
                   String(decoding: try NativeLocalZellijBundle.bytes(url, limit: 4096), as: UTF8.self) == expected { return }
                try await Task.sleep(for: .milliseconds(20))
            }
            throw failure("Native workload oracle differs: " + url.lastPathComponent)
        }.value
    }
    private static func activateOwnedApplication() async throws -> Int32 {
        let current = NSRunningApplication.current
        let bundle = Bundle.main.bundleURL.standardizedFileURL
        guard current.bundleURL?.standardizedFileURL == bundle, let identifier = current.bundleIdentifier,
              NSWorkspace.shared.runningApplications.filter({ $0.bundleIdentifier == identifier }).map(\.processIdentifier) == [current.processIdentifier] else {
            throw failure("LaunchServices does not identify the unique owned fixture app")
        }
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true; configuration.createsNewApplicationInstance = false
        configuration.allowsRunningApplicationSubstitution = false; configuration.arguments = ["--cli", "--help"]
        return try await withCheckedThrowingContinuation { continuation in
            NSWorkspace.shared.openApplication(at: bundle, configuration: configuration) { @Sendable application, error in
                if let error { continuation.resume(throwing: error) }
                else if let application { continuation.resume(returning: application.processIdentifier) }
                else { continuation.resume(throwing: BridgeFailure(message: "Native readiness activation returned no process")) }
            }
        }
    }
    private static func save(_ value: Evidence, _ report: URL) async throws {
        try await Task.detached {
            let data = try JSONEncoder().encode(value)
            guard data.count <= 65_536 else { throw failure("Native readiness report exceeds its byte bound") }
            try data.write(to: report, options: .atomic)
        }.value
    }
    private nonisolated static func quote(_ value: String) -> String { "'" + value.replacingOccurrences(of: "'", with: "'\\''") + "'" }
    private nonisolated static func failure(_ message: String) -> BridgeFailure { BridgeFailure(message: message) }
}

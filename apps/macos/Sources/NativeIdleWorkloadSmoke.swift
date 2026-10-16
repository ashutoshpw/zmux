import AppKit

private struct IdleWorkloadConfiguration: Codable, Sendable {
    let sourceSHA: String
    let nonce: String
    let repetitions: Int
}

private struct IdleWorkloadChild: Codable, Sendable {
    let surfaceID: UInt64
    let sessionID: UInt64
    let processID: UInt32
}

private struct IdleWorkloadPhase: Codable, Sendable {
    let name: String
    let repetition: Int
    let stage: String
    let sequence: Int
    let wallUnixSeconds: Double
    let children: [IdleWorkloadChild]
}

private struct IdleWorkloadReport: Codable, Sendable {
    let schemaVersion: Int
    let sourceSHA: String
    let nonce: String
    let appPID: Int32
    let engine: String
    var state: String
    var phases: [IdleWorkloadPhase]
    var allChildren: [IdleWorkloadChild]
}

/// Explicit idle workload ownership through normal coordinator actions. Resource
/// observations belong to the external sampler; these are not product budgets.
@MainActor
enum NativeIdleWorkloadSmoke {
    static func run(coordinator: ApplicationCoordinator, report: URL) async throws {
        guard let path = ProcessInfo.processInfo.environment["ZMUX_IDLE_WORKLOAD_ROOT"],
              ProcessInfo.processInfo.environment["ZMUX_TERMINAL_ENGINE"] == nil else {
            throw failure("Idle workload requires its private root and the default engine")
        }
        let root = URL(fileURLWithPath: path, isDirectory: true)
        let configuration = try await Task.detached {
            let data = try Data(contentsOf: root.appendingPathComponent("configuration.json"))
            guard data.count <= 4096 else { throw BridgeFailure(message: "Idle configuration exceeds its byte bound") }
            return try JSONDecoder().decode(IdleWorkloadConfiguration.self, from: data)
        }.value
        let source = Bundle.main.object(forInfoDictionaryKey: "ZmuxSourceSHA") as? String ?? ""
        guard configuration.sourceSHA == source,
              UUID(uuidString: configuration.nonce) != nil,
              (1...3).contains(configuration.repetitions) else {
            throw failure("Idle configuration source, nonce or repetition bound is invalid")
        }
        let outerDeadline = ContinuousClock.now + .seconds(1200)
        var result = IdleWorkloadReport(schemaVersion: 1, sourceSHA: source,
                                       nonce: configuration.nonce, appPID: ProcessInfo.processInfo.processIdentifier,
                                       engine: "vt100", state: "running", phases: [], allChildren: [])
        try await save(result, to: report)
        try await wait(coordinator, until: outerDeadline) { value in
            value.model.workspaces.count == 1 && value.workspace?.root.panes.count == 1
                && value.workspace?.root.panes.first?.surfaces.count == 1
                && readyChild(value, nonce: configuration.nonce) != nil
        }
        guard let initial = coordinator.snapshot,
              let pane = initial.workspace?.root.panes.first,
              let first = readyChild(initial, nonce: configuration.nonce) else {
            throw failure("Idle fixture did not obtain its first exact terminal child")
        }
        result.allChildren.append(first)
        var live = [first]

        for repetition in 1...configuration.repetitions {
            if live.count > 1 {
                for child in live.dropFirst() {
                    coordinator.enqueue(.surface("close_surface", child.surfaceID, nil, nil, nil))
                    try await wait(coordinator, until: outerDeadline) { value in
                        value.workspace?.root.panes.first?.surfaces.contains(where: { $0.id == child.surfaceID }) == false
                    }
                }
                live = [first]
                coordinator.selectTab(first.surfaceID)
                try await wait(coordinator, until: outerDeadline) { value in
                    value.workspace?.root.panes.first?.selectedSurface == first.surfaceID
                        && readyChild(value, nonce: configuration.nonce)?.processID == first.processID
                }
            }
            try await phase("idle1", repetition: repetition, children: live, result: &result,
                            coordinator: coordinator, report: report, deadline: outerDeadline)
            for count in 2...8 {
                coordinator.enqueue(.surface("create_surface", nil, pane.id, nil, nil))
                let previous = Set(live.map(\.surfaceID))
                try await wait(coordinator, until: outerDeadline) { value in
                    value.workspace?.root.panes.first?.surfaces.count == count
                        && readyChild(value, nonce: configuration.nonce).map { !previous.contains($0.surfaceID) } == true
                }
                guard let snapshot = coordinator.snapshot, let child = readyChild(snapshot, nonce: configuration.nonce) else {
                    throw failure("New idle tab did not retain its exact child identity")
                }
                live.append(child)
                result.allChildren.append(child)
            }
            coordinator.selectTab(first.surfaceID)
            try await wait(coordinator, until: outerDeadline) { value in
                value.workspace?.root.panes.first?.selectedSurface == first.surfaceID
                    && readyChild(value, nonce: configuration.nonce)?.processID == first.processID
            }
            try await phase("idle8", repetition: repetition, children: live, result: &result,
                            coordinator: coordinator, report: report, deadline: outerDeadline)
        }
        result.state = "completed_phases"
        try await save(result, to: report)
        try await coordinator.shutdown()
        result.state = "complete"
        try await save(result, to: report)
    }

    private static func phase(_ name: String, repetition: Int, children: [IdleWorkloadChild],
                              result: inout IdleWorkloadReport, coordinator: ApplicationCoordinator,
                              report: URL, deadline: ContinuousClock.Instant) async throws {
        for (stage, seconds) in [("warmup", 30), ("measure", 120), ("complete", 0)] {
            guard ContinuousClock.now < deadline, result.phases.count < 18 else {
                throw failure("Idle workload exceeded its fixed outer/phase bound")
            }
            result.phases.append(IdleWorkloadPhase(name: name, repetition: repetition, stage: stage,
                                                   sequence: result.phases.count + 1,
                                                   wallUnixSeconds: Date().timeIntervalSince1970, children: children))
            try await save(result, to: report)
            let phaseDeadline = min(deadline, ContinuousClock.now + .seconds(seconds))
            while ContinuousClock.now < phaseDeadline {
                guard let snapshot = coordinator.snapshot,
                      snapshot.model.workspaces.count == 1,
                      snapshot.workspace?.root.panes.count == 1,
                      snapshot.workspace?.root.panes.first?.surfaces.count == children.count,
                      snapshot.frames.count == 1,
                      snapshot.frames.first?.processId == children.first?.processID,
                      snapshot.frames.first?.snapshot?.state == "running",
                      snapshot.lastError == nil else {
                    throw failure("Idle workload changed terminal ownership or lost its visible child")
                }
                try await Task.sleep(for: .seconds(1))
            }
            guard ContinuousClock.now < deadline else { throw failure("Idle workload exhausted its outer deadline") }
        }
    }

    private static func readyChild(_ value: DesktopSnapshot, nonce: String) -> IdleWorkloadChild? {
        guard let pane = value.workspace?.root.panes.first,
              let session = pane.sessionId,
              let frame = value.frames.first(where: { $0.sessionId == session }),
              let pid = frame.processId,
              frame.snapshot?.state == "running",
              frame.snapshot?.viewport.contents.contains("IDLE_\(nonce)_\(pid)") == true else { return nil }
        return IdleWorkloadChild(surfaceID: pane.selectedSurface, sessionID: session, processID: pid)
    }

    private static func wait(_ coordinator: ApplicationCoordinator, until outer: ContinuousClock.Instant,
                             predicate: (DesktopSnapshot) -> Bool) async throws {
        let deadline = min(outer, ContinuousClock.now + .seconds(10))
        while ContinuousClock.now < deadline {
            if let value = coordinator.snapshot {
                if predicate(value) { return }
                if let error = value.lastError { throw failure(error) }
            }
            try await Task.sleep(for: .milliseconds(32))
        }
        throw failure("Idle workload did not reach its bounded production snapshot")
    }

    private static func save(_ value: IdleWorkloadReport, to url: URL) async throws {
        try await Task.detached {
            let data = try JSONEncoder().encode(value)
            guard data.count <= 131_072 else { throw BridgeFailure(message: "Idle report exceeds its byte bound") }
            try data.write(to: url, options: .atomic)
        }.value
    }

    private static func failure(_ message: String) -> BridgeFailure {
        BridgeFailure(message: "Native idle workload: " + message)
    }
}

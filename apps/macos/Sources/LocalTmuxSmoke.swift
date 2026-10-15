import AppKit
import Foundation

/// Packaged acceptance uses the public CLI, a real tmux owner and the mounted
/// terminal's input/rendering paths. It never drives tmux through a mock backend.
enum LocalTmuxSmoke {
    private struct Identity: Decodable, Equatable, Sendable {
        let id: UUID
        let serverIncarnation: UUID
        let tmuxSessionId: String
        let createdAt: UInt64
    }
    private struct Receipt: Decodable, Sendable {
        let state: String
        let identity: Identity?
        let surface: UUID?
        let reused: Bool
        let ownerOutcome: String
    }
    private struct Client: Decodable, Sendable { let processId: UInt32 }
    private struct Session: Decodable, Sendable {
        let id: UUID
        let name: String
        let serverIncarnation: UUID
        let tmuxSessionId: String
        let createdAt: UInt64
        let state: String
        let clients: [Client]
        var identity: Identity {
            Identity(id: id, serverIncarnation: serverIncarnation,
                     tmuxSessionId: tmuxSessionId, createdAt: createdAt)
        }
    }
    private struct Status: Decodable, Sendable { let session: Session }
    private struct Listing: Decodable, Sendable { let socketPath: String; let sessions: [Session] }
    private struct CreatedSurface: Decodable, Sendable { let surfaceId: UInt64; let surfaceUuid: UUID }
    private struct Reopened: Decodable, Sendable { let createdSurfaces: [CreatedSurface] }

    @MainActor
    static func validate(
        report: URL,
        cli: @escaping @MainActor ([String]) async throws -> Data,
        snapshot: @MainActor () -> DesktopSnapshot?,
        terminal: @MainActor (UInt64) -> TerminalView?
    ) async throws {
        guard let configured = ProcessInfo.processInfo.environment["ZMUX_LOCAL_TMUX_ROOT"],
              configured.hasPrefix("/") else { throw failure("Native fixture requires an explicit absolute profile root") }
        let profile = try decode(Listing.self, await cli(["local-tmux", "list"]))
        guard profile.socketPath == URL(fileURLWithPath: configured).appendingPathComponent("server.sock").path,
              profile.sessions.isEmpty else { throw failure("Refusing to spawn against an unexpected/nonempty native fixture profile") }
        guard let originalWorkspace = snapshot()?.workspace,
              let originalSurface = originalWorkspace.root.panes.first(where: { $0.id == originalWorkspace.focusedPane })?.selectedSurface else {
            throw failure("No initial workspace/terminal")
        }
        let existing = surfaceIDs(snapshot())
        let name = "native-smoke-" + UUID().uuidString.lowercased()
        let root = try await Task.detached {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent(name, isDirectory: true)
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false,
                                                    attributes: [.posixPermissions: 0o700])
            return root
        }.value
        let count = root.appendingPathComponent("startup-count")
        let ownerPID = root.appendingPathComponent("owner-pid")
        var identity: Identity?
        var targets: Set<String> = []

        @MainActor func status(_ id: UUID) async throws -> Session {
            try decode(Status.self, await cli(["local-tmux", "status", "--id", id.uuidString])).session
        }
        @MainActor func cleanup() async throws {
            var errors: [String] = []
            for target in targets {
                do { _ = try await cli(["close-surface", "--surface", target]) }
                catch { errors.append("close fixture surface: " + error.localizedDescription) }
            }
            do {
                let list = try decode(Listing.self, await cli(["local-tmux", "list"]))
                for session in list.sessions where session.name == name {
                    if let identity, session.identity != identity {
                        throw failure("Cleanup refused a replaced fixture identity")
                    }
                    _ = try await cli(["local-tmux", "close", "--id", session.id.uuidString])
                }
                let remaining = try decode(Listing.self, await cli(["local-tmux", "list"]))
                guard remaining.sessions.allSatisfy({ $0.name != name && $0.id != identity?.id }) else {
                    throw failure("Explicit fixture close left the managed owner registered")
                }
            } catch { errors.append("close owned fixture: " + error.localizedDescription) }
            do {
                _ = try await cli(["select-workspace", String(originalWorkspace.id)])
                _ = try await cli(["select-surface", String(originalSurface)])
            } catch { errors.append("restore original selection: " + error.localizedDescription) }
            if errors.isEmpty {
                try await removeFixture(root)
            } else { throw failure(errors.joined(separator: "; ")) }
        }

        do {
            let command = "printf x >> " + quote(count.path)
                + "; printf '%s' \"$$\" > " + quote(ownerPID.path + ".pending")
                + "; mv " + quote(ownerPID.path + ".pending") + " " + quote(ownerPID.path)
                + "; printf '\\nZMUX_TMUX_NATIVE_READY\\n'; exec /bin/sh -i"
            let started = try decode(Receipt.self, await cli([
                "local-tmux", "start", name, "--cwd", root.path, "--command", command,
                "--workspace", String(originalWorkspace.id)
            ]))
            guard started.state == "completed", started.ownerOutcome == "created", !started.reused,
                  let owned = started.identity, let firstTarget = started.surface else {
                throw failure("Start did not return a completed created-owner receipt")
            }
            identity = owned
            targets.insert(firstTarget.uuidString)
            try await waitUntil {
                guard let state = snapshot(), let pane = selectedPane(state), let session = pane.sessionId else { return false }
                return !existing.contains(pane.selectedSurface) && terminal(session)?.window != nil
                    && state.frames.contains { $0.sessionId == session && $0.snapshot?.viewport.contents.contains("ZMUX_TMUX_NATIVE_READY") == true }
            }
            guard let initial = snapshot(), let pane = selectedPane(initial), let session = pane.sessionId,
                  let view = terminal(session) else { throw failure("Attached client has no mounted native terminal") }
            let firstSurface = pane.selectedSurface
            let live = try await status(owned.id)
            try requireClient(live, identity: owned, session: session, snapshot: initial)
            let originalPID = try await waitForFile(ownerPID)
            guard UInt32(originalPID) != nil, try await waitForFile(count) == "x" else {
                throw failure("Startup did not publish exactly one shell identity")
            }
            try await inputProbe(view: view, root: root, suffix: "first", pid: originalPID,
                                 session: session, snapshot: snapshot)

            let reused = try decode(Receipt.self, await cli(["tmux", "attach", "--id", owned.id.uuidString]))
            guard reused.state == "completed", reused.reused, reused.identity == owned,
                  reused.surface == firstTarget else { throw failure("Alias attach did not reuse the exact existing surface/identity") }
            try requireClient(try await status(owned.id), identity: owned, session: session, snapshot: snapshot())

            _ = try await cli(["close-surface", "--surface", firstTarget.uuidString])
            targets.remove(firstTarget.uuidString)
            try await waitUntil { !surfaceIDs(snapshot()).contains(firstSurface) }
            try await waitUntil { try await status(owned.id).clients.isEmpty }
            guard try await status(owned.id).state == "live" else { throw failure("Closing the native client killed its durable owner") }
            let reopenedReceipt = try decode(Reopened.self, await cli(["reopen-closed"]))
            guard reopenedReceipt.createdSurfaces.count == 1, let created = reopenedReceipt.createdSurfaces.first,
                  created.surfaceId != firstSurface, !existing.contains(created.surfaceId) else {
                throw failure("Reopen omitted the single attributable replacement surface")
            }
            let reopenedTarget = created.surfaceUuid.uuidString
            targets.insert(reopenedTarget)
            try await waitUntil {
                guard let state = snapshot(), let pane = selectedPane(state), let session = pane.sessionId else { return false }
                guard pane.selectedSurface == created.surfaceId else { return false }
                return terminal(session)?.window != nil
                    && state.frames.contains { $0.sessionId == session && $0.snapshot?.viewport.contents.contains("ZMUX_TMUX_NATIVE_FIRST") == true }
            }
            guard let reopened = snapshot(), let reopenedPane = selectedPane(reopened),
                  let reopenedSession = reopenedPane.sessionId, let reopenedView = terminal(reopenedSession) else {
                throw failure("Reopened owner has no mounted native terminal")
            }
            guard reopenedPane.selectedSurface == created.surfaceId else { throw failure("Reopen selection changed before verification") }
            guard reopenedSession != session else { throw failure("Reopen reused the closed PTY client") }
            try requireClient(try await status(owned.id), identity: owned, session: reopenedSession, snapshot: reopened)
            guard try await waitForFile(ownerPID) == originalPID, try await waitForFile(count) == "x" else {
                throw failure("Reopen changed the durable shell PID or replayed startup")
            }
            try await inputProbe(view: reopenedView, root: root, suffix: "reopened", pid: originalPID,
                                 session: reopenedSession, snapshot: snapshot)
            guard reopenedView.bounds.width > 20, reopenedView.bounds.height > 20,
                  let bitmap = reopenedView.bitmapImageRepForCachingDisplay(in: reopenedView.bounds) else {
                throw failure("Reopened terminal has no native drawing surface")
            }
            reopenedView.displayIfNeeded()
            reopenedView.effectiveAppearance.performAsCurrentDrawingAppearance {
                reopenedView.cacheDisplay(in: reopenedView.bounds, to: bitmap)
            }
            guard let png = bitmap.representation(using: .png, properties: [:]), png.count > 4096 else {
                throw failure("Reopened tmux terminal did not render a usable PNG")
            }
            let writing = Task<Void, Error>.detached { try png.write(to: report, options: .atomic) }
            try await writing.value
            _ = try await cli(["close-surface", "--surface", reopenedTarget])
            targets.remove(reopenedTarget)
            try await waitUntil { try await status(owned.id).clients.isEmpty }
            guard try await status(owned.id).identity == owned,
                  try await waitForFile(ownerPID) == originalPID, try await waitForFile(count) == "x" else {
                throw failure("Final client close changed the durable owner")
            }
        } catch {
            let primary = error
            do { try await cleanup() }
            catch { throw failure(primary.localizedDescription + "; cleanup failed: " + error.localizedDescription) }
            throw primary
        }
        try await cleanup()
    }

    @MainActor
    private static func inputProbe(view: TerminalView, root: URL, suffix: String, pid: String,
                                   session: UInt64, snapshot: @MainActor () -> DesktopSnapshot?) async throws {
        let path = root.appendingPathComponent("input-" + suffix)
        let marker = "ZMUX_TMUX_NATIVE_" + suffix.uppercased()
        view.insertText("printf '%s' \"$$\" > " + quote(path.path + ".pending")
                        + "; mv " + quote(path.path + ".pending") + " " + quote(path.path)
                        + "; printf '\\n" + marker + "\\n'\r",
                        replacementRange: NSRange(location: NSNotFound, length: 0))
        guard try await waitForFile(path) == pid else { throw failure("Native input reached a different shell after " + suffix) }
        try await waitUntil {
            snapshot()?.frames.contains {
                $0.sessionId == session && $0.snapshot?.viewport.contents.components(separatedBy: "\n").contains(marker) == true
            } == true
        }
    }

    private static func decode<T: Decodable>(_ type: T.Type, _ data: Data) throws -> T {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(type, from: data)
    }
    private static func requireClient(_ status: Session, identity: Identity, session: UInt64,
                                      snapshot: DesktopSnapshot?) throws {
        guard status.identity == identity, status.state == "live", status.clients.count == 1,
              let frame = snapshot?.frames.first(where: { $0.sessionId == session }), !frame.starting,
              frame.failure == nil, let process = frame.processId, process == status.clients[0].processId else {
            throw failure("Native PTY process does not match the single real tmux client/owner identity")
        }
    }
    private static func selectedPane(_ snapshot: DesktopSnapshot) -> Pane? {
        guard let workspace = snapshot.workspace else { return nil }
        return workspace.root.panes.first { $0.id == workspace.focusedPane }
    }
    private static func surfaceIDs(_ snapshot: DesktopSnapshot?) -> Set<UInt64> {
        Set(snapshot?.model.workspaces.flatMap { $0.root.panes.flatMap { $0.surfaces.map(\.id) } } ?? [])
    }
    @MainActor
    private static func waitUntil(line: UInt = #line, _ predicate: @MainActor () async throws -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(15)
        while ContinuousClock.now < deadline {
            if try await predicate() { return }
            try await Task.sleep(for: .milliseconds(30))
        }
        throw failure("Bounded native condition at line \(line) timed out")
    }
    private static func waitForFile(_ path: URL) async throws -> String {
        try await Task.detached {
            let deadline = ContinuousClock.now + .seconds(15)
            while ContinuousClock.now < deadline {
                if FileManager.default.fileExists(atPath: path.path) {
                    return try String(contentsOf: path, encoding: .utf8)
                }
                try await Task.sleep(for: .milliseconds(30))
            }
            throw failure("Fixture file was not published: " + path.lastPathComponent)
        }.value
    }
    private static func removeFixture(_ root: URL) async throws {
        let removal = Task<Void, Error>.detached { try FileManager.default.removeItem(at: root) }
        try await removal.value
    }
    private static func quote(_ value: String) -> String { "'" + value.replacingOccurrences(of: "'", with: "'\\''") + "'" }
    private static func failure(_ message: String) -> BridgeFailure { BridgeFailure(message: "Local tmux native smoke: " + message) }
}

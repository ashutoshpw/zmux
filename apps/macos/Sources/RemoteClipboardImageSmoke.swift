import AppKit
import Darwin
import Foundation

/// Combined native acceptance through the real managed SSH connection. Fixture
/// controls live exclusively in the existing packaged smoke entrypoint.
enum RemoteClipboardImageSmoke {
    private struct Configuration: Decodable, Sendable {
        let root: String
        let clientConfig: String
        let identity: String
        let uploads: String
    }

    @MainActor
    static func validate(
        configuration: URL,
        snapshot: @MainActor () -> DesktopSnapshot?,
        terminal: @MainActor (UInt64) -> TerminalView?
    ) async throws {
        let config = try await Task.detached {
            try JSONDecoder().decode(Configuration.self, from: Data(contentsOf: configuration))
        }.value
        let root = URL(fileURLWithPath: config.root, isDirectory: true)
        guard let cli = Bundle.main.resourceURL?.appendingPathComponent("bin/zmux"),
              let original = snapshot()?.model.focusedWorkspace else {
            throw BridgeFailure(message: "Remote image smoke has no bundled CLI or initial workspace")
        }
        let fixture = try await Task.detached { try ClipboardImageSmoke.makeFixturePNG() }.value
        let before = try await Task.detached { try cachedImages() }.value
        let startup = "printf '\\nZMUX_REMOTE_IMAGE_READY\\n'; while IFS= read -r pasted; do eval \"set -- $pasted\"; printf '%s' \"$1\" > "
            + quote(root.appendingPathComponent("inserted-path.pending").path)
            + "; mv " + quote(root.appendingPathComponent("inserted-path.pending").path) + " " + quote(root.appendingPathComponent("inserted-path").path)
            + "; printf '\\nZMUX_REMOTE_IMAGE_PATH:%s\\n' \"$1\"; done"
        _ = try await Task.detached {
            try run(cli, arguments: ["ssh", "fixture", "--ssh-config", config.clientConfig,
                                     "--identity", config.identity, "--name", "Native image acceptance",
                                     "--command", startup], root: root)
        }.value

        var remoteWorkspace: UInt64?
        do {
            try await waitUntil {
                guard let value = snapshot(), let workspace = value.workspace,
                      workspace.id != original, let pane = workspace.root.panes.first,
                      pane.selected.remote != nil, let session = pane.sessionId else { return false }
                return value.frames.contains {
                    $0.sessionId == session && $0.snapshot?.viewport.contents.components(separatedBy: "\n")
                        .contains("ZMUX_REMOTE_IMAGE_READY") == true
                } && terminal(session) != nil
            }
            guard let workspace = snapshot()?.workspace, let pane = workspace.root.panes.first,
                  let session = pane.sessionId, let view = terminal(session) else {
                throw BridgeFailure(message: "Managed SSH terminal did not reach the native view")
            }
            remoteWorkspace = workspace.id

            // Fresh authentication must now fail. Upload success therefore proves reuse
            // of the terminal's authenticated master, rather than an independent login.
            try await Task.detached {
                try Data().write(to: root.appendingPathComponent("authorized_keys"))
                let output = try run(URL(fileURLWithPath: "/usr/bin/ssh"), arguments: [
                    "-F", config.clientConfig, "-i", config.identity, "fixture", "true"
                ], root: root, expectedSuccess: false)
                guard String(decoding: output, as: UTF8.self).contains("Permission denied") else {
                    throw BridgeFailure(message: "Fresh SSH authentication failed for an unexpected reason")
                }
            }.value

            let previousUploads = Set(snapshot()?.uploads.map(\.id) ?? [])
            let pasteboard = NSPasteboard.withUniqueName()
            defer { pasteboard.releaseGlobally() }
            guard pasteboard.setData(fixture, forType: .png),
                  pasteboard.setString("REMOTE_IMAGE_TEXT_MUST_NOT_BE_PASTED", forType: .string),
                  view.paste(from: pasteboard) else {
                throw BridgeFailure(message: "Actual managed SSH terminal rejected the image paste")
            }
            _ = try await Task.detached {
                try run(cli, arguments: ["select-workspace", String(original)], root: root)
            }.value
            try await waitUntil { snapshot()?.model.focusedWorkspace == original }
            try await waitUntil {
                snapshot()?.uploads.contains { !previousUploads.contains($0.id) && $0.surfaceId == pane.selectedSurface } == true
            }
            guard let admitted = snapshot()?.uploads.first(where: { !previousUploads.contains($0.id) }) else {
                throw BridgeFailure(message: "Image paste created no attributable remote upload")
            }
            try await waitForFile(root.appendingPathComponent("ack-ready"))

            let observed = try await Task.detached {
                let sources = try cachedImages().subtracting(before)
                guard sources.count == 1, let source = sources.first else {
                    throw BridgeFailure(message: "Remote image paste did not retain exactly one new prepared source")
                }
                let ack = try Data(contentsOf: root.appendingPathComponent("ack"))
                guard ack.last == 0, let remotePath = String(data: ack.dropLast(), encoding: .utf8),
                      URL(fileURLWithPath: remotePath).deletingLastPathComponent().deletingLastPathComponent().path == config.uploads else {
                    throw BridgeFailure(message: "Remote image acknowledgement escaped the isolated uploads directory")
                }
                let bytes = try Data(contentsOf: source)
                guard !bytes.isEmpty, bytes == (try Data(contentsOf: URL(fileURLWithPath: remotePath))) else {
                    throw BridgeFailure(message: "Remote SSH image bytes differ from the prepared native PNG")
                }
                guard !FileManager.default.fileExists(atPath: root.appendingPathComponent("inserted-path").path) else {
                    throw BridgeFailure(message: "Image paste submitted the remote shell command")
                }
                return (source, remotePath, bytes.count)
            }.value
            guard let held = snapshot()?.uploads.first(where: { $0.id == admitted.id }),
                  !held.sourceReleased, held.delivery == "pending", held.sessionId == session else {
                throw BridgeFailure(message: "Prepared source was released before its live SSH worker completed")
            }
            try await Task.detached {
                try Data().write(to: root.appendingPathComponent("release-ack"))
            }.value
            try await waitUntil {
                snapshot()?.uploads.contains {
                    $0.id == admitted.id && $0.state == "completed" && $0.delivery == "inserted" && $0.sourceReleased
                } == true
            }
            guard let complete = snapshot()?.uploads.first(where: { $0.id == admitted.id }),
                  complete.remotePaths == [observed.1], complete.bytesSent == UInt64(observed.2),
                  !complete.remoteResultUncertain, complete.error == nil, complete.deliveryError == nil else {
                throw BridgeFailure(message: "Remote image transfer receipt did not match independently observed bytes/path")
            }
            try await waitForMissing(observed.0)
            try await waitForMissing(observed.0.deletingLastPathComponent())
            guard snapshot()?.model.focusedWorkspace == original,
                  snapshot()?.frames.allSatisfy({ $0.snapshot?.viewport.contents.contains("zmux-upload.") != true }) == true else {
                throw BridgeFailure(message: "Remote image delivery changed focus or inserted into the newly focused terminal")
            }
            _ = try await Task.detached {
                try run(cli, arguments: ["select-workspace", String(workspace.id)], root: root)
            }.value
            try await waitUntil {
                snapshot()?.model.focusedWorkspace == workspace.id && snapshot()?.frames.contains {
                    $0.sessionId == session && $0.snapshot?.viewport.contents.contains("zmux-upload.") == true
                } == true
            }
            try await Task.detached {
                guard !FileManager.default.fileExists(atPath: root.appendingPathComponent("inserted-path").path) else {
                    throw BridgeFailure(message: "Remote image insertion submitted the shell before explicit Return")
                }
            }.value

            // Only this explicit Return submits the inserted quoted path. The remote
            // read loop independently parses it, preserving spaces, apostrophe and UTF-8.
            view.insertText("\r", replacementRange: NSRange(location: NSNotFound, length: 0))
            try await waitForFile(root.appendingPathComponent("inserted-path"))
            try await Task.detached {
                let received = try String(contentsOf: root.appendingPathComponent("inserted-path"), encoding: .utf8)
                guard received == observed.1 else {
                    throw BridgeFailure(message: "Native image insertion changed the remote shell path")
                }
            }.value
            guard snapshot()?.frames.first(where: { $0.sessionId == session })?.snapshot?.viewport.contents
                .contains("REMOTE_IMAGE_TEXT_MUST_NOT_BE_PASTED") == false else {
                throw BridgeFailure(message: "Remote image paste leaked its text representation into the terminal")
            }
            try await close(workspace: workspace.id, original: original, cli: cli, root: root)
            try await waitUntil { snapshot()?.model.focusedWorkspace == original }
        } catch {
            let primary = error
            if let remoteWorkspace {
                do { try await close(workspace: remoteWorkspace, original: original, cli: cli, root: root) }
                catch { throw BridgeFailure(message: "\(primary.localizedDescription); remote smoke cleanup failed: \(error.localizedDescription)") }
            }
            throw primary
        }
    }

    private static func close(workspace: UInt64, original: UInt64, cli: URL, root: URL) async throws {
        _ = try await Task.detached {
            _ = try run(cli, arguments: ["close-workspace", String(workspace)], root: root)
            return try run(cli, arguments: ["select-workspace", String(original)], root: root)
        }.value
    }

    private static func cachedImages() throws -> Set<URL> {
        let cache = try FileManager.default.url(for: .cachesDirectory, in: .userDomainMask, appropriateFor: nil, create: false)
            .appendingPathComponent("app.w3dev.zmux.dev/ClipboardImages", isDirectory: true)
        guard FileManager.default.fileExists(atPath: cache.path) else { return [] }
        return Set(try FileManager.default.contentsOfDirectory(at: cache, includingPropertiesForKeys: nil)
            .filter { $0.lastPathComponent != ".lock" }
            .map { $0.appendingPathComponent("image.png") })
    }

    private static func quote(_ text: String) -> String { "'" + text.replacingOccurrences(of: "'", with: "'\\''") + "'" }

    @MainActor
    private static func waitUntil(_ predicate: @MainActor () -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(20)
        while ContinuousClock.now < deadline {
            if predicate() { return }
            try await Task.sleep(for: .milliseconds(32))
        }
        throw BridgeFailure(message: "Combined native remote image acceptance timed out")
    }

    private static func waitForFile(_ url: URL) async throws { try await waitForExistence(url, exists: true) }
    private static func waitForMissing(_ url: URL) async throws { try await waitForExistence(url, exists: false) }

    private static func waitForExistence(_ url: URL, exists: Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(20)
        while ContinuousClock.now < deadline {
            if await Task.detached(operation: { FileManager.default.fileExists(atPath: url.path) }).value == exists { return }
            try await Task.sleep(for: .milliseconds(32))
        }
        throw BridgeFailure(message: "Remote image fixture file did not become \(exists ? "present" : "absent"): \(url.lastPathComponent)")
    }

    /// Runs only on detached workers. Files avoid pipe backpressure and subprocess
    /// lifetime is bounded even if a broken control socket never answers the CLI.
    private static func run(_ executable: URL, arguments: [String], root: URL, expectedSuccess: Bool = true) throws -> Data {
        let outputURL = root.appendingPathComponent("process-\(UUID().uuidString).log")
        guard FileManager.default.createFile(atPath: outputURL.path, contents: nil, attributes: [.posixPermissions: 0o600]) else {
            throw BridgeFailure(message: "Could not create remote smoke process output")
        }
        defer {
            do { try FileManager.default.removeItem(at: outputURL) }
            catch { FileHandle.standardError.write(Data("Remote smoke process-output cleanup failed: \(error.localizedDescription)\n".utf8)) }
        }
        let output = try FileHandle(forWritingTo: outputURL)
        defer {
            do { try output.close() }
            catch { FileHandle.standardError.write(Data("Remote smoke output close failed: \(error.localizedDescription)\n".utf8)) }
        }
        let process = Process()
        process.executableURL = executable
        process.arguments = arguments
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = output
        process.standardError = output
        try process.run()
        let deadline = ContinuousClock.now + .seconds(12)
        while process.isRunning && ContinuousClock.now < deadline { Thread.sleep(forTimeInterval: 0.01) }
        if process.isRunning {
            process.terminate()
            let grace = ContinuousClock.now + .seconds(1)
            while process.isRunning && ContinuousClock.now < grace { Thread.sleep(forTimeInterval: 0.01) }
            if process.isRunning { Darwin.kill(process.processIdentifier, SIGKILL) }
            process.waitUntilExit()
            throw BridgeFailure(message: "Remote smoke subprocess timed out: \(executable.lastPathComponent)")
        }
        process.waitUntilExit()
        let data = try Data(contentsOf: outputURL)
        guard (process.terminationStatus == 0) == expectedSuccess else {
            throw BridgeFailure(message: "Remote smoke subprocess returned \(process.terminationStatus): \(String(decoding: data.prefix(4096), as: UTF8.self))")
        }
        return data
    }
}

import AppKit
import Darwin
import WebKit

/// Actual WKDownload conversion, native save sheet actions and bundled CLI receipts.
/// Fixture DOM clicks exercise conversion only; they do not claim pointer parity.
@MainActor
enum BrowserDownloadSmoke {
    private struct Created: Decodable {
        struct Target: Decodable { let surfaceId: UInt64 }
        let createdSurfaces: [Target]
    }
    private struct Event: Decodable { let download: BrowserDownloadRecord }
    private struct Listing: Decodable { let downloads: [BrowserDownloadRecord]; let count: Int }
    private static func decode<T: Decodable>(_ type: T.Type, _ data: Data) throws -> T {
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(type, from: data)
    }
    private static func wait(_ message: String, _ predicate: () -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(8)
        while !predicate() {
            guard ContinuousClock.now < deadline else { throw BridgeFailure(message: message) }
            try await Task.sleep(for: .milliseconds(25))
        }
    }
    private static func chooseDestination(_ destination: URL, panel: NSSavePanel) async throws {
        // Apple explicitly disallows programmatic NSSavePanel.ok(_:) on macOS
        // 10.15+. Drive the real remote UI from a separately authorized process.
        // Selection and acceptance are separate so the app independently checks
        // the panel's actual destination before any Save action is sent.
        try await drivePanel("select", currentName: panel.nameFieldStringValue, destination: destination)
        do {
            try await wait("Save panel did not select the requested fixture destination") {
                panel.url?.path == destination.path
            }
        } catch {
            throw BridgeFailure(message: "Save panel selected " + (panel.url?.path ?? "nothing")
                                + "; expected " + destination.path)
        }
        FileHandle.standardError.write(Data(("Confirmed download save panel URL: " + destination.path + "\n").utf8))
        try await drivePanel("accept", currentName: destination.lastPathComponent, destination: destination)
    }
    private static func drivePanel(_ phase: String, currentName: String, destination: URL) async throws {
        guard let app = Bundle.main.executableURL else { throw BridgeFailure(message: "Native save driver cannot identify its exact app") }
        let arguments = [phase, String(getpid()), app.path, currentName, destination.deletingLastPathComponent().path, destination.lastPathComponent]
        let output = destination.deletingLastPathComponent().appendingPathComponent("ax-" + phase + "-" + destination.lastPathComponent + ".log")
        try await runDriver(arguments, output: output)
    }
    private static func preflight(report: URL) async throws {
        guard let app = Bundle.main.executableURL else { throw BridgeFailure(message: "Native save driver cannot identify its exact app") }
        let output = report.deletingLastPathComponent().appendingPathComponent("native-download-preflight.log")
        try await runDriver(["preflight", String(getpid()), app.path], output: output)
    }
    private static func runDriver(_ arguments: [String], output: URL) async throws {
        let executable = Bundle.main.bundleURL.deletingLastPathComponent().appendingPathComponent("native-save-panel-driver")
        try await Task.detached {
            guard FileManager.default.isExecutableFile(atPath: executable.path),
                  FileManager.default.createFile(atPath: output.path, contents: nil, attributes: [.posixPermissions: 0o600]) else {
                throw BridgeFailure(message: "Native download acceptance gated: external save-panel UI driver or its private log is unavailable")
            }
            let log = try FileHandle(forWritingTo: output)
            defer {
                do { try log.close() }
                catch { FileHandle.standardError.write(Data(("Save-panel driver log cleanup: " + error.localizedDescription + "\n").utf8)) }
            }
            let process = Process()
            process.executableURL = executable
            process.arguments = arguments
            process.standardInput = FileHandle.nullDevice
            process.standardOutput = log
            process.standardError = log
            try process.run()
            let deadline = ContinuousClock.now + .seconds(15)
            while process.isRunning, ContinuousClock.now < deadline { usleep(25_000) }
            let timedOut = process.isRunning
            if timedOut {
                process.terminate()
                let stopDeadline = ContinuousClock.now + .milliseconds(500)
                while process.isRunning, ContinuousClock.now < stopDeadline { usleep(25_000) }
                if process.isRunning, Darwin.kill(process.processIdentifier, SIGKILL) != 0, errno != ESRCH {
                    throw BridgeFailure(message: "Could not stop the owned save-panel UI driver (errno " + String(errno) + ")")
                }
            }
            process.waitUntilExit()
            let reader = try FileHandle(forReadingFrom: output)
            let data: Data
            do { data = try reader.read(upToCount: 8192) ?? Data(); try reader.close() }
            catch {
                do { try reader.close() }
                catch { FileHandle.standardError.write(Data(("Save-panel driver read cleanup: " + error.localizedDescription + "\n").utf8)) }
                throw error
            }
            FileHandle.standardError.write(data)
            guard !timedOut, process.terminationReason == .exit, process.terminationStatus == 0 else {
                throw BridgeFailure(message: "Native download acceptance gated: external save-panel driver " + (arguments.first ?? "unknown") + " failed (" + String(process.terminationStatus) + "): " + String(decoding: data, as: UTF8.self))
            }
        }.value
    }
    private nonisolated static func removeFixture(_ root: URL) async throws {
        let removal = Task<Void, Error>.detached { try FileManager.default.removeItem(at: root) }
        try await removal.value
    }
    static func validate(
        origin: String, window: NSWindow, report: URL,
        cli: @escaping @MainActor ([String], Bool) async throws -> Data,
        snapshot: @MainActor () -> DesktopSnapshot?,
        browser: @MainActor (UInt64) -> BrowserView?
    ) async throws {
        // Discover an unavailable host capability before creating a browser,
        // admitting a download or presenting a sheet. This is never acceptance.
        try await preflight(report: report)
        let setup = Task<URL, Error>.detached {
            let requested = FileManager.default.temporaryDirectory
                .appendingPathComponent("zmux-native-download-" + UUID().uuidString, isDirectory: true)
            try FileManager.default.createDirectory(at: requested, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
            do {
                guard let resolved = Darwin.realpath(requested.path, nil) else {
                    throw BridgeFailure(message: "Could not resolve the native download fixture (errno " + String(errno) + ").")
                }
                defer { Darwin.free(resolved) }
                let path = String(cString: resolved)
                let root = URL(fileURLWithPath: path, isDirectory: true)
                guard root.path == path else { throw BridgeFailure(message: "File URL changed the canonical native download fixture path") }
                FileHandle.standardError.write(Data(("Native download fixture root: " + root.path + "\n").utf8))
                return root
            } catch {
                do { try FileManager.default.removeItem(at: requested) }
                catch { FileHandle.standardError.write(Data(("native download fixture setup cleanup: " + error.localizedDescription + "\n").utf8)) }
                throw error
            }
        }
        let root = try await setup.value
        var surface: UInt64?
        do {
            let created = try decode(Created.self, await cli(["browser", "open", "--no-focus", origin + "/first"], false))
            guard let id = created.createdSurfaces.first?.surfaceId else { throw BridgeFailure(message: "Download fixture omitted its browser") }
            surface = id
            try await wait("Download fixture browser did not load") { browser(id)?.webView.title == "First fixture" && browser(id)?.webView.isLoading == false }
            let target = ["--surface", String(id)]
            let expected = Data(Array("zmux-native-download".utf8) + [0, 255, 1])
            let selected = snapshot()?.workspace?.root.panes.map { $0.selectedSurface }
            // Same WebKit cookie context must survive an HTTP redirect into WKDownload.
            _ = try await cli(["browser", "eval"] + target + ["document.cookie = 'zmuxDownload=allowed; Path=/'; null"], false)
            for (index, endpoint) in ["/download-redirect", "/download-mime", "/download"].enumerated() {
                let prior = Set(snapshot()?.downloads.map(\.downloadId) ?? [])
                if index == 2 {
                    _ = try await cli(["browser", "goto"] + target + [origin + "/first"], false)
                    try await wait("Action download fixture did not load") { browser(id)?.webView.title == "First fixture" && browser(id)?.webView.isLoading == false }
                    _ = try await cli(["browser", "eval"] + target + ["const a = document.createElement('a'); a.href = '/download'; a.download = 'action.bin'; document.body.append(a); a.click(); null"], false)
                } else { _ = try await cli(["browser", "goto"] + target + [origin + endpoint], false) }
                try await wait("Native download save sheet did not open") { window.attachedSheet is NSSavePanel && snapshot()?.downloads.contains { !prior.contains($0.downloadId) } == true }
                guard let panel = window.attachedSheet as? NSSavePanel else { throw BridgeFailure(message: "Download did not use NSSavePanel") }
                let destination = root.appendingPathComponent("saved-" + String(index) + ".bin")
                try await chooseDestination(destination, panel: panel)
                try await wait("Native download never confirmed final publication") { snapshot()?.downloads.contains { !prior.contains($0.downloadId) && $0.status == "saved" && $0.cleanupComplete } == true }
                let verify = Task<Data, Error>.detached { try Data(contentsOf: destination) }
                guard try await verify.value == expected else { throw BridgeFailure(message: "WebKit saved different download bytes") }
                let list = try decode(Listing.self, await cli(["browser", "download", "list"] + target + ["--json"], false))
                guard let record = list.downloads.first, record.path == destination.path, record.bytes == UInt64(expected.count) else { throw BridgeFailure(message: "Native and CLI history disagree") }
                // Observing a final path must leave its terminal event available.
                _ = try await cli(["browser", "download", "wait"] + target + ["--path", destination.path, "--timeout-ms", "1000"], false)
                let event = try decode(Event.self, await cli(["browser", "download", "wait"] + target + ["--timeout-ms", "1000"], false))
                guard event.download.downloadId == record.downloadId, event.download.status == "saved" else { throw BridgeFailure(message: "Path wait consumed or retargeted the download event") }
            }
            // The real panel's Cancel action must acknowledge a terminal cancellation.
            var prior = Set(snapshot()?.downloads.map(\.downloadId) ?? [])
            _ = try await cli(["browser", "goto"] + target + [origin + "/download"], false)
            try await wait("Cancellation save sheet did not open") { window.attachedSheet is NSSavePanel }
            (window.attachedSheet as? NSSavePanel)?.cancel(nil)
            try await wait("Save-sheet cancellation did not clean up") { snapshot()?.downloads.contains { !prior.contains($0.downloadId) && $0.status == "cancelled" && $0.cleanupComplete } == true }
            _ = try await cli(["browser", "download", "wait"] + target + ["--timeout-ms", "1000"], false)

            // Navigation must not cancel an independent body. Explicit cancellation
            // waits for the physical object and removes only its private partial file.
            prior = Set(snapshot()?.downloads.map(\.downloadId) ?? [])
            _ = try await cli(["browser", "goto"] + target + [origin + "/download-slow"], false)
            try await wait("Slow download save sheet did not open") { window.attachedSheet is NSSavePanel }
            guard let panel = window.attachedSheet as? NSSavePanel else { throw BridgeFailure(message: "Slow download lost its save sheet") }
            try await chooseDestination(root.appendingPathComponent("cancelled.bin"), panel: panel)
            try await wait("Slow download did not start") { snapshot()?.downloads.contains { !prior.contains($0.downloadId) && $0.status == "downloading" } == true }
            guard let slow = snapshot()?.downloads.first(where: { !prior.contains($0.downloadId) }) else { throw BridgeFailure(message: "Slow download lost its owner") }
            _ = try await cli(["browser", "goto"] + target + [origin + "/second"], false)
            try await wait("Navigation during download did not finish") { browser(id)?.webView.title == "Second fixture" && browser(id)?.webView.isLoading == false }
            guard snapshot()?.downloads.first(where: { $0.downloadId == slow.downloadId })?.status == "downloading" else { throw BridgeFailure(message: "Navigation cancelled or retargeted its admitted download") }
            _ = try await cli(["browser", "download", "cancel"] + target + [slow.downloadId.uuidString], false)
            try await wait("Physical download cancellation did not clean up") { snapshot()?.downloads.contains { $0.downloadId == slow.downloadId && $0.status == "cancelled" && $0.cleanupComplete } == true }
            let checkCancelled = Task<Bool, Never>.detached { FileManager.default.fileExists(atPath: root.appendingPathComponent("cancelled.bin").path) }
            let cancellationPublished = await checkCancelled.value
            guard !cancellationPublished, snapshot()?.workspace?.root.panes.map({ $0.selectedSurface }) == selected else { throw BridgeFailure(message: "Download cancellation published bytes or changed focus") }
            _ = try await cli(["close-surface", "--surface", String(id)], false)
            surface = nil
            try await removeFixture(root)
        } catch {
            if let panel = window.attachedSheet as? NSSavePanel { panel.cancel(nil) }
            var cleanupConfirmed = true
            if let surface {
                do {
                    _ = try await cli(["close-surface", "--surface", String(surface)], false)
                    try await wait("Download fixture physical cancellation remains unconfirmed") {
                        snapshot()?.downloads.filter { $0.origin.browser.surfaceId == surface }.allSatisfy { $0.terminal && $0.cleanupComplete } == true
                    }
                } catch {
                    cleanupConfirmed = false
                    FileHandle.standardError.write(Data(("download fixture surface cleanup: " + error.localizedDescription + "\n").utf8))
                }
            }
            if cleanupConfirmed {
                do { try await removeFixture(root) }
                catch { FileHandle.standardError.write(Data(("download fixture file cleanup: " + error.localizedDescription + "\n").utf8)) }
            }
            throw error
        }
    }
}

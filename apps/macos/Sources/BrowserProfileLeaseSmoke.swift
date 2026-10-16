import Darwin
import Foundation
import WebKit

@MainActor
enum BrowserProfileLeaseSmoke {
    static func run(profile: UUID, browser: BrowserView) async throws {
        guard browser.window != nil,
              browser.webView.configuration.websiteDataStore.identifier == profile else {
            throw BridgeFailure(message: "Profile lease acceptance requires the actual mounted UUID website store.")
        }
        let executable = Bundle.main.bundleURL.deletingLastPathComponent()
            .appendingPathComponent("native-profile-lease-probe")
        try await Task.detached {
            let root = try BrowserProfileLeaseDirectory.resolve()
            let path = URL(fileURLWithPath: root).appendingPathComponent("browser-profile-" + profile.uuidString.lowercased() + ".lock")
            guard FileManager.default.isExecutableFile(atPath: executable.path) else {
                throw BridgeFailure(message: "Independent native profile lease probe is unavailable.")
            }
            let process = Process()
            process.executableURL = executable
            process.arguments = [path.path]
            process.standardInput = FileHandle.nullDevice
            process.standardOutput = FileHandle.nullDevice
            process.standardError = FileHandle.nullDevice
            try process.run()
            // The child also owns an unconditional five-second alarm and cannot
            // spawn descendants. All observation/waits remain off MainActor.
            let deadline = ContinuousClock.now + .seconds(3)
            while process.isRunning, ContinuousClock.now < deadline { usleep(10_000) }
            let timedOut = process.isRunning
            if timedOut { process.terminate() }
            process.waitUntilExit()
            guard !timedOut, process.terminationReason == .exit, process.terminationStatus == 0 else {
                throw BridgeFailure(message: "Independent process did not observe the mounted profile's shared lease.")
            }
        }.value
        FileHandle.standardError.write(Data("Native browser profile lease: mounted UUID store blocks independent-process exclusive ownership\n".utf8))
    }
}

import AppKit
import Darwin

@MainActor
enum AppStartup {
    /// Runs before AppKit and Rust workers start. CLI invocations never open a window.
    static func prepare() {
        let windowFixture = CommandLine.arguments.contains("--window-lifecycle-smoke")
        if windowFixture {
            FileHandle.standardError.write(Data("Window lifecycle startup: prepare entered pid=\(ProcessInfo.processInfo.processIdentifier)\n".utf8))
        }
        guard let resources = Bundle.main.resourceURL else { return }
        let cliDirectory = resources.appendingPathComponent("bin")
        var arguments = Array(CommandLine.arguments.dropFirst())
        if arguments.first == "--cli" { arguments.removeFirst() }
        if !arguments.isEmpty && arguments.first != "--smoke-test" && arguments.first != "--window-lifecycle-smoke" && arguments.first != "--terminal-engine-smoke" && arguments.first != "--idle-workload-smoke" && arguments.first != "--terminal-selection-smoke" && arguments.first != "--completed-history-smoke" && arguments.first != "--agent-inbox-smoke" && !(arguments.first?.hasPrefix("-psn_") ?? false) {
            let process = Process()
            process.executableURL = cliDirectory.appendingPathComponent("zmux")
            process.arguments = arguments
            do { try process.run(); process.waitUntilExit(); exit(process.terminationStatus) }
            catch {
                FileHandle.standardError.write(Data("zmux: unable to launch bundled CLI: \(error.localizedDescription)\n".utf8))
                exit(1)
            }
        }
        let inheritedPath = ProcessInfo.processInfo.environment["PATH"] ?? "/usr/bin:/bin:/usr/sbin:/sbin"
        setenv("PATH", cliDirectory.path + ":" + inheritedPath, 1)
        if let path = ProcessInfo.processInfo.environment["ZMUX_SOCKET_PATH"] {
            setenv("ZMUX_SOCKET", path, 1)
        } else if ProcessInfo.processInfo.environment["ZMUX_SOCKET"] == nil {
            let path = URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Library/Application Support/zmux/control.sock").path
            setenv("ZMUX_SOCKET", path, 1)
        }
        setenv("TERM_PROGRAM", "zmux", 1)
        if windowFixture {
            FileHandle.standardError.write(Data("Window lifecycle startup: prepare completed\n".utf8))
        }
    }

    static func validateCloudCallbackContract() throws {
        let endpoint = try CloudEndpoint("https://zmux.w3dev.app")
        let authorization = try CloudAuthorization(endpoint: endpoint)
        var callback = URLComponents(string: CloudAuthorization.callback)!
        callback.queryItems = [URLQueryItem(name: "state", value: authorization.state), URLQueryItem(name: "code", value: "native-smoke-code")]
        guard let url = callback.url, try authorization.code(from: url) == "native-smoke-code" else {
            throw BridgeFailure(message: "Native cloud callback could not consume a valid authorization code")
        }
        let invalidCallbacks = ["zmux-dev://oauth/callback?state=incorrect&code=x",
                                "https://zmux.w3dev.app/callback?state=\(authorization.state)&code=x",
                                "zmux-dev://oauth/callback?state=\(authorization.state)&state=\(authorization.state)&code=x",
                                "zmux-dev://oauth/callback?state=\(authorization.state)&code=x&access_token=forbidden"]
        for value in invalidCallbacks {
            if (try? authorization.code(from: URL(string: value)!)) != nil {
                throw BridgeFailure(message: "Native cloud callback accepted an invalid state or token handoff")
            }
        }
        guard let parts = URLComponents(url: authorization.url, resolvingAgainstBaseURL: false),
              parts.queryItems?.first(where: { $0.name == "code_challenge_method" })?.value == "S256",
              parts.queryItems?.first(where: { $0.name == "code_challenge" })?.value?.count == 43,
              (try? CloudEndpoint("http://example.com")) == nil else {
            throw BridgeFailure(message: "Native cloud authorization configuration failed validation")
        }
    }
}

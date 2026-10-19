import AppKit
import AuthenticationServices

// CloudSettings currently depends on the app-lifetime terminal owner, but this
// callback-only probe must not link the Rust terminal bridge. Keep these minimal
// API stand-ins local to the fixture and fail if the probe starts using them.
struct CloudTerminalSessionScope {
    let sessionId: UInt64
}

struct CloudTerminalSessionDescriptor {
    let scope: CloudTerminalSessionScope
    let title: String
}

enum CloudTerminalSharingState: Equatable {
    case off
    case starting
    case sharing
    case stopping
    case failed(String)

    var isSharing: Bool {
        if case .sharing = self { return true }
        return false
    }

    var message: String {
        switch self {
        case .off: return "Terminal sharing is off."
        case .starting: return "Starting terminal sharing…"
        case .sharing: return "Terminal sharing is on."
        case .stopping: return "Stopping terminal sharing…"
        case .failed(let message): return message
        }
    }
}

@MainActor
final class CloudTerminalHost {
    var sharingState: CloudTerminalSharingState { .off }

    func observeSharing(_ observer: @escaping @MainActor (CloudTerminalSharingState) -> Void) -> UUID {
        fatalError("callback-only fixture unexpectedly observed terminal sharing")
    }

    func removeSharingObserver(_ id: UUID) {
        fatalError("callback-only fixture unexpectedly removed a terminal sharing observer")
    }

    func stopForCredentialChange() async -> String? {
        fatalError("callback-only fixture unexpectedly stopped terminal sharing")
    }

    func setSharingEnabled(_ enabled: Bool) async throws {
        fatalError("callback-only fixture unexpectedly changed terminal sharing")
    }

    func listSessions(on device: CloudDevice) async throws -> [CloudTerminalSessionDescriptor] {
        fatalError("callback-only fixture unexpectedly listed terminal sessions")
    }
}

// Only the unrelated application shell is a stand-in. The actual framework class,
// its imported API annotations, and CloudSettings.swift are compiled unchanged.
@MainActor
final class MainWindowHost: NSObject {
    let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100),
                          styleMask: [.titled], backing: .buffered, defer: false)
    let cloudEndpoint: CloudEndpoint
    let cloudClient: CloudClient
    let cloudTerminalHost = CloudTerminalHost()

    override init() {
        let endpoint: CloudEndpoint
        do { endpoint = try CloudEndpoint("https://cloud-auth-callback.invalid") }
        catch { fatalError("invalid callback fixture endpoint: \(error)") }
        cloudEndpoint = endpoint
        cloudClient = CloudClient(endpoint: endpoint)
        super.init()
    }

    func updateCloudStatus(_ title: String, connected: Bool) {}

    func openCloudTerminal(_ session: CloudTerminalSessionDescriptor) -> Bool {
        fatalError("callback-only fixture unexpectedly opened a terminal session")
    }
}

@main
@MainActor
struct CloudAuthenticationCallbackProbe {
    nonisolated static func fail(_ message: String) -> Never {
        FileHandle.standardError.write(Data(("Cloud auth callback fixture failed: " + message + "\n").utf8))
        exit(1)
    }

    static func descendants(_ view: NSView) -> [NSView] {
        [view] + view.subviews.flatMap { descendants($0) }
    }

    static func main() async {
        _ = NSApplication.shared
        guard zmux_auth_callback_install() else { fail("could not capture the framework completion") }
        let owner = MainWindowHost()
        let controller = CloudWindowController(owner: owner)
        guard let content = controller.window?.contentView,
              let signIn = descendants(content).compactMap({ $0 as? NSButton })
                .first(where: { $0.title == "Sign in through browser" }) else {
            fail("the actual Cloud settings sign-in control was not created")
        }
        guard signIn.isEnabled else { fail("sign-in was already disabled") }
        // Invoke the real Objective-C action without showing a window or refreshing
        // an account: this must never read Keychain items or make HTTP requests.
        _ = controller.perform(NSSelectorFromString("signIn:"), with: nil)
        guard !signIn.isEnabled, zmux_auth_callback_has_completion() else {
            fail("sign-in did not start an authentication session")
        }
        await withCheckedContinuation { (returned: CheckedContinuation<Void, Never>) in
            DispatchQueue(label: "zmux.fixture.authentication-foreign-reply").async {
                guard !Thread.isMainThread else { fail("framework stand-in ran on the main thread") }
                FileHandle.standardOutput.write(Data("Invoking actual Cloud settings completion from a foreign queue\n".utf8))
                guard zmux_auth_callback_invoke_failure() else { fail("the framework completion disappeared") }
                FileHandle.standardOutput.write(Data("Foreign authentication completion returned\n".utf8))
                returned.resume()
            }
        }
        let deadline = ContinuousClock.now.advanced(by: .seconds(3))
        while !signIn.isEnabled && ContinuousClock.now < deadline {
            do { try await Task.sleep(for: .milliseconds(10)) }
            catch { fail("waiting for completion was cancelled") }
        }
        guard signIn.isEnabled else { fail("completion did not release the real sign-in control") }
        guard descendants(content).compactMap({ $0 as? NSTextField }).contains(where: {
            $0.stringValue.contains("SDK dry-run refused (fixture).")
        }) else { fail("the SDK failure was not presented by the real controller") }
        MainActor.preconditionIsolated()
        controller.close()
        zmux_auth_callback_reset()
        print("Cloud auth callback passed: foreign queue accepted, UI recovered on MainActor, SDK failure displayed.")
    }
}

import AppKit
import AuthenticationServices

// Replace only the framework transport. CloudSettings.swift and CloudClient.swift
// are compiled unchanged, so this drives the real controller's sign-in callback.
// The unchecked box models Objective-C invoking a legacy completion on any queue.
struct ForeignAuthenticationCompletion: @unchecked Sendable {
    let invoke: (URL?, (any Error)?) -> Void
}

typealias ASPresentationAnchor = NSWindow

@MainActor
protocol ASWebAuthenticationPresentationContextProviding: AnyObject {
    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor
}

@MainActor
final class ASWebAuthenticationSession {
    static var latest: ASWebAuthenticationSession?
    let completion: ForeignAuthenticationCompletion
    weak var presentationContextProvider: (any ASWebAuthenticationPresentationContextProviding)?

    init(url: URL, callbackURLScheme: String?, completionHandler: @escaping (URL?, (any Error)?) -> Void) {
        completion = ForeignAuthenticationCompletion(invoke: completionHandler)
        Self.latest = self
    }

    func start() -> Bool { true }
    func cancel() {}
}

@MainActor
final class MainWindowHost: NSObject {
    let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100),
                          styleMask: [.titled], backing: .buffered, defer: false)
    func updateCloudStatus(_ title: String, connected: Bool) {}
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
        guard !signIn.isEnabled, let session = ASWebAuthenticationSession.latest else {
            fail("sign-in did not start an authentication session")
        }
        let completion = session.completion
        await withCheckedContinuation { (returned: CheckedContinuation<Void, Never>) in
            DispatchQueue(label: "zmux.fixture.authentication-foreign-reply").async {
                guard !Thread.isMainThread else { fail("framework stand-in ran on the main thread") }
                FileHandle.standardOutput.write(Data("Invoking actual Cloud settings completion from a foreign queue\n".utf8))
                completion.invoke(nil, NSError(domain: "ZmuxAuthenticationFixture", code: 7,
                                               userInfo: [NSLocalizedDescriptionKey: "SDK dry-run refused (fixture)."]))
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
        ASWebAuthenticationSession.latest = nil
        print("Cloud auth callback passed: foreign queue accepted, UI recovered on MainActor, SDK failure displayed.")
    }
}

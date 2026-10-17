import AppKit

/// Explicit user presentation can require Launch Services activation when macOS
/// refuses cooperative activation. Target only this uniquely registered process;
/// never substitute another bundle or request a new application instance.
@MainActor
final class NativeApplicationActivation {
    @MainActor
    private final class Request {
        var result: Result<Int32, BridgeFailure>?
    }
    private var pending: Request?

    func activateCurrent() async throws {
        let request: Request
        if let existing = pending { request = existing }
        else {
            let current = NSRunningApplication.current
            let bundle = Bundle.main.bundleURL.standardizedFileURL
            guard current.processIdentifier == getpid(), current.bundleURL?.standardizedFileURL == bundle,
                  let identifier = current.bundleIdentifier,
                  NSWorkspace.shared.runningApplications.filter({ $0.bundleIdentifier == identifier }).map(\.processIdentifier) == [getpid()] else {
                throw BridgeFailure(message: "Cannot uniquely identify this running zmux app for activation")
            }
            request = Request()
            pending = request
            let configuration = NSWorkspace.OpenConfiguration()
            configuration.activates = true
            configuration.createsNewApplicationInstance = false
            configuration.allowsRunningApplicationSubstitution = false
            configuration.arguments = []
            NSWorkspace.shared.openApplication(at: bundle, configuration: configuration) { @Sendable [weak self, weak request] running, error in
                let result: Result<Int32, BridgeFailure>
                if let error { result = .failure(BridgeFailure(message: "zmux activation failed: " + error.localizedDescription)) }
                else if let running { result = .success(running.processIdentifier) }
                else { result = .failure(BridgeFailure(message: "zmux activation returned no process identity")) }
                let owner = self
                let capturedRequest = request
                Task { @MainActor in
                    capturedRequest?.result = result
                    if owner?.pending === capturedRequest { owner?.pending = nil }
                }
            }
        }
        let deadline = ContinuousClock.now + .seconds(3)
        while request.result == nil {
            guard ContinuousClock.now < deadline else {
                // Keep the pending owner until its callback arrives. A timeout
                // does not authorize another launch/activation request.
                throw BridgeFailure(message: "zmux activation is unconfirmed; the request was not replayed")
            }
            try await Task.sleep(for: .milliseconds(20))
        }
        try Task.checkCancellation()
        guard let result = request.result, try result.get() == getpid() else {
            throw BridgeFailure(message: "Activation did not confirm this running zmux process")
        }
    }
}

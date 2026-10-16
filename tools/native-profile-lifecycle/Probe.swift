// Original MIT feasibility probe. Uses only generated stores in its unique bundle.
import AppKit
import Darwin
import Foundation
import WebKit

private struct Removal: Codable, Sendable {
    let phase: String
    var callbackReceived = false
    var errorDomain: String?
    var errorCode: Int?
    var errorDescription: String?
}
private struct Report: Codable, Sendable {
    let sourceSHA: String
    let appPID: Int32
    let bundleID: String
    let nonce: String
    let profileA: UUID
    let profileB: UUID
    var state = "starting"
    var stage = "startup"
    var mountedA = false
    var aViewReleased: Bool?
    var aStoreReleased: Bool?
    var heldDataRetained = false
    var heldAObservation: String?
    var reopenedAEmpty = false
    var reopenedAObservation: String?
    var reopenedAStorageObservation: String?
    var separateBRetained = false
    var separateBObservation: String?
    var removals: [Removal] = []
    var failure: String?
}
private struct ProbeFailure: Error, CustomStringConvertible { let description: String }

@MainActor
private final class Probe: NSObject, NSApplicationDelegate, WKNavigationDelegate {
    private let output: URL
    private let fixture: URL
    private var report: Report
    private let window = NSWindow(contentRect: NSRect(x: 30, y: 30, width: 1280, height: 480),
        styleMask: [.titled, .closable], backing: .buffered, defer: false)
    private var aStore: WKWebsiteDataStore?
    private var bStore: WKWebsiteDataStore?
    private var aView: WKWebView?
    private var bView: WKWebView?
    private weak var retiredAView: WKWebView?
    private weak var retiredAStore: WKWebsiteDataStore?
    private weak var retiredBView: WKWebView?
    private var navigation: [ObjectIdentifier: CheckedContinuation<Void, any Error>] = [:]

    init(output: URL, fixture: URL, nonce: String, a: UUID, b: UUID) {
        self.output = output
        self.fixture = fixture
        report = Report(sourceSHA: Bundle.main.object(forInfoDictionaryKey: "ProbeSourceSHA") as? String ?? "",
            appPID: getpid(), bundleID: Bundle.main.bundleIdentifier ?? "", nonce: nonce, profileA: a, profileB: b)
        super.init()
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        Task { await run() }
    }
    private func save() async throws {
        let value = report
        let output = output
        try await Task.detached {
            let data = try JSONEncoder().encode(value)
            guard data.count <= 65536 else { throw ProbeFailure(description: "Report exceeds bound") }
            try data.write(to: output, options: .atomic)
        }.value
    }
    private func makeView(profile: UUID, x: CGFloat) -> (WKWebsiteDataStore, WKWebView) {
        let store = WKWebsiteDataStore(forIdentifier: profile)
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = store
        let view = WKWebView(frame: NSRect(x: x, y: 0, width: 630, height: 470), configuration: configuration)
        view.navigationDelegate = self
        window.contentView?.addSubview(view)
        return (store, view)
    }
    private func load(_ view: WKWebView) async throws {
        try await withCheckedThrowingContinuation { continuation in
            navigation[ObjectIdentifier(view)] = continuation
            view.load(URLRequest(url: fixture))
        }
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        self.navigation.removeValue(forKey: ObjectIdentifier(webView))?.resume()
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) {
        self.navigation.removeValue(forKey: ObjectIdentifier(webView))?.resume(throwing: error)
    }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) {
        self.navigation.removeValue(forKey: ObjectIdentifier(webView))?.resume(throwing: error)
    }
    private func evaluate(_ view: WKWebView?, _ script: String) async throws -> String {
        guard let view else { throw ProbeFailure(description: "Expected owned WebKit view is absent") }
        return try await withCheckedThrowingContinuation { continuation in
            view.evaluateJavaScript(script) { value, error in
                if let error { continuation.resume(throwing: error) }
                else if let text = value as? String { continuation.resume(returning: text) }
                else { continuation.resume(throwing: ProbeFailure(description: "Expected string WebKit result")) }
            }
        }
    }
    private func seedA() async throws {
        guard let view = aView else { throw ProbeFailure(description: "A is absent") }
        try await load(view)
        guard try await evaluate(aView, "localStorage.setItem('probe','A');document.cookie='probe=A; Max-Age=3600; Path=/'; JSON.stringify({local:localStorage.getItem('probe'),cookie:document.cookie.includes('probe=A')})") == "{\"local\":\"A\",\"cookie\":true}" else {
            throw ProbeFailure(description: "A did not seed actual persistent website data")
        }
    }
    private func seedB() async throws {
        guard let view = bView else { throw ProbeFailure(description: "B is absent") }
        try await load(view)
        guard try await evaluate(bView, "localStorage.setItem('probe','B');document.cookie='probe=B; Max-Age=3600; Path=/'; JSON.stringify({local:localStorage.getItem('probe'),cookie:document.cookie.includes('probe=B')})") == "{\"local\":\"B\",\"cookie\":true}" else {
            throw ProbeFailure(description: "B did not seed independent website data")
        }
    }
    private func remove(_ profile: UUID, phase: String) async throws -> Removal {
        report.stage = phase + "_callback_pending"
        report.removals.append(Removal(phase: phase))
        try await save()
        // No internal timeout/replay: the external owned-process watchdog records
        // a missing callback as unknown. Only this public callback is a receipt.
        let result: Removal = await withCheckedContinuation { continuation in
            WKWebsiteDataStore.remove(forIdentifier: profile) { error in
                let error = error as NSError?
                continuation.resume(returning: Removal(phase: phase, callbackReceived: true,
                    errorDomain: error?.domain, errorCode: error?.code,
                    errorDescription: error.map { String($0.localizedDescription.prefix(1024)) }))
            }
        }
        report.removals[report.removals.count - 1] = result
        report.stage = phase + "_callback_received"
        try await save()
        return result
    }
    private func dropA() {
        retiredAView = aView
        retiredAStore = aStore
        aView?.stopLoading()
        aView?.navigationDelegate = nil
        aView?.removeFromSuperview()
        aView = nil
        aStore = nil
    }
    private func observeARelease() async throws {
        let deadline = ContinuousClock.now + .seconds(5)
        while retiredAView != nil, ContinuousClock.now < deadline {
            try await Task.sleep(for: .milliseconds(32))
        }
        report.aViewReleased = retiredAView == nil
        report.aStoreReleased = retiredAStore == nil
        try await save()
        guard retiredAView == nil else { throw ProbeFailure(description: "Application-released WKWebView still exists; removal prerequisite is unproven") }
        // Weak-store disappearance is diagnostic only, never a retirement receipt.
    }
    private func reopenA() async throws {
        (aStore, aView) = makeView(profile: report.profileA, x: 0)
        guard let view = aView else { throw ProbeFailure(description: "A reopen is absent") }
        try await load(view)
        let observation = try await evaluate(aView, "JSON.stringify({empty:localStorage.getItem('probe')===null,cookie:!document.cookie.includes('probe=')})")
        report.reopenedAObservation = String(observation.prefix(4096))
        report.reopenedAEmpty = observation == "{\"empty\":true,\"cookie\":true}"
        report.reopenedAStorageObservation = String(try await evaluate(aView,
            "JSON.stringify({localValue:localStorage.getItem('probe'),cookieValue:document.cookie})").prefix(4096))
    }
    private func run() async {
        do {
            guard report.profileA != report.profileB,
                  report.bundleID.hasPrefix("app.w3dev.zmux.profile-lifecycle-probe.") else {
                throw ProbeFailure(description: "Probe lacks its unique isolated namespace")
            }
            try await save()
            window.title = "zmux isolated profile lifecycle probe"
            (aStore, aView) = makeView(profile: report.profileA, x: 0)
            (bStore, bView) = makeView(profile: report.profileB, x: 640)
            window.orderFront(nil)
            try await seedA()
            try await seedB()
            report.mountedA = aView?.window === window && aStore?.identifier == report.profileA
            guard report.mountedA else { throw ProbeFailure(description: "A is not the actual mounted UUID store") }
            let held = try await remove(report.profileA, phase: "mounted_a_remove")
            guard held.errorDomain != nil else { throw ProbeFailure(description: "Public removal unexpectedly succeeded with mounted A") }
            let heldObservation = try await evaluate(aView, "JSON.stringify({local:localStorage.getItem('probe'),cookie:document.cookie.includes('probe=A')})")
            report.heldAObservation = String(heldObservation.prefix(4096))
            report.heldDataRetained = heldObservation == "{\"local\":\"A\",\"cookie\":true}"
            guard report.heldDataRetained else { throw ProbeFailure(description: "Mounted removal failure did not preserve A data") }
            dropA()
            try await observeARelease()
            let released = try await remove(report.profileA, phase: "released_a_remove")
            guard released.errorDomain == nil else { throw ProbeFailure(description: "Public removal after releasing A failed; see exact callback error") }
            try await reopenA()
            let separateObservation = try await evaluate(bView, "JSON.stringify({local:localStorage.getItem('probe'),cookie:document.cookie.includes('probe=B')})")
            report.separateBObservation = String(separateObservation.prefix(4096))
            report.separateBRetained = separateObservation == "{\"local\":\"B\",\"cookie\":true}"
            guard report.reopenedAEmpty, report.separateBRetained else { throw ProbeFailure(description: "Removal/recreation did not preserve profile isolation") }
            dropA()
            try await observeARelease()
            let cleanupA = try await remove(report.profileA, phase: "cleanup_a")
            retiredBView = bView
            bView?.stopLoading()
            bView?.navigationDelegate = nil
            bView?.removeFromSuperview()
            bView = nil
            bStore = nil
            let bReleaseDeadline = ContinuousClock.now + .seconds(5)
            while retiredBView != nil, ContinuousClock.now < bReleaseDeadline {
                try await Task.sleep(for: .milliseconds(32))
            }
            guard retiredBView == nil else { throw ProbeFailure(description: "Released B WKWebView still exists; cleanup prerequisite is unproven") }
            let cleanupB = try await remove(report.profileB, phase: "cleanup_b")
            guard cleanupA.errorDomain == nil, cleanupB.errorDomain == nil else {
                throw ProbeFailure(description: "Generated-store public cleanup failed; no filesystem deletion attempted")
            }
            report.state = "complete"
            report.stage = "public_cleanup_complete"
            try await save()
            exit(0)
        } catch {
            report.state = "failed"
            report.failure = String(String(describing: error).prefix(2048))
            do { try await save() }
            catch { FileHandle.standardError.write(Data(("Probe report save failed: " + error.localizedDescription + "\n").utf8)) }
            // Do not replay a missing/failed removal callback as cleanup, and do
            // not delete WebKit filesystem paths or touch unrelated stores.
            FileHandle.standardError.write(Data(((report.failure ?? "probe failed") + "\n").utf8))
            exit(1)
        }
    }
}

@main
@MainActor
private enum Main {
    static func main() {
        let arguments = CommandLine.arguments
        guard arguments.count == 6, let fixture = URL(string: arguments[2]),
              fixture.scheme == "http", fixture.host == "127.0.0.1",
              let a = UUID(uuidString: arguments[4]), let b = UUID(uuidString: arguments[5]) else { exit(2) }
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        let delegate = Probe(output: URL(fileURLWithPath: arguments[1]), fixture: fixture,
            nonce: arguments[3], a: a, b: b)
        app.delegate = delegate
        withExtendedLifetime(delegate) { app.run() }
    }
}

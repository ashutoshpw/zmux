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
private struct DataRemoval: Codable, Sendable {
    let scope: String
    let dataTypes: [String]
    var callbackReceived = false
}
private struct CookieObservation: Codable, Sendable {
    let name: String
    let value: String
    let domain: String
    let path: String
    let sessionOnly: Bool
    let expiresAt: Double?
    let secure: Bool
    let httpOnly: Bool
}
private struct NativeCookieSet: Codable, Sendable {
    let profile: UUID
    let value: String
    var callbackReceived = false
}
private struct Report: Codable, Sendable {
    let sourceSHA: String
    let appPID: Int32
    let bundleID: String
    let nonce: String
    let profileA: UUID
    let profileB: UUID
    let phase: String
    let scenario: String
    let termination: String
    var preparerPID: Int32?
    var state = "starting"
    var stage = "startup"
    var mountedA = false
    var aViewReleased: Bool?
    var aStoreReleased: Bool?
    var bViewReleased: Bool?
    var callerRetainsExactAStore: Bool?
    var callerRetainsExactBStore: Bool?
    var heldDataRetained = false
    var heldAObservation: String?
    var reopenedAEmpty = false
    var reopenedAObservation: String?
    var reopenedAStorageObservation: String?
    var separateBRetained = false
    var separateBObservation: String?
    var separateBStorageObservation: String?
    var aCookieStoreObservation: [CookieObservation]?
    var bCookieStoreObservation: [CookieObservation]?
    var nativeCookieSets: [NativeCookieSet] = []
    var aNativeCookieRetained = false
    var bNativeCookieRetained = false
    var aDataRemoval: DataRemoval?
    var removals: [Removal] = []
    var failure: String?
}
private struct ProbeFailure: Error, CustomStringConvertible { let description: String }

@MainActor
private final class Probe: NSObject, NSApplicationDelegate, WKNavigationDelegate {
    private let output: URL
    private let fixture: URL
    private let preparation: URL
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
    private weak var retiredBStore: WKWebsiteDataStore?
    private var navigation: [ObjectIdentifier: CheckedContinuation<Void, any Error>] = [:]

    init(output: URL, fixture: URL, nonce: String, a: UUID, b: UUID, phase: String, preparation: URL, scenario: String) {
        self.output = output
        self.fixture = fixture
        self.preparation = preparation
        report = Report(sourceSHA: Bundle.main.object(forInfoDictionaryKey: "ProbeSourceSHA") as? String ?? "",
            appPID: getpid(), bundleID: Bundle.main.bundleIdentifier ?? "", nonce: nonce, profileA: a, profileB: b,
            phase: phase, scenario: scenario, termination: "nsapplication_terminate")
        super.init()
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        Task { await run() }
    }
    private func callerRetainsExactStore(_ store: WKWebsiteDataStore?, retired: WKWebsiteDataStore?, profile: UUID) -> Bool {
        guard let store, let retired else { return false }
        return store === retired && store.identifier == profile && store.isPersistent
    }
    func applicationWillTerminate(_ notification: Notification) {
        // Public caller references only: this does not observe NetworkProcess
        // sessions, cookie writes or a durability/flush completion.
        let aRetained = callerRetainsExactStore(aStore, retired: retiredAStore, profile: report.profileA)
        let bRetained = callerRetainsExactStore(bStore, retired: retiredBStore, profile: report.profileB)
        let viewsReleased = aView == nil && bView == nil && retiredAView == nil && retiredBView == nil
        let line = "ZMUX_APPKIT_TERMINATION {\"event\":\"application_will_terminate\",\"appPID\":\(getpid()),\"phase\":\"\(report.phase)\",\"callerRetainsExactAStore\":\(aRetained ? "true" : "false"),\"callerRetainsExactBStore\":\(bRetained ? "true" : "false"),\"viewsReleased\":\(viewsReleased ? "true" : "false")}\n"
        FileHandle.standardError.write(Data(line.utf8))
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
    private func dropA(retainStore: Bool = false) {
        retiredAView = aView
        retiredAStore = aStore
        aView?.stopLoading()
        aView?.navigationDelegate = nil
        aView?.removeFromSuperview()
        aView = nil
        if !retainStore { aStore = nil }
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
    private func clearInactiveAData() async throws {
        guard let store = aStore, store.identifier == report.profileA,
              aView == nil, retiredAView == nil else {
            throw ProbeFailure(description: "Store-specific data removal requires the exact inactive A owner")
        }
        let types = WKWebsiteDataStore.allWebsiteDataTypes()
        guard types.contains(WKWebsiteDataTypeCookies) else {
            throw ProbeFailure(description: "All website data types omitted cookies")
        }
        report.aDataRemoval = DataRemoval(scope: "profile_a_all_website_data", dataTypes: types.sorted())
        report.stage = "inactive_a_data_remove_callback_pending"
        try await save()
        // Completion has no error parameter and is not a retirement receipt.
        // Keep this exact store until completion; never retry a missing callback.
        let _: Void = await withCheckedContinuation { continuation in
            store.removeData(ofTypes: types, modifiedSince: .distantPast) {
                continuation.resume()
            }
        }
        report.aDataRemoval?.callbackReceived = true
        report.stage = "inactive_a_data_remove_callback_received"
        try await save()
    }
    private func loadA() async throws {
        (aStore, aView) = makeView(profile: report.profileA, x: 0)
        guard let view = aView else { throw ProbeFailure(description: "A reopen is absent") }
        try await load(view)
    }
    private func loadFreshB() async throws {
        (bStore, bView) = makeView(profile: report.profileB, x: 640)
        guard let view = bView else { throw ProbeFailure(description: "Fresh B is absent") }
        try await load(view)
    }
    private func reopenA() async throws {
        try await loadA()
        let observation = try await evaluate(aView, "JSON.stringify({empty:localStorage.getItem('probe')===null,cookie:!document.cookie.includes('probe=')})")
        report.reopenedAObservation = String(observation.prefix(4096))
        report.reopenedAEmpty = observation == "{\"empty\":true,\"cookie\":true}"
        report.reopenedAStorageObservation = String(try await evaluate(aView,
            "JSON.stringify({localValue:localStorage.getItem('probe'),cookieValue:document.cookie})").prefix(4096))
    }
    private func observeBData() async throws {
        let observation = try await evaluate(bView, "JSON.stringify({local:localStorage.getItem('probe'),cookie:document.cookie.includes('probe=B')})")
        report.separateBObservation = String(observation.prefix(4096))
        report.separateBRetained = observation == "{\"local\":\"B\",\"cookie\":true}"
        report.separateBStorageObservation = String(try await evaluate(bView,
            "JSON.stringify({localValue:localStorage.getItem('probe'),cookieValue:document.cookie})").prefix(4096))
    }
    private func observeRetainedAData() async throws {
        let observation = try await evaluate(aView, "JSON.stringify({local:localStorage.getItem('probe'),cookie:document.cookie.includes('probe=A')})")
        report.heldAObservation = String(observation.prefix(4096))
        report.heldDataRetained = observation == "{\"local\":\"A\",\"cookie\":true}"
        report.reopenedAStorageObservation = String(try await evaluate(aView,
            "JSON.stringify({localValue:localStorage.getItem('probe'),cookieValue:document.cookie})").prefix(4096))
    }
    private func readCookies(_ store: WKWebsiteDataStore?) async throws -> [CookieObservation] {
        guard let store else { throw ProbeFailure(description: "Exact cookie store is absent") }
        let cookies: [CookieObservation] = await withCheckedContinuation { continuation in
            store.httpCookieStore.getAllCookies { cookies in
                continuation.resume(returning: cookies.prefix(129).map {
                    CookieObservation(name: String($0.name.prefix(256)), value: String($0.value.prefix(256)),
                        domain: String($0.domain.prefix(256)), path: String($0.path.prefix(256)),
                        sessionOnly: $0.isSessionOnly, expiresAt: $0.expiresDate?.timeIntervalSince1970,
                        secure: $0.isSecure, httpOnly: $0.isHTTPOnly)
                })
            }
        }
        guard cookies.count <= 128 else { throw ProbeFailure(description: "Cookie observation exceeds bound") }
        return cookies
    }
    private func setNativeCookie(_ store: WKWebsiteDataStore?, profile: UUID, value: String) async throws {
        guard report.phase == "prepare", report.scenario == "baseline",
              let store, store.identifier == profile, store.isPersistent, report.nativeCookieSets.count < 2 else {
            throw ProbeFailure(description: "Native cookie creation requires the exact preparer store")
        }
        let cookies = try await readCookies(store)
        guard cookies.count == 1, let original = cookies.first,
              original.name == "probe", original.value == value, original.domain == fixture.host,
              original.path == "/", !original.sessionOnly, !original.secure, !original.httpOnly,
              let expiry = original.expiresAt, expiry > Date().timeIntervalSince1970,
              let native = HTTPCookie(properties: [.name: "native_created", .value: value,
                  .domain: original.domain, .path: original.path, .expires: Date(timeIntervalSince1970: expiry)]),
              !native.isSessionOnly, !native.isSecure, !native.isHTTPOnly else {
            throw ProbeFailure(description: "Original persistent JS cookie attributes are not comparable")
        }
        report.nativeCookieSets.append(NativeCookieSet(profile: profile, value: value))
        report.stage = "native_cookie_" + value.lowercased() + "_set_callback_pending"
        try await save()
        // Public completion acknowledges storage, not a promised disk flush.
        // Call once; the unchanged external watchdog bounds a missing callback.
        let _: Void = await withCheckedContinuation { continuation in
            store.httpCookieStore.setCookie(native) { continuation.resume() }
        }
        report.nativeCookieSets[report.nativeCookieSets.count - 1].callbackReceived = true
        report.stage = "native_cookie_" + value.lowercased() + "_set_callback_received"
        try await save()
    }
    private func observeNativeCookies() async throws {
        // The distinct name cannot satisfy the unchanged original probe= checks.
        report.aNativeCookieRetained = try await evaluate(aView,
            "JSON.stringify(document.cookie.split(';').map(v=>v.trim()).includes('native_created=A'))") == "true"
        report.bNativeCookieRetained = try await evaluate(bView,
            "JSON.stringify(document.cookie.split(';').map(v=>v.trim()).includes('native_created=B'))") == "true"
    }
    private func requireComparableCookieStores() throws {
        for (cookies, value) in [(report.aCookieStoreObservation, "A"), (report.bCookieStoreObservation, "B")] {
            guard let cookies, cookies.count == 2,
                  Set(cookies.map(\.name)) == ["probe", "native_created"],
                  cookies.allSatisfy({ $0.value == value && $0.domain == fixture.host && $0.path == "/" &&
                      !$0.sessionOnly && !$0.secure && !$0.httpOnly && ($0.expiresAt ?? 0) > Date().timeIntervalSince1970 }),
                  let firstExpiry = cookies.first?.expiresAt, let lastExpiry = cookies.last?.expiresAt,
                  abs(firstExpiry - lastExpiry) < 1 else {
                throw ProbeFailure(description: "Baseline requires both comparable persistent cookie creation paths")
            }
        }
    }
    private func observeCookieStores() async throws {
        report.stage = "cookie_store_observation_pending"
        try await save()
        report.aCookieStoreObservation = try await readCookies(aStore)
        report.bCookieStoreObservation = try await readCookies(bStore)
        report.stage = "cookie_store_observation_received"
        try await save()
    }
    private func dropB(retainStore: Bool = false) async throws {
        retiredBView = bView
        retiredBStore = bStore
        bView?.stopLoading()
        bView?.navigationDelegate = nil
        bView?.removeFromSuperview()
        bView = nil
        if !retainStore { bStore = nil }
        let deadline = ContinuousClock.now + .seconds(5)
        while retiredBView != nil, ContinuousClock.now < deadline {
            try await Task.sleep(for: .milliseconds(32))
        }
        report.bViewReleased = retiredBView == nil
        guard retiredBView == nil else { throw ProbeFailure(description: "Released B WKWebView still exists; cleanup prerequisite is unproven") }
    }
    private func runFreshProcess() async throws {
        let preparation = preparation
        let prepared = try await Task.detached {
            let data = try Data(contentsOf: preparation)
            guard data.count <= 65536 else { throw ProbeFailure(description: "Preparation report exceeds bound") }
            return try JSONDecoder().decode(Report.self, from: data)
        }.value
        guard prepared.phase == "prepare", prepared.state == "prepared", prepared.stage == "prepared_for_fresh_process",
              prepared.scenario == report.scenario,
              prepared.termination == report.termination,
              prepared.sourceSHA == report.sourceSHA, prepared.bundleID == report.bundleID, prepared.nonce == report.nonce,
              prepared.profileA == report.profileA, prepared.profileB == report.profileB, prepared.appPID != getpid(),
              prepared.separateBRetained,
              (report.scenario == "baseline" ? prepared.heldDataRetained && prepared.removals.isEmpty && prepared.aDataRemoval == nil
                : prepared.reopenedAEmpty && prepared.aDataRemoval?.callbackReceived == true) else {
            throw ProbeFailure(description: "Fresh process does not match the completed exact preparation")
        }
        report.preparerPID = prepared.appPID
        report.stage = "fresh_process_loading"
        try await save()
        // Never seed on reopen. Observe the exact namespace from the exited app.
        try await loadFreshB()
        if report.scenario == "baseline" {
            try await loadA()
            try await observeRetainedAData()
        } else { try await reopenA() }
        try await observeBData()
        if report.scenario == "baseline" { try await observeNativeCookies() }
        try await observeCookieStores()
        let aMatches = report.scenario == "baseline" ? report.heldDataRetained : report.reopenedAEmpty
        guard aMatches, report.separateBRetained else {
            throw ProbeFailure(description: report.scenario == "baseline"
                ? "Fresh-process no-removal baseline did not preserve A and B data"
                : "Fresh process did not observe empty A and unchanged B")
        }
        if report.scenario == "baseline" {
            try requireComparableCookieStores()
            guard report.aNativeCookieRetained, report.bNativeCookieRetained, report.nativeCookieSets.isEmpty else {
                throw ProbeFailure(description: "Fresh-process baseline did not preserve native-created A/B cookies")
            }
        }
        report.stage = "fresh_process_data_verified"
        try await save()
        dropA()
        try await observeARelease()
        let cleanupA = try await remove(report.profileA, phase: "cleanup_a")
        try await dropB()
        let cleanupB = try await remove(report.profileB, phase: "cleanup_b")
        guard cleanupA.errorDomain == nil, cleanupB.errorDomain == nil else {
            throw ProbeFailure(description: "Generated-store public cleanup failed; no filesystem deletion attempted")
        }
        report.state = "complete"
        report.stage = "public_cleanup_complete"
        try await save()
    }
    private func run() async {
        do {
            guard report.profileA != report.profileB,
                  report.bundleID.hasPrefix("app.w3dev.zmux.profile-lifecycle-probe.") else {
                throw ProbeFailure(description: "Probe lacks its unique isolated namespace")
            }
            try await save()
            window.title = "zmux isolated profile lifecycle probe"
            window.orderFront(nil)
            if report.phase == "reopen" {
                try await runFreshProcess()
                NSApp.terminate(nil)
                return
            }
            (aStore, aView) = makeView(profile: report.profileA, x: 0)
            (bStore, bView) = makeView(profile: report.profileB, x: 640)
            try await seedA()
            try await seedB()
            report.mountedA = aView?.window === window && aStore?.identifier == report.profileA
            guard report.mountedA else { throw ProbeFailure(description: "A is not the actual mounted UUID store") }
            if report.scenario == "baseline" {
                try await setNativeCookie(aStore, profile: report.profileA, value: "A")
                try await setNativeCookie(bStore, profile: report.profileB, value: "B")
                try await observeRetainedAData()
                try await observeBData()
                try await observeNativeCookies()
                try await observeCookieStores()
                guard report.heldDataRetained, report.separateBRetained else {
                    throw ProbeFailure(description: "No-removal baseline did not seed separate A/B data")
                }
                try requireComparableCookieStores()
                guard report.aNativeCookieRetained, report.bNativeCookieRetained,
                      report.nativeCookieSets.count == 2, report.nativeCookieSets.allSatisfy(\.callbackReceived) else {
                    throw ProbeFailure(description: "Native creation path did not seed exact A/B cookies")
                }
                // This comparison changes caller-store lifetime only. Release
                // both views as before, keeping the same UUID store instances
                // strongly held until normal AppKit termination.
                dropA(retainStore: true)
                try await observeARelease()
                try await dropB(retainStore: true)
                report.callerRetainsExactAStore = callerRetainsExactStore(aStore, retired: retiredAStore, profile: report.profileA)
                report.callerRetainsExactBStore = callerRetainsExactStore(bStore, retired: retiredBStore, profile: report.profileB)
                guard report.aViewReleased == true, report.bViewReleased == true,
                      report.aStoreReleased == false,
                      report.callerRetainsExactAStore == true, report.callerRetainsExactBStore == true else {
                    throw ProbeFailure(description: "Preparer requires released views and both exact caller-retained stores")
                }
                report.state = "prepared"
                report.stage = "prepared_for_fresh_process"
                try await save()
                NSApp.terminate(nil)
                return
            }
            let held = try await remove(report.profileA, phase: "mounted_a_remove")
            guard held.errorDomain != nil else { throw ProbeFailure(description: "Public removal unexpectedly succeeded with mounted A") }
            let heldObservation = try await evaluate(aView, "JSON.stringify({local:localStorage.getItem('probe'),cookie:document.cookie.includes('probe=A')})")
            report.heldAObservation = String(heldObservation.prefix(4096))
            report.heldDataRetained = heldObservation == "{\"local\":\"A\",\"cookie\":true}"
            guard report.heldDataRetained else { throw ProbeFailure(description: "Mounted removal failure did not preserve A data") }
            dropA(retainStore: true)
            try await observeARelease()
            try await clearInactiveAData()
            dropA()
            try await observeARelease()
            let released = try await remove(report.profileA, phase: "released_a_remove")
            guard released.errorDomain == nil else { throw ProbeFailure(description: "Public removal after releasing A failed; see exact callback error") }
            try await reopenA()
            try await observeBData()
            guard report.reopenedAEmpty, report.separateBRetained else { throw ProbeFailure(description: "Removal/recreation did not preserve profile isolation") }
            dropA()
            try await observeARelease()
            try await dropB()
            // Preserve B for the separate reader. Preparation is not fresh-process proof.
            report.state = "prepared"
            report.stage = "prepared_for_fresh_process"
            try await save()
            NSApp.terminate(nil)
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
        guard arguments.count == 9, let fixture = URL(string: arguments[2]),
              fixture.scheme == "http", fixture.host == "127.0.0.1",
              let a = UUID(uuidString: arguments[4]), let b = UUID(uuidString: arguments[5]),
              ["prepare", "reopen"].contains(arguments[6]), ["baseline", "clear"].contains(arguments[8]) else { exit(2) }
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        let delegate = Probe(output: URL(fileURLWithPath: arguments[1]), fixture: fixture,
            nonce: arguments[3], a: a, b: b, phase: arguments[6], preparation: URL(fileURLWithPath: arguments[7]), scenario: arguments[8])
        app.delegate = delegate
        withExtendedLifetime(delegate) { app.run() }
    }
}

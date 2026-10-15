import AppKit
import WebKit

/// One bounded native owner retains each WebKit object and its original datastore
/// until physical cancellation/completion and owned-file cleanup are acknowledged.
@MainActor
final class BrowserDownloads: NSObject, WKDownloadDelegate {
    private enum Phase: Equatable { case reserved, choosing, downloading, finalizing, cancelling, awaitingReceipt, cleanupFailed }
    @MainActor
    private final class Job {
        let token: BrowserDownloadToken
        let origin: BrowserOwner
        let profile: UUID?
        let webView: WKWebView
        // Keep the navigation delegate alive through an approved conversion even
        // after its surface leaves the UI; WebKit delivers the physical object later.
        let conversionOwner: BrowserView
        let began = ContinuousClock.now
        var phase = Phase.reserved
        var download: WKDownload?
        var panel: NSSavePanel?
        var destination: URL?
        var cancelled = false
        var unknownAdmission = false
        var admitting = true
        var conversionPending = true
        var cancellationRequested = false
        var failureMessage: String?
        var redirects = 0
        var progressTask: Task<Void, Never>?
        var lifecycleTask: Task<Void, Never>?
        var lastProgress = ContinuousClock.now
        var lastBytes: UInt64 = 0
        init(token: BrowserDownloadToken, origin: BrowserOwner, profile: UUID?, webView: WKWebView, conversionOwner: BrowserView) {
            self.token = token; self.origin = origin; self.profile = profile; self.webView = webView; self.conversionOwner = conversionOwner
        }
    }
    private let bridge: NativeBridge
    private weak var owner: AppController?
    private let files = BrowserDownloadFiles()
    private var jobs: [UUID: Job] = [:]
    private var panelOwner: UUID?
    private var current: DesktopSnapshot?
    private var downloadsWindow: BrowserDownloadsWindowController?
    private var timer: Task<Void, Never>?
    private var stopping = false

    init(owner: AppController, bridge: NativeBridge) { self.owner = owner; self.bridge = bridge }

    func reserve(origin: BrowserOwner, profile: UUID?, webView: WKWebView, conversionOwner: BrowserView, filename: String) async throws -> BrowserDownloadToken {
        guard !stopping, jobs.count < 4, jobs.values.filter({ $0.origin.surfaceId == origin.surfaceId }).count < 2,
              let epoch = current?.downloadEpoch else { throw BridgeFailure(message: "Download capacity or its runtime is unavailable.") }
        let token = BrowserDownloadToken(epoch: epoch, downloadId: UUID(), hostId: origin.hostId)
        let job = Job(token: token, origin: origin, profile: profile, webView: webView, conversionOwner: conversionOwner)
        jobs[token.downloadId] = job
        startTimer()
        defer { job.admitting = false }
        do {
            try await bridge.browserDownload(NativeDownloadCommand(operation: "begin", owner: origin, token: token, filename: Self.safeFilename(filename)))
            return token
        } catch {
            if let failure = error as? NativeDownloadFailure, !failure.outcomeUnknown {
                release(job)
            } else {
                // Keep the known token and physical credit. A later snapshot can
                // establish admission and cancel it without replaying Begin.
                job.unknownAdmission = true
                job.cancelled = true
            }
            job.conversionPending = false
            throw error
        }
    }

    func adopt(_ download: WKDownload, token: BrowserDownloadToken, webView: WKWebView) {
        guard let job = jobs[token.downloadId], job.token == token else {
            download.cancel { _ in }
            Self.reportUncertain(BridgeFailure(message: "WebKit delivered an unowned conversion; cancellation was requested."))
            return
        }
        guard job.download == nil else {
            download.cancel { _ in }
            Self.reportUncertain(BridgeFailure(message: "A duplicate download conversion was refused; the original owner remains active."))
            return
        }
        guard job.webView === webView, download.webView === webView else {
            // Keep this admission charged until the refused physical object confirms
            // cancellation. Never drop an admitted job on a malformed conversion.
            job.conversionPending = false
            job.cancelled = true
            job.failureMessage = "WebKit delivered a mismatched download conversion."
            job.phase = .cancelling
            job.cancellationRequested = true
            download.cancel { [weak self, weak job] _ in
                Task { @MainActor in
                    guard let self, let job else { return }
                    self.cancelAcknowledged(job)
                }
            }
            return
        }
        job.conversionPending = false
        job.download = download
        download.delegate = self
        if job.cancelled { finishCancelled(job) }
    }
    func abandon(_ token: BrowserDownloadToken) {
        guard let job = jobs[token.downloadId], job.token == token else { return }
        job.conversionPending = false
        if job.phase == .cancelling { finishCancelled(job) }
        else { cancel(job) }
    }
    func cancelConversion(_ token: BrowserDownloadToken) {
        guard let job = jobs[token.downloadId], job.token == token else { return }
        // Approval already returned .download. Absence of didBecome is not proof
        // that WebKit owns no physical object; keep its credit/delegate until then.
        cancel(job)
    }
    func cancelHost(_ host: UUID) {
        for job in Array(jobs.values) where job.token.hostId == host { cancel(job) }
    }
    func update(_ snapshot: DesktopSnapshot) {
        current = snapshot
        downloadsWindow?.update(snapshot.downloads, profiles: Dictionary(uniqueKeysWithValues: snapshot.browserProfiles.profiles.map { ($0.id, $0.name) }))
        tick()
    }
    private func startTimer() {
        guard timer == nil else { return }
        timer = Task { [weak self] in
            while !Task.isCancelled {
                self?.tick()
                do { try await Task.sleep(for: .milliseconds(100)) } catch { return }
            }
        }
    }
    private func tick() {
        guard let snapshot = current else { return }
        for job in Array(jobs.values) {
            let record = snapshot.downloads.first { $0.token == job.token }
            if let record, record.terminal, job.phase == .awaitingReceipt {
                if !record.cleanupComplete { job.phase = .cleanupFailed }
                else if !job.admitting && job.progressTask == nil && job.lifecycleTask == nil { release(job) }
                continue
            }
            if job.unknownAdmission {
                if record != nil { job.unknownAdmission = false; cancel(job) }
                continue
            }
            if record?.status == "cancelling" || ContinuousClock.now - job.began > .seconds(900)
                || (job.phase == .reserved && ContinuousClock.now - job.began > .seconds(30)) {
                cancel(job)
            }
            if job.phase == .choosing && ContinuousClock.now - job.began > .seconds(120) { cancel(job) }
            if job.phase == .downloading, let download = job.download,
               job.progressTask == nil, ContinuousClock.now - job.lastProgress >= .milliseconds(250) {
                let bytes = UInt64(max(0, download.progress.completedUnitCount))
                guard bytes > job.lastBytes else { continue }
                // A cancellation threshold is not advertised as a hard disk quota.
                if bytes > 512 * 1024 * 1024 { cancel(job); continue }
                job.lastBytes = bytes
                job.lastProgress = ContinuousClock.now
                let total = download.progress.totalUnitCount > 0 ? UInt64(download.progress.totalUnitCount) : nil
                job.progressTask = Task { [weak self, weak job] in
                    guard let self, let job else { return }
                    defer { job.progressTask = nil }
                    do { try await self.bridge.browserDownload(NativeDownloadCommand(operation: "progress", token: job.token, bytes: bytes, totalBytes: total)) }
                    catch { self.cancel(job) }
                }
            }
        }
    }
    /// MainActor stays responsive while physical WebKit callbacks, background file
    /// cleanup and authoritative snapshots drain. A timeout is an explicit failure.
    func shutdown() async throws {
        stopping = true
        downloadsWindow?.close()
        for job in Array(jobs.values) { cancel(job) }
        let deadline = ContinuousClock.now + .seconds(5)
        while !jobs.isEmpty, ContinuousClock.now < deadline {
            if let snapshot = try await bridge.poll() { update(snapshot) }
            tick()
            if jobs.values.contains(where: { $0.phase == .cleanupFailed }) {
                throw BridgeFailure(message: "A download's private staging cleanup failed; native ownership remains charged.")
            }
            try await Task.sleep(for: .milliseconds(25))
        }
        guard jobs.isEmpty else { throw BridgeFailure(message: "Download shutdown could not confirm physical cancellation and cleanup before its deadline.") }
        timer?.cancel(); timer = nil
    }
    func showWindow(_ sender: Any?) {
        if downloadsWindow == nil {
            downloadsWindow = BrowserDownloadsWindowController(cancel: { [weak self] record in
                guard let self else { return }
                self.owner?.cancelBrowserDownload(record)
            })
        }
        downloadsWindow?.update(current?.downloads ?? [], profiles: Dictionary(uniqueKeysWithValues: (current?.browserProfiles.profiles ?? []).map { ($0.id, $0.name) }))
        downloadsWindow?.showWindow(sender)
    }

    private func job(_ download: WKDownload) -> Job? { jobs.values.first { $0.download === download } }
    private func release(_ job: Job) {
        job.download?.delegate = nil
        job.progressTask?.cancel(); job.progressTask = nil
        job.lifecycleTask = nil
        jobs.removeValue(forKey: job.token.downloadId)
        if jobs.isEmpty { timer?.cancel(); timer = nil }
    }
    private func cancel(_ job: Job) {
        guard ![Phase.finalizing, .cancelling, .awaitingReceipt, .cleanupFailed].contains(job.phase) else { return }
        job.cancelled = true
        job.panel?.cancel(nil)
        if job.phase == .choosing { return } // Destination handler owns its nil reply/cleanup.
        finishCancelled(job)
    }
    private func finishCancelled(_ job: Job) {
        guard job.phase != .awaitingReceipt, job.phase != .cleanupFailed else { return }
        job.cancelled = true
        job.phase = .cancelling
        guard !job.conversionPending, !job.cancellationRequested else { return }
        job.cancellationRequested = true
        if let download = job.download {
            download.cancel { [weak self, weak job] _ in
                Task { @MainActor in
                    guard let self, let job else { return }
                    self.cancelAcknowledged(job)
                }
            }
        } else { cancelAcknowledged(job) }
    }
    private func cancelAcknowledged(_ job: Job) {
        job.phase = .awaitingReceipt
        job.lifecycleTask = Task { [weak self, weak job] in
            guard let self, let job else { return }
            defer { job.lifecycleTask = nil }
            var cleanupComplete = true
            do { try await self.files.discard(job.token.downloadId) }
            catch { cleanupComplete = false }
            let operation = job.failureMessage == nil ? "cancelled" : "failed"
            do { try await self.bridge.browserDownload(NativeDownloadCommand(operation: operation, token: job.token, message: job.failureMessage, cleanupComplete: cleanupComplete)) }
            catch { Self.reportUncertain(error) }
        }
    }

    func download(_ download: WKDownload, decideDestinationUsing response: URLResponse, suggestedFilename: String) async -> URL? {
        guard let job = job(download), job.phase == .reserved, !job.cancelled else { return nil }
        job.phase = .choosing
        let deadline = ContinuousClock.now + .seconds(120)
        while panelOwner != nil, !job.cancelled, ContinuousClock.now < deadline {
            do { try await Task.sleep(for: .milliseconds(50)) } catch { job.cancelled = true }
        }
        guard !job.cancelled, ContinuousClock.now < deadline, let window = owner?.window else {
            finishCancelled(job); return nil
        }
        panelOwner = job.token.downloadId
        let panel = NSSavePanel()
        job.panel = panel
        panel.nameFieldStringValue = Self.safeFilename(suggestedFilename)
        panel.canCreateDirectories = true
        panel.title = "Save Download"
        panel.prompt = "Save"
        let selection: URL? = await withCheckedContinuation { continuation in
            panel.beginSheetModal(for: window) { response in
                continuation.resume(returning: response == .OK ? panel.url : nil)
            }
        }
        job.panel = nil
        panelOwner = nil
        guard let selection, !job.cancelled else { finishCancelled(job); return nil }
        do {
            let payload = try await files.prepare(job.token.downloadId, destination: selection)
            guard !job.cancelled else { finishCancelled(job); return nil }
            job.destination = selection
            try await bridge.browserDownload(NativeDownloadCommand(operation: "destination", token: job.token, filename: selection.lastPathComponent))
            guard !job.cancelled else { finishCancelled(job); return nil }
            job.phase = .downloading
            return payload
        } catch {
            job.failureMessage = Self.safeMessage(error.localizedDescription)
            finishCancelled(job)
            return nil
        }
    }
    func downloadDidFinish(_ download: WKDownload) {
        guard let job = job(download), job.phase == .downloading else { return }
        job.phase = .finalizing
        job.progressTask?.cancel()
        job.lifecycleTask = Task { [weak self, weak job] in
            guard let self, let job else { return }
            defer { job.lifecycleTask = nil }
            do {
                try await self.bridge.browserDownload(NativeDownloadCommand(operation: "finalizing", token: job.token))
                let saved = try await self.files.publish(job.token.downloadId)
                job.phase = .awaitingReceipt
                try await self.bridge.browserDownload(NativeDownloadCommand(operation: "saved", token: job.token, path: saved.path, bytes: saved.bytes, message: saved.cleanupError, cleanupComplete: saved.cleanupComplete))
            } catch {
                if job.phase == .awaitingReceipt { Self.reportUncertain(error); return } // Preserve published files.
                await self.fail(job, message: error.localizedDescription)
            }
        }
    }
    func download(_ download: WKDownload, didFailWithError error: any Error, resumeData: Data?) {
        guard let job = job(download), [Phase.reserved, .choosing, .downloading].contains(job.phase) else { return }
        job.failureMessage = Self.safeMessage("The download failed (" + String((error as NSError).code) + ").")
        if job.phase == .choosing {
            // The suspended destination handler owns its eventual nil reply and
            // cleanup. Cancel the actual sheet/queued choice before releasing credit.
            cancel(job)
            return
        }
        job.cancelled = true
        job.phase = .cancelling
        job.lifecycleTask = Task { [weak self, weak job] in
            guard let self, let job else { return }
            defer { job.lifecycleTask = nil }
            await self.fail(job, message: job.failureMessage ?? "The download failed.")
        }
    }
    private func fail(_ job: Job, message: String) async {
        guard job.phase != .awaitingReceipt, job.phase != .cleanupFailed else { return }
        job.phase = .awaitingReceipt
        var cleanupComplete = true
        do { try await files.discard(job.token.downloadId) }
        catch { cleanupComplete = false }
        let safe = Self.safeMessage(message)
        do { try await bridge.browserDownload(NativeDownloadCommand(operation: "failed", token: job.token, message: safe, cleanupComplete: cleanupComplete)) }
        catch { Self.reportUncertain(error) }
    }
    func download(_ download: WKDownload, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest) async -> WKDownload.RedirectPolicy {
        guard let job = job(download), !job.cancelled, let url = request.url,
              job.redirects < 20,
              ["http", "https"].contains(url.scheme?.lowercased() ?? ""), url.user == nil, url.password == nil else { return .cancel }
        job.redirects += 1
        return .allow
    }
    func download(_ download: WKDownload, didReceive challenge: URLAuthenticationChallenge) async -> (URLSession.AuthChallengeDisposition, URLCredential?) {
        if challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust { return (.performDefaultHandling, nil) }
        return (.cancelAuthenticationChallenge, nil)
    }
    private static func reportUncertain(_ error: any Error) {
        FileHandle.standardError.write(Data(("download acknowledgement retained for reconciliation: " + safeMessage(error.localizedDescription) + "\n").utf8))
    }
    static func safeFilename(_ raw: String) -> String {
        guard !raw.isEmpty, !raw.hasPrefix("."), !raw.contains("/"), !raw.contains("\\") else { return "download" }
        var result = ""
        for scalar in raw.unicodeScalars where !CharacterSet.controlCharacters.contains(scalar) {
            let piece = String(scalar)
            if result.utf8.count + piece.utf8.count > 200 { break }
            result += piece
        }
        return result.isEmpty ? "download" : result
    }
    private static func safeMessage(_ raw: String) -> String {
        var result = ""
        for scalar in raw.unicodeScalars where !CharacterSet.controlCharacters.contains(scalar) {
            let piece = String(scalar)
            if result.utf8.count + piece.utf8.count > 400 { break }
            result += piece
        }
        return result
    }
}

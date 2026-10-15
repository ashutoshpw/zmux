import AppKit
import WebKit

/// Public WebKit host. Pages receive no message handler, native socket or terminal bridge.
@MainActor
final class BrowserView: BackgroundView, WKNavigationDelegate {
    let surfaceID: UInt64
    let hostID = UUID()
    private(set) var documentGeneration: UInt64 = 1
    var onAutomationAction: (@MainActor (BrowserAction) -> Void)?
    private weak var owner: (any BrowserCommandOwner)?
    let webView: WKWebView
    private enum FocusDestination { case address, document }
    private var focusDestination = FocusDestination.document
    private var addressSelection: NSRange?
    private weak var documentResponder: NSResponder?
    private let address = NSTextField()
    private let status = NSTextField(labelWithString: "Private browser")
    private var generation: UInt64?
    private var automationOwner: BrowserOwner?
    private var emitAutomation: (@MainActor (BrowserHostEvent) -> Void)?
    private struct PendingDownload {
        // Retain the exact policy object until WebKit converts it; object IDs alone
        // could otherwise be reused by another navigation.
        let policy: AnyObject
        let token: BrowserDownloadToken
        let expiry: Task<Void, Never>
    }
    private var pendingDownloads: [ObjectIdentifier: PendingDownload] = [:]
    private var closed = false
    private var processTerminated = false
    private var currentNavigation: WKNavigation?
    private var lastOperationID: UInt64 = 0
    private var titleObservation: NSKeyValueObservation?
    // KVO only signals a change. One consumer reads current WebKit state and owner
    // on MainActor, coalescing page-controlled title churn into a bounded slot.
    private let titleEvents = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
    private var titleTask: Task<Void, Never>?
    private var documentReady = false
    private struct PageObservation: Equatable {
        let owner: BrowserOwner
        let url: String
        let title: String
    }
    private var lastObservation: PageObservation?
    private struct PendingAutomation {
        let request: NativeBrowserRequest
        var owner: BrowserOwner
        let deadline: ContinuousClock.Instant
        var navigation: WKNavigation?
        var timer: Task<Void, Never>?
        var waitTask: Task<Void, Never>?
        var isolatedEval = false
        var mutationStarted = false
    }
    private var operations: [UInt64: PendingAutomation] = [:]
    // A timed-out page promise cannot be cancelled by WebKit. Keep its actual
    // callback slot charged until WebKit resolves it, so retries cannot grow it.
    private var javaScriptInFlight: Set<UInt64> = []
    private lazy var back = IconButton(symbol: "chevron.left", label: "Back", target: self, action: #selector(goBack))
    private lazy var forward = IconButton(symbol: "chevron.right", label: "Forward", target: self, action: #selector(goForward))
    private lazy var reload = IconButton(symbol: "arrow.clockwise", label: "Reload or stop", target: self, action: #selector(reloadPage))

    let profileID: UUID?
    init(surface: Surface, owner: any BrowserCommandOwner, dataStore: WKWebsiteDataStore) {
        profileID = surface.browserProfileId
        surfaceID = surface.id
        focusDestination = surface.url == nil || surface.url == "about:blank" ? .address : .document
        self.owner = owner
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = dataStore
        webView = WKWebView(frame: .zero, configuration: configuration)
        super.init(color: ZmuxPalette.chrome)
        webView.navigationDelegate = self
        webView.underPageBackgroundColor = ZmuxPalette.terminal
        address.placeholderString = "Enter a URL"
        address.font = .systemFont(ofSize: 12)
        address.focusRingType = .exterior
        address.target = self; address.action = #selector(submitAddress)
        address.setAccessibilityLabel("Browser address")
        status.font = .systemFont(ofSize: 10); status.textColor = ZmuxPalette.subdued
        status.lineBreakMode = .byTruncatingTail
        let toolbar = NSStackView(views: [back, forward, reload, address])
        toolbar.orientation = .horizontal; toolbar.spacing = 4
        for button in [back, forward, reload] {
            button.widthAnchor.constraint(equalToConstant: 26).isActive = true
            button.heightAnchor.constraint(equalToConstant: 26).isActive = true
        }
        address.setContentHuggingPriority(.defaultLow, for: .horizontal)
        for view in [toolbar, webView, status] { view.translatesAutoresizingMaskIntoConstraints = false; addSubview(view) }
        NSLayoutConstraint.activate([
            toolbar.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 6), toolbar.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            toolbar.topAnchor.constraint(equalTo: topAnchor, constant: 5), toolbar.heightAnchor.constraint(equalToConstant: 28),
            webView.leadingAnchor.constraint(equalTo: leadingAnchor), webView.trailingAnchor.constraint(equalTo: trailingAnchor),
            webView.topAnchor.constraint(equalTo: toolbar.bottomAnchor, constant: 5), webView.bottomAnchor.constraint(equalTo: status.topAnchor, constant: -4),
            status.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8), status.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            status.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -4), status.heightAnchor.constraint(equalToConstant: 15)
        ])
        let click = NSClickGestureRecognizer(target: self, action: #selector(focusSurface))
        click.delaysPrimaryMouseButtonEvents = false
        addGestureRecognizer(click)
        let stream = titleEvents.stream
        titleTask = Task { [weak self] in
            for await _ in stream {
                guard !Task.isCancelled else { break }
                self?.publishPageObservation()
            }
        }
        update(surface)
    }
    required init?(coder: NSCoder) { nil }
    func update(_ surface: Surface) {
        guard !closed else { return }
        guard generation != surface.navigation else { return }
        let previous = automationOwner
        generation = surface.navigation
        if previous != nil { advanceDocument(causedBy: nil, previousOwner: previous) }
        guard let value = surface.url, let url = URL(string: value), Self.allowed(url) else {
            showError("This address is not supported"); return
        }
        address.stringValue = value == "about:blank" ? "" : value
        status.stringValue = "Loading…"
        if value == "about:blank" {
            currentNavigation = webView.loadHTMLString("<!doctype html><title>New tab · zmux</title><style>body{margin:0;background:#1c1c1e;color:#ddd;font:14px -apple-system;display:grid;place-items:center;min-height:100vh}.hint{text-align:center}h1{font-size:26px;font-weight:500;letter-spacing:-1px}p{color:#888}</style><div class='hint'><h1>zmux</h1><p>Enter a URL above to get started.</p></div>", baseURL: nil)
        } else { currentNavigation = webView.load(URLRequest(url: url)) }
    }
    func requestZoom(_ operation: BrowserZoom) {
        guard !closed else { return }
        onAutomationAction?(.zoom(operation: operation))
    }
    func findInPage() {
        let alert = NSAlert()
        alert.messageText = "Find in page"
        let field = NSTextField(string: "")
        field.frame = NSRect(x: 0, y: 0, width: 280, height: 24)
        alert.accessoryView = field
        alert.addButton(withTitle: "Find"); alert.addButton(withTitle: "Cancel")
        if alert.runModal() == .alertFirstButtonReturn {
            let configuration = WKFindConfiguration()
            configuration.caseSensitive = false
            webView.find(field.stringValue, configuration: configuration) { [weak self] result in
                self?.status.stringValue = result.matchFound ? "Match found" : "No matches"
            }
        }
    }
    func focusAddress() {
        guard let window, window.isKeyWindow, !isHiddenOrHasHiddenAncestor else { return }
        focusDestination = .address
        addressSelection = nil
        window.makeFirstResponder(address)
        address.selectText(nil)
    }
    /// Capture only an actual AppKit responder owned by this presented host.
    /// DOM focus in a hidden document cannot alter the native focus preference.
    @discardableResult
    func rememberOwnedFocus(in candidate: NSWindow) -> Bool {
        guard window === candidate, let responder = candidate.firstResponder else { return false }
        if responder === address || responder === address.currentEditor() {
            focusDestination = .address
            addressSelection = (address.currentEditor() as? NSTextView)?.selectedRange()
            return true
        }
        if responder === webView || (responder as? NSView)?.isDescendant(of: webView) == true {
            focusDestination = .document
            documentResponder = responder
            return true
        }
        return false
    }
    /// Selection/layout reconciliation calls this only for its selected browser.
    func restorePreferredFocus() -> Bool {
        guard let window, window.isKeyWindow, !isHiddenOrHasHiddenAncestor else { return false }
        switch focusDestination {
        case .address:
            guard window.makeFirstResponder(address) else { return false }
            if let range = addressSelection, let editor = address.currentEditor() as? NSTextView {
                let length = (address.stringValue as NSString).length
                let location = min(range.location, length)
                editor.setSelectedRange(NSRange(location: location, length: min(range.length, length - location)))
            } else { address.selectText(nil) }
            return true
        case .document:
            if let responder = documentResponder,
               responder === webView || (responder as? NSView)?.isDescendant(of: webView) == true,
               window.makeFirstResponder(responder) { return true }
            return window.makeFirstResponder(webView)
        }
    }
    func bindAutomation(epoch: UUID, identityNamespace: UUID, emit: @escaping @MainActor (BrowserHostEvent) -> Void) {
        guard !closed else { return }
        if let current = automationOwner, current.epoch == epoch, current.identityNamespace == identityNamespace {
            emitAutomation = emit
            return
        }
        if let current = automationOwner {
            cancelPendingDownloads()
            owner?.cancelBrowserDownloads(host: hostID)
            cancelOperations(code: "host_replaced", message: "Browser runtime owner changed")
            emitAutomation?(.detach(owner: current))
        }
        emitAutomation = emit
        let current = BrowserOwner(epoch: epoch, identityNamespace: identityNamespace, surfaceId: surfaceID, hostId: hostID, documentGeneration: documentGeneration, modelNavigation: generation ?? 0)
        automationOwner = current
        emit(.attach(owner: current))
        observeTitle()
    }
    /// Reannounce a complete owner after bounded transport loss. Rust accepts only
    /// the same host with a nondecreasing generation; this cannot revive old work.
    func automationAttachment() -> BrowserHostEvent? {
        guard !closed, let owner = automationOwner, owner.documentGeneration > 0 else { return nil }
        return .attach(owner: owner)
    }
    /// Recovery publishes the current page after reattachment, so a dropped final
    /// title signal cannot leave the Rust model behind WebKit indefinitely.
    func automationObservation() -> BrowserHostEvent? {
        guard let observation = pageObservation() else { return nil }
        return .observed(owner: observation.owner, url: observation.url, title: observation.title)
    }
    private func pageObservation() -> PageObservation? {
        guard !closed, !processTerminated, documentReady, !webView.isLoading,
              let current = automationOwner, current.modelNavigation == generation,
              let url = webView.url ?? URL(string: "about:blank"), Self.allowed(url),
              url.absoluteString.utf8.count <= 8192 else { return nil }
        // Scalar bounding keeps the raw payload within 4096 UTF-8 bytes even for
        // a page-controlled grapheme containing many combining characters.
        let title = String(String.UnicodeScalarView((webView.title ?? "Browser").unicodeScalars.prefix(1024)))
        return PageObservation(owner: current, url: url.absoluteString, title: title)
    }
    private func publishPageObservation() {
        guard let observation = pageObservation(),
              observation != lastObservation else { return }
        lastObservation = observation
        emitAutomation?(.observed(owner: observation.owner, url: observation.url, title: observation.title))
    }
    private func observeTitle() {
        titleObservation?.invalidate(); titleObservation = nil
        lastObservation = nil
        guard !closed, documentReady, automationOwner != nil else { return }
        let continuation = titleEvents.continuation
        titleObservation = webView.observe(\.title, options: [.new]) { @Sendable [continuation] _, _ in
            _ = continuation.yield(())
        }
        publishPageObservation()
    }
    private func invalidateTitleObservation() {
        documentReady = false
        titleObservation?.invalidate(); titleObservation = nil
        lastObservation = nil
    }
    /// Toolbar and menus enter the same Rust admission/receipt route as the CLI.
    func requestNavigation(_ action: BrowserAction) {
        guard action.isNavigation, !closed else { return }
        onAutomationAction?(action)
    }
    func close() {
        guard !closed else { return }
        closed = true
        cancelPendingDownloads()
        owner?.cancelBrowserDownloads(host: hostID)
        invalidateTitleObservation()
        titleEvents.continuation.finish(); titleTask?.cancel(); titleTask = nil
        cancelOperations(code: "host_closed", message: "Browser surface was closed")
        if let current = automationOwner { emitAutomation?(.detach(owner: current)) }
        automationOwner = nil
        currentNavigation = nil
        webView.stopLoading()
        if pendingDownloads.isEmpty { webView.navigationDelegate = nil }
        webView.removeFromSuperview()
    }
    /// Starts exactly once; asynchronous WebKit callbacks cannot replay a timed-out operation.
    func executeAutomation(_ request: NativeBrowserRequest) {
        guard !closed, let current = automationOwner, request.owner == current else {
            emitAutomation?(.fail(operationId: request.operationId, owner: request.owner, error: .init(code: "stale_owner", message: "Browser owner or document changed before dispatch", outcomeUnknown: false)))
            return
        }
        guard request.operationId > lastOperationID else {
            emitAutomation?(.fail(operationId: request.operationId, owner: current, error: .init(code: "already_dispatched", message: "Browser operation will not be replayed", outcomeUnknown: true)))
            return
        }
        lastOperationID = request.operationId
        guard operations.count < 8, request.timeoutMs > 0, request.timeoutMs <= 30_000 else {
            emitAutomation?(.fail(operationId: request.operationId, owner: current, error: .init(code: "host_busy", message: "Browser operation capacity or timeout limit exceeded", outcomeUnknown: false)))
            return
        }
        operations[request.operationId] = PendingAutomation(request: request, owner: current,
            deadline: ContinuousClock().now.advanced(by: .milliseconds(Int64(request.timeoutMs))))
        let id = request.operationId
        let timer = Task { [weak self] in
            do { try await Task.sleep(for: .milliseconds(Int64(request.timeoutMs))) } catch { return }
            guard let self, let pending = self.operations[id] else { return }
            self.failOperation(id, code: "browser_timeout", message: "Browser operation deadline elapsed; the operation will not be replayed", outcomeUnknown: pending.request.action.isNavigation || pending.mutationStarted)
        }
        operations[id]?.timer = timer
        if request.action.isNavigation {
            startNavigation(id)
        } else if processTerminated {
            failOperation(id, code: "web_process_terminated", message: "Web content process stopped; reload to recover", outcomeUnknown: false)
        } else if case .zoom(let operation) = request.action {
            executeZoom(id, operation: operation)
        } else if webView.isLoading {
            if request.action.isWait { scheduleWait(id) }
            else { failOperation(id, code: "document_loading", message: "Document is loading; wait for navigation completion", outcomeUnknown: false) }
        } else if case .eval = request.action {
            preflightEval(id)
        } else {
            runJavaScript(id, action: request.action)
        }
    }
    private func executeZoom(_ id: UInt64, operation: BrowserZoom) {
        guard let pending = operations[id], beforeDeadline(id, pending) else { return }
        let candidate: Double
        switch operation {
        case .in: candidate = webView.pageZoom + 0.1
        case .out: candidate = webView.pageZoom - 0.1
        case .reset: candidate = 1
        case .set(let value): candidate = value
        }
        guard candidate.isFinite else {
            failOperation(id, code: "invalid_argument", message: "Browser zoom factor must be finite", outcomeUnknown: false)
            return
        }
        operations[id]?.mutationStarted = true
        webView.pageZoom = min(5, max(0.25, candidate))
        completeOperation(id, value: .object(["kind": .string("page_zoom"), "value": .number(webView.pageZoom)]))
    }
    private func startNavigation(_ id: UInt64) {
        guard let operation = operations[id] else { return }
        guard beforeDeadline(id, operation) else { return }
        guard !operations.contains(where: { $0.key != id && $0.value.navigation != nil }) else {
            failOperation(id, code: "navigation_pending", message: "Another browser navigation is still pending", outcomeUnknown: false)
            return
        }
        let navigation: WKNavigation?
        switch operation.request.action {
        case .back:
            guard webView.canGoBack else { failOperation(id, code: "no_history", message: "No previous browser history entry", outcomeUnknown: false); return }
            navigation = webView.goBack()
        case .forward:
            guard webView.canGoForward else { failOperation(id, code: "no_history", message: "No next browser history entry", outcomeUnknown: false); return }
            navigation = webView.goForward()
        case .reload: navigation = webView.reload()
        default: return
        }
        guard let navigation else { failOperation(id, code: "navigation_rejected", message: "WebKit did not start navigation", outcomeUnknown: false); return }
        operations[id]?.navigation = navigation
        currentNavigation = navigation
    }
    private func advanceDocument(causedBy operationID: UInt64?, previousOwner: BrowserOwner? = nil) {
        invalidateTitleObservation()
        guard let previous = previousOwner ?? automationOwner else {
            if documentGeneration < UInt64.max { documentGeneration += 1 }
            return
        }
        guard documentGeneration < UInt64.max else { close(); return }
        documentGeneration += 1
        let current = BrowserOwner(epoch: previous.epoch, identityNamespace: previous.identityNamespace, surfaceId: surfaceID, hostId: hostID, documentGeneration: documentGeneration, modelNavigation: generation ?? 0)
        for id in Array(operations.keys) {
            if id == operationID { operations[id]?.owner = current }
            else { failOperation(id, code: "document_changed", message: "Browser document changed during the operation", outcomeUnknown: operations[id]?.request.action.outcomeMayBeUnknown ?? false) }
        }
        automationOwner = current
        emitAutomation?(.documentChanged(owner: previous, documentGeneration: documentGeneration, modelNavigation: current.modelNavigation, causedBy: operationID))
    }
    private func cancelOperations(code: String, message: String) {
        for id in Array(operations.keys) {
            failOperation(id, code: code, message: message, outcomeUnknown: operations[id]?.request.action.outcomeMayBeUnknown ?? false)
        }
    }
    private func navigationOperation(_ navigation: WKNavigation?) -> UInt64? {
        guard let navigation else { return nil }
        return operations.first { $0.value.navigation === navigation }?.key
    }
    private func failOperation(_ id: UInt64, code: String, message: String, outcomeUnknown: Bool) {
        guard let operation = operations.removeValue(forKey: id) else { return }
        operation.timer?.cancel()
        operation.waitTask?.cancel()
        emitAutomation?(.fail(operationId: id, owner: operation.owner, error: .init(code: code, message: String(message.prefix(1024)), outcomeUnknown: outcomeUnknown)))
    }
    private func completeOperation(_ id: UInt64, value: BrowserJSON) {
        guard let operation = operations[id] else { return }
        guard beforeDeadline(id, operation) else { return }
        guard !closed, automationOwner == operation.owner else {
            failOperation(id, code: "document_changed", message: "Browser document changed before completion", outcomeUnknown: operation.request.action.outcomeMayBeUnknown)
            return
        }
        var fields: [String: BrowserJSON]
        if case .object(let object) = value { fields = object } else { fields = ["value": value] }
        fields["surface_id"] = .unsigned(surfaceID)
        fields["host_id"] = .string(hostID.uuidString.lowercased())
        fields["document_generation"] = .unsigned(documentGeneration)
        if case .eval = operation.request.action {
            fields["content_world"] = .string(operation.isolatedEval ? "isolated" : "page")
            if operation.isolatedEval {
                fields["content_world_note"] = .string("Page CSP blocked dynamic evaluation. The script ran once in an isolated content world, which shares the DOM but cannot access page JavaScript globals.")
            }
        }
        else if case .zoom = operation.request.action { fields["content_world"] = .string("native") }
        else { fields["content_world"] = .string("isolated") }
        let result = BrowserJSON.object(fields)
        do {
            guard try JSONEncoder().encode(result).count <= BrowserAutomationScript.resultBytes else {
                failOperation(id, code: "result_too_large", message: "Browser result exceeds the byte limit", outcomeUnknown: operation.request.action.outcomeMayBeUnknown)
                return
            }
        } catch { failOperation(id, code: "invalid_result", message: error.localizedDescription, outcomeUnknown: operation.request.action.outcomeMayBeUnknown); return }
        operations.removeValue(forKey: id)?.timer?.cancel()
        operation.waitTask?.cancel()
        emitAutomation?(.complete(operationId: id, owner: operation.owner, result: result))
    }
    private func preflightEval(_ id: UInt64) {
        guard let operation = operations[id] else { return }
        guard beforeDeadline(id, operation) else { return }
        guard !javaScriptInFlight.contains(id), javaScriptInFlight.count < 8 else {
            failOperation(id, code: "host_busy", message: "WebKit still owns the maximum number of pending JavaScript callbacks", outcomeUnknown: false)
            return
        }
        let expectedOwner = operation.owner
        javaScriptInFlight.insert(id)
        webView.callAsyncJavaScript(BrowserAutomationScript.evalPreflight, arguments: [:], in: nil, in: .page) { [weak self] result in
            guard let self else { return }
            self.javaScriptInFlight.remove(id)
            guard let pending = self.operations[id] else { return }
            guard self.beforeDeadline(id, pending) else { return }
            guard !self.closed, self.automationOwner == expectedOwner, pending.owner == expectedOwner else {
                self.failOperation(id, code: "document_changed", message: "Browser document changed before user evaluation started", outcomeUnknown: false)
                return
            }
            let isolated: Bool
            switch result {
            case .success(let value):
                if let state = value as? String, state == "allowed" { isolated = false }
                else if let state = value as? String, state == "csp_blocked" { isolated = true }
                else {
                    self.failOperation(id, code: "javascript_preflight_failed", message: "Page dynamic evaluation capability could not be established; user script was not executed", outcomeUnknown: false)
                    return
                }
            case .failure(let error):
                guard BrowserAutomationScript.isNativeCSPDenial(error) else {
                    self.failOperation(id, code: "javascript_preflight_failed", message: "User script was not executed: " + error.localizedDescription, outcomeUnknown: false)
                    return
                }
                isolated = true
            }
            self.operations[id]?.isolatedEval = isolated
            self.runJavaScript(id, action: pending.request.action)
        }
    }
    private func runJavaScript(_ id: UInt64, action: BrowserAction) {
        guard let operation = operations[id] else { return }
        guard beforeDeadline(id, operation) else { return }
        guard !javaScriptInFlight.contains(id), javaScriptInFlight.count < 8 else {
            failOperation(id, code: "host_busy", message: "WebKit still owns the maximum number of pending JavaScript callbacks", outcomeUnknown: false)
            return
        }
        struct Input: Encodable { let action: BrowserAction; let hostId: String; let documentGeneration: String }
        let encoder = JSONEncoder(); encoder.keyEncodingStrategy = .convertToSnakeCase
        let payload: String
        do {
            payload = String(decoding: try encoder.encode(Input(action: action, hostId: hostID.uuidString.lowercased(), documentGeneration: String(operation.owner.documentGeneration))), as: UTF8.self)
        } catch { failOperation(id, code: "invalid_argument", message: error.localizedDescription, outcomeUnknown: false); return }
        let world: WKContentWorld
        if case .eval = action {
            world = operation.isolatedEval ? BrowserAutomationScript.isolatedWorld : .page
            operations[id]?.mutationStarted = true
        } else { world = BrowserAutomationScript.isolatedWorld }
        switch action {
        case .fill, .click: operations[id]?.mutationStarted = true
        default: break
        }
        javaScriptInFlight.insert(id)
        webView.callAsyncJavaScript(BrowserAutomationScript.source, arguments: ["payload": payload], in: nil, in: world) { [weak self] result in
            guard let self else { return }
            self.javaScriptInFlight.remove(id)
            guard let pending = self.operations[id] else { return }
            guard self.beforeDeadline(id, pending) else { return }
            guard !self.closed, self.automationOwner == pending.owner else {
                self.failOperation(id, code: "document_changed", message: "Browser document changed during JavaScript execution", outcomeUnknown: pending.request.action.outcomeMayBeUnknown)
                return
            }
            switch result {
            case .failure(let error): self.failOperation(id, code: "javascript_failed", message: error.localizedDescription, outcomeUnknown: pending.request.action.outcomeMayBeUnknown)
            case .success(let value):
                guard let text = value as? String, text.utf8.count <= BrowserAutomationScript.resultBytes else {
                    self.failOperation(id, code: "result_too_large", message: "Browser returned an invalid or oversized JSON result", outcomeUnknown: pending.request.action.outcomeMayBeUnknown); return
                }
                do {
                    // Validate the Foundation tree before recursive Codable decoding;
                    // explicit page-world eval can replace JSON.stringify itself.
                    let untyped = try JSONSerialization.jsonObject(with: Data(text.utf8))
                    var stack: [(Any, Int)] = [(untyped, 0)]
                    var count = 0
                    while let (item, depth) = stack.popLast() {
                        count += 1
                        guard depth <= 32, count <= 4096 else {
                            throw BrowserAutomationFailure(code: "result_too_large", message: "Browser result exceeds its structural limit", outcomeUnknown: pending.request.action.outcomeMayBeUnknown)
                        }
                        if let array = item as? [Any] { stack.append(contentsOf: array.map { ($0, depth + 1) }) }
                        else if let object = item as? [String: Any] { stack.append(contentsOf: object.values.map { ($0, depth + 1) }) }
                    }
                    let json = try JSONDecoder().decode(BrowserJSON.self, from: Data(text.utf8))
                    if case .object(let object) = json, case .object(let error)? = object["error"], case .string(let code)? = error["code"], case .string(let message)? = error["message"] {
                        self.failOperation(id, code: code, message: message, outcomeUnknown: pending.request.action.outcomeMayBeUnknown)
                    } else if pending.request.action.isWait {
                        guard case .object(let root) = json, case .object(let value)? = root["value"],
                              case .bool(let matched)? = value["matched"] else {
                            throw BrowserAutomationFailure(code: "invalid_result", message: "Wait probe did not return a predicate result", outcomeUnknown: false)
                        }
                        if matched { self.completeOperation(id, value: json) }
                        else { self.scheduleWait(id) }
                    } else { self.completeOperation(id, value: json) }
                } catch let failure as BrowserAutomationFailure {
                    self.failOperation(id, code: failure.code, message: failure.message, outcomeUnknown: failure.outcomeUnknown)
                } catch { self.failOperation(id, code: "invalid_result", message: error.localizedDescription, outcomeUnknown: pending.request.action.outcomeMayBeUnknown) }
            }
        }
    }
    private func beforeDeadline(_ id: UInt64, _ operation: PendingAutomation) -> Bool {
        guard ContinuousClock().now < operation.deadline else {
            failOperation(id, code: "browser_timeout", message: "Browser operation deadline elapsed; the operation will not be replayed", outcomeUnknown: operation.request.action.isNavigation || operation.mutationStarted)
            return false
        }
        return true
    }
    private func scheduleWait(_ id: UInt64) {
        guard let operation = operations[id], operation.request.action.isWait,
              beforeDeadline(id, operation) else { return }
        operation.waitTask?.cancel()
        operations[id]?.waitTask = Task { [weak self] in
            do { try await Task.sleep(for: .milliseconds(100)) } catch { return }
            guard let self, let pending = self.operations[id] else { return }
            self.operations[id]?.waitTask = nil
            guard !self.closed, self.automationOwner == pending.owner, self.beforeDeadline(id, pending) else { return }
            if self.webView.isLoading { self.scheduleWait(id) }
            else { self.runJavaScript(id, action: pending.request.action) }
        }
    }
    @objc private func submitAddress() {
        var value = address.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        if value.isEmpty { value = "about:blank" }
        else if !value.contains("://") && value != "about:blank" { value = "https://" + value }
        guard let url = URL(string: value), Self.allowed(url) else { showError("Enter an http or https address"); return }
        owner?.navigateBrowser(surfaceID, url: url.absoluteString)
        focusDestination = .document
        window?.makeFirstResponder(webView)
    }
    @objc private func focusSurface() { owner?.selectTab(surfaceID) }
    @objc private func goBack() { requestNavigation(.back(snapshotAfter: false)) }
    @objc private func goForward() { requestNavigation(.forward(snapshotAfter: false)) }
    @objc private func reloadPage() { requestNavigation(.reload(snapshotAfter: false)) }
    private static func allowed(_ url: URL) -> Bool {
        if url.absoluteString == "about:blank" { return true }
        return ["https", "http"].contains(url.scheme?.lowercased() ?? "") && url.host?.isEmpty == false && url.user == nil && url.password == nil
    }
    private func showError(_ message: String) { status.stringValue = message; status.textColor = .systemRed }
    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction) async -> WKNavigationActionPolicy {
        guard !closed else { return .cancel }
        if navigationAction.shouldPerformDownload {
            guard let url = navigationAction.request.url, Self.allowedDownload(url),
                  await reserveDownload(policy: navigationAction, filename: url.lastPathComponent) else { return .cancel }
            return .download
        }
        guard let url = navigationAction.request.url, Self.allowed(url) else { showError("This link type is not supported"); return .cancel }
        if navigationAction.targetFrame == nil {
            if let pane = owner?.snapshot?.model.workspaces.flatMap({ $0.root.panes }).first(where: { $0.surfaces.contains(where: { $0.id == surfaceID }) }) {
                owner?.createBrowser(in: pane.id, url: url.absoluteString, profile: profileID)
            }
            return .cancel
        }
        return .allow
    }
    func webView(_ webView: WKWebView, decidePolicyFor navigationResponse: WKNavigationResponse) async -> WKNavigationResponsePolicy {
        guard !closed else { return .cancel }
        let response = navigationResponse.response
        let attachment = (response as? HTTPURLResponse)?.value(forHTTPHeaderField: "Content-Disposition")?
            .split(separator: ";", maxSplits: 1).first?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() == "attachment"
        if !navigationResponse.canShowMIMEType || attachment {
            guard let url = response.url, Self.allowedDownload(url),
                  await reserveDownload(policy: navigationResponse, filename: response.suggestedFilename ?? url.lastPathComponent) else { return .cancel }
            return .download
        }
        return .allow
    }
    private static func allowedDownload(_ url: URL) -> Bool {
        allowed(url) && ["https", "http"].contains(url.scheme?.lowercased() ?? "")
    }
    private func reserveDownload(policy: AnyObject, filename: String) async -> Bool {
        guard !closed, pendingDownloads.count < 4, let captured = automationOwner, let owner else {
            showError("Download owner or capacity is unavailable."); return false
        }
        do {
            let token = try await owner.reserveBrowserDownload(origin: captured, profile: profileID, webView: webView, conversionOwner: self, filename: filename)
            // Capture precedes admission. A document/host change while Rust validates
            // cannot silently assign this conversion to the replacement page.
            guard !closed, !processTerminated, automationOwner == captured else {
                owner.abandonBrowserDownload(token); return false
            }
            let key = ObjectIdentifier(policy)
            let expiry = Task { [weak self] in
                do { try await Task.sleep(for: .seconds(30)) } catch { return }
                guard let self, let pending = self.pendingDownloads[key] else { return }
                self.owner?.cancelBrowserDownloadConversion(pending.token)
                self.showError("WebKit did not deliver the download conversion.")
            }
            pendingDownloads[key] = PendingDownload(policy: policy, token: token, expiry: expiry)
            for id in Array(operations.keys) where operations[id]?.request.action.isNavigation == true {
                failOperation(id, code: "navigation_became_download", message: "Navigation became a download; observe it through browser download list or wait.", outcomeUnknown: false)
            }
            currentNavigation = nil
            status.stringValue = "Download started"; status.textColor = ZmuxPalette.subdued
            return true
        } catch { showError(error.localizedDescription); return false }
    }
    private func adoptDownload(_ download: WKDownload, policy: AnyObject) {
        guard let pending = pendingDownloads.removeValue(forKey: ObjectIdentifier(policy)) else {
            download.cancel { _ in }; showError("An unowned download was refused."); return
        }
        pending.expiry.cancel()
        owner?.adoptBrowserDownload(download, token: pending.token, webView: webView)
        if closed && pendingDownloads.isEmpty { webView.navigationDelegate = nil }
    }
    private func cancelPendingDownloads() {
        for pending in pendingDownloads.values {
            pending.expiry.cancel()
            owner?.cancelBrowserDownloadConversion(pending.token)
        }
        // Retain exact policy objects and the navigation delegate until didBecome.
        // A close/timeout cannot pretend the not-yet-delivered object is gone.
    }
    func webView(_ webView: WKWebView, navigationAction: WKNavigationAction, didBecome download: WKDownload) {
        adoptDownload(download, policy: navigationAction)
    }
    func webView(_ webView: WKWebView, navigationResponse: WKNavigationResponse, didBecome download: WKDownload) {
        adoptDownload(download, policy: navigationResponse)
    }
    func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
        guard !closed else { return }
        currentNavigation = navigation
        processTerminated = false
        let cause = navigationOperation(navigation)
        advanceDocument(causedBy: cause)
        status.stringValue = "Loading…"; status.textColor = ZmuxPalette.subdued
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        guard !closed else { return }
        if let currentNavigation, currentNavigation !== navigation { return }
        back.isEnabled = webView.canGoBack; forward.isEnabled = webView.canGoForward
        status.stringValue = "Private browser"; status.textColor = ZmuxPalette.subdued
        if let url = webView.url ?? URL(string: "about:blank"), Self.allowed(url) {
            address.stringValue = url.absoluteString == "about:blank" ? "" : url.absoluteString
        }
        documentReady = true
        observeTitle()
        if let id = navigationOperation(navigation), let operation = operations[id] {
            if operation.request.action.snapshotAfter { runJavaScript(id, action: .snapshot(options: .init())) }
            else { completeOperation(id, value: .object(["value": .object(["url": .string(webView.url?.absoluteString ?? "about:blank"), "title": .string(webView.title ?? "")]), "truncated": .bool(false)])) }
        }
    }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) {
        guard !closed else { return }
        if let id = navigationOperation(navigation) { failOperation(id, code: "navigation_failed", message: error.localizedDescription, outcomeUnknown: true) }
        if (error as NSError).code != NSURLErrorCancelled { showError(error.localizedDescription) }
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) {
        guard !closed else { return }
        if let id = navigationOperation(navigation) { failOperation(id, code: "navigation_failed", message: error.localizedDescription, outcomeUnknown: true) }
        showError(error.localizedDescription)
    }
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        guard !closed else { return }
        processTerminated = true
        cancelPendingDownloads()
        owner?.cancelBrowserDownloads(host: hostID)
        advanceDocument(causedBy: nil)
        showError("Page stopped unexpectedly. Reload to recover.")
    }

}

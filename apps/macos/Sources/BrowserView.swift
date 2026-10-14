import AppKit
import WebKit

/// Public WebKit host. Pages receive no message handler, native socket or terminal bridge.
@MainActor
final class BrowserView: BackgroundView, WKNavigationDelegate {
    let surfaceID: UInt64
    private weak var owner: AppController?
    let webView: WKWebView
    private let address = NSTextField()
    private let status = NSTextField(labelWithString: "Private browser")
    private var generation: UInt64?
    private lazy var back = IconButton(symbol: "chevron.left", label: "Back", target: self, action: #selector(goBack))
    private lazy var forward = IconButton(symbol: "chevron.right", label: "Forward", target: self, action: #selector(goForward))
    private lazy var reload = IconButton(symbol: "arrow.clockwise", label: "Reload or stop", target: self, action: #selector(reloadPage))

    init(surface: Surface, owner: AppController) {
        surfaceID = surface.id
        self.owner = owner
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
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
        update(surface)
    }
    required init?(coder: NSCoder) { nil }
    func update(_ surface: Surface) {
        guard generation != surface.navigation else { return }
        generation = surface.navigation
        guard let value = surface.url, let url = URL(string: value), Self.allowed(url) else {
            showError("This address is not supported"); return
        }
        address.stringValue = value == "about:blank" ? "" : value
        status.stringValue = "Loading…"
        if value == "about:blank" {
            webView.loadHTMLString("<!doctype html><title>New tab · zmux</title><style>body{margin:0;background:#1c1c1e;color:#ddd;font:14px -apple-system;display:grid;place-items:center;min-height:100vh}.hint{text-align:center}h1{font-size:26px;font-weight:500;letter-spacing:-1px}p{color:#888}</style><div class='hint'><h1>zmux</h1><p>Enter a URL above to get started.</p></div>", baseURL: nil)
        } else { webView.load(URLRequest(url: url)) }
    }
    func adjustZoom(_ delta: Double) { webView.pageZoom = min(3, max(0.5, webView.pageZoom + delta)) }
    func resetZoom() { webView.pageZoom = 1 }
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
    func focusAddress() { window?.makeFirstResponder(address); address.selectText(nil) }
    func close() { webView.stopLoading(); webView.navigationDelegate = nil; webView.removeFromSuperview() }
    @objc private func submitAddress() {
        var value = address.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        if value.isEmpty { value = "about:blank" }
        else if !value.contains("://") && value != "about:blank" { value = "https://" + value }
        guard let url = URL(string: value), Self.allowed(url) else { showError("Enter an http or https address"); return }
        owner?.navigateBrowser(surfaceID, url: url.absoluteString)
        window?.makeFirstResponder(webView)
    }
    @objc private func focusSurface() { owner?.selectTab(surfaceID) }
    @objc private func goBack() { webView.goBack() }
    @objc private func goForward() { webView.goForward() }
    @objc private func reloadPage() { if webView.isLoading { webView.stopLoading() } else { webView.reload() } }
    private static func allowed(_ url: URL) -> Bool {
        if url.absoluteString == "about:blank" { return true }
        return ["https", "http"].contains(url.scheme?.lowercased() ?? "") && url.host?.isEmpty == false && url.user == nil && url.password == nil
    }
    private func showError(_ message: String) { status.stringValue = message; status.textColor = .systemRed }
    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction) async -> WKNavigationActionPolicy {
        if navigationAction.shouldPerformDownload { showError("Downloads are not available in this development build"); return .cancel }
        guard let url = navigationAction.request.url, Self.allowed(url) else { showError("This link type is not supported"); return .cancel }
        if navigationAction.targetFrame == nil {
            if let pane = owner?.snapshot?.model.workspaces.flatMap({ $0.root.panes }).first(where: { $0.surfaces.contains(where: { $0.id == surfaceID }) }) {
                owner?.createBrowser(in: pane.id, url: url.absoluteString)
            }
            return .cancel
        }
        return .allow
    }
    func webView(_ webView: WKWebView, decidePolicyFor navigationResponse: WKNavigationResponse) async -> WKNavigationResponsePolicy {
        guard navigationResponse.canShowMIMEType else { showError("Downloads are not available in this development build"); return .cancel }
        return .allow
    }
    func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
        status.stringValue = "Loading…"; status.textColor = ZmuxPalette.subdued
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        back.isEnabled = webView.canGoBack; forward.isEnabled = webView.canGoForward
        status.stringValue = "Private browser"; status.textColor = ZmuxPalette.subdued
        if let url = webView.url ?? URL(string: "about:blank"), Self.allowed(url) {
            address.stringValue = url.absoluteString == "about:blank" ? "" : url.absoluteString
            owner?.observeBrowser(surfaceID, url: url.absoluteString, title: webView.title ?? "Browser")
        }
    }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) {
        if (error as NSError).code != NSURLErrorCancelled { showError(error.localizedDescription) }
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) { showError(error.localizedDescription) }
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { showError("Page stopped unexpectedly. Reload to recover.") }

}

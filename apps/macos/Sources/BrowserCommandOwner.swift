import AppKit
import WebKit

/// Browser resources address the application, independent of the window that
/// currently presents their surface. Pages receive none of these native routes.
@MainActor
protocol BrowserCommandOwner: AnyObject {
    var snapshot: DesktopSnapshot? { get }
    func createBrowser(in pane: UInt64, url: String, profile: UUID?)
    func navigateBrowser(_ id: UInt64, url: String)
    func selectTab(_ id: UInt64)
    func reserveBrowserDownload(origin: BrowserOwner, profile: UUID?, webView: WKWebView, conversionOwner: BrowserView, filename: String) async throws -> BrowserDownloadToken
    func adoptBrowserDownload(_ download: WKDownload, token: BrowserDownloadToken, webView: WKWebView)
    func abandonBrowserDownload(_ token: BrowserDownloadToken)
    func cancelBrowserDownloadConversion(_ token: BrowserDownloadToken)
    func cancelBrowserDownloads(host: UUID)
}

@MainActor
protocol BrowserDownloadOwner: AnyObject {
    func downloadWindow(for origin: BrowserOwner) -> NSWindow?
    func cancelBrowserDownload(_ record: BrowserDownloadRecord)
}

import AppKit

/// Actual public WKWebView pageZoom, through the bundled CLI and shared menu owner.
@MainActor
enum BrowserZoomSmoke {
    private struct Reply: Decodable {
        let kind: String
        let value: Double
        let contentWorld: String
        let surfaceId: UInt64
        let hostId: UUID
        let documentGeneration: UInt64
    }
    private struct Created: Decodable {
        struct Target: Decodable { let surfaceId: UInt64 }
        let createdSurfaces: [Target]
    }
    private static func decode<T: Decodable>(_ type: T.Type, _ data: Data) throws -> T {
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(type, from: data)
    }
    private static func wait(_ message: String, _ predicate: () -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(8)
        while !predicate() {
            guard ContinuousClock.now < deadline else { throw BridgeFailure(message: message) }
            try await Task.sleep(for: .milliseconds(25))
        }
    }
    private static func selection(_ snapshot: DesktopSnapshot?) -> [UInt64] {
        guard let model = snapshot?.model else { return [] }
        var result = [model.focusedWindow ?? 0, model.focusedWorkspace ?? 0]
        for window in model.windows { result += [window.id, window.selectedWorkspace ?? 0] }
        for workspace in model.workspaces {
            result += [workspace.id, workspace.focusedPane]
            for pane in workspace.root.panes { result += [pane.id, pane.selectedSurface] }
        }
        return result
    }
    static func validate(
        surface: UInt64,
        cli: @escaping @MainActor ([String], Bool) async throws -> Data,
        snapshot: @MainActor () -> DesktopSnapshot?,
        browser: @MainActor (UInt64) -> BrowserView?
    ) async throws {
        guard let original = browser(surface) else { throw BridgeFailure(message: "Zoom fixture lost its real WebKit host") }
        let initial = original.webView.pageZoom
        let selected = selection(snapshot())
        let host = original.hostID
        let generation = original.documentGeneration
        var hidden: UInt64?
        @MainActor func command(_ operand: String, target: UInt64, expected: Double) async throws {
            guard let view = browser(target) else { throw BridgeFailure(message: "Zoom target lost its WebKit host") }
            let reply = try decode(Reply.self, await cli(["browser", "zoom", operand, "--surface", String(target)], false))
            guard reply.kind == "page_zoom", reply.contentWorld == "native",
                  reply.surfaceId == target, reply.hostId == view.hostID,
                  reply.documentGeneration == view.documentGeneration,
                  abs(reply.value - expected) < 0.000001,
                  abs(view.webView.pageZoom - expected) < 0.000001 else {
                throw BridgeFailure(message: "Zoom receipt disagrees with its exact native WKWebView")
            }
        }
        do {
            for (operand, expected) in [("1.25", 1.25), ("in", 1.35), ("out", 1.25), ("reset", 1.0),
                                        ("5", 5.0), ("in", 5.0), ("0.25", 0.25), ("out", 0.25),
                                        ("-1", 0.25), ("0", 0.25), ("10", 5.0), ("1e308", 5.0), ("reset", 1.0)] {
                try await command(operand, target: surface, expected: expected)
            }
            for invalid in ["NaN", "inf", "-inf"] {
                let result = try await cli(["browser", "zoom", invalid, "--surface", String(surface)], true)
                guard String(decoding: result, as: UTF8.self).contains("must be finite"),
                      original.webView.pageZoom == 1 else {
                    throw BridgeFailure(message: "Invalid zoom mutated WebKit or failed for an unrelated reason")
                }
            }
            // The same production method called by native menu actions must
            // enqueue shared admission, rather than immediately mutate WebKit.
            original.requestZoom(.in)
            guard original.webView.pageZoom == 1 else { throw BridgeFailure(message: "Menu zoom bypassed shared admission") }
            try await wait("Shared menu zoom did not reach the real WebKit host") { abs(original.webView.pageZoom - 1.1) < 0.000001 }
            original.requestZoom(.reset)
            try await wait("Shared menu zoom reset did not complete") { original.webView.pageZoom == 1 }
            let created = try decode(Created.self, await cli(["browser", "open", "--no-focus", "about:blank"], false))
            guard let id = created.createdSurfaces.first?.surfaceId else { throw BridgeFailure(message: "Hidden zoom fixture omitted its browser") }
            hidden = id
            try await wait("Hidden zoom browser did not load") { browser(id)?.webView.title != nil && browser(id)?.webView.isLoading == false }
            try await command("1.7", target: id, expected: 1.7)
            guard original.webView.pageZoom == 1, selection(snapshot()) == selected,
                  original.hostID == host, original.documentGeneration == generation else {
                throw BridgeFailure(message: "Zoom changed selection, document generation or another tab's zoom")
            }
            _ = try await cli(["close-surface", "--surface", String(id)], false)
            hidden = nil
            try await command(String(initial), target: surface, expected: initial)
            FileHandle.standardError.write(Data("Native browser zoom: CLI/menu steps, limits, hidden host, exact receipts and focus retention passed\n".utf8))
        } catch {
            if let hidden {
                do { _ = try await cli(["close-surface", "--surface", String(hidden)], false) }
                catch { FileHandle.standardError.write(Data(("Zoom fixture cleanup: " + error.localizedDescription + "\n").utf8)) }
            }
            throw error
        }
    }
}

import AppKit

/// Public CLI activation on an actual hidden WebKit host, including navigation.
@MainActor
enum BrowserClickSmoke {
    private struct Created: Decodable {
        struct Target: Decodable { let surfaceId: UInt64 }
        let createdSurfaces: [Target]
    }
    private struct Reply: Decodable {
        struct Value: Decodable { let dispatched: Bool; let ref: String }
        struct Snapshot: Decodable { let nodes: [BrowserJSON] }
        let value: Value
        let snapshot: Snapshot?
        let surfaceId: UInt64
        let hostId: UUID
        let documentGeneration: UInt64
        let contentWorld: String
    }
    private struct Envelope<T: Decodable>: Decodable { let value: T }
    private struct Observation: Codable {
        let events: [String]
        let activation: [String]
        let checked: Bool
        let cancelled: Bool
        let cancelledEvents: [String]
        let otherClicks: Int
        let clicks: Int
        let focused: String
        let documentHasFocus: Bool
        let readyState: String
        let visibility: String
    }
    private static func diagnostic(_ observation: Observation) -> String {
        do { return String(decoding: try JSONEncoder().encode(observation), as: UTF8.self) }
        catch { return "Observation encoding failed: " + error.localizedDescription }
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
        origin: String,
        cli: @escaping @MainActor ([String], Bool) async throws -> Data,
        snapshot: @MainActor () -> DesktopSnapshot?,
        browser: @MainActor (UInt64) -> BrowserView?
    ) async throws {
        let selected = selection(snapshot())
        let created = try decode(Created.self, await cli(["browser", "open", "--no-focus", origin + "/"], false))
        guard let id = created.createdSurfaces.first?.surfaceId else { throw BridgeFailure(message: "Click fixture omitted its hidden browser") }
        var hidden: UInt64? = id
        do {
            try await wait("Click fixture browser did not load") { browser(id)?.webView.title == "First fixture" && browser(id)?.webView.isLoading == false }
            guard let view = browser(id) else { throw BridgeFailure(message: "Click fixture lost its real WebKit host") }
            let host = view.hostID, generation = view.documentGeneration
            let target = ["--surface", String(id)]
            let setup = #"""
            document.body.innerHTML = '<button id="first" class="click-many">First</button><button id="other" class="click-many">Other</button><input id="activate" type="checkbox"><input id="cancel" type="checkbox"><button id="disabled" disabled>Disabled</button><a id="next" href="/second">Next page</a>';
            globalThis.zmuxClickEvents = []; globalThis.zmuxActivation = []; globalThis.zmuxCancelled = [];
            globalThis.zmuxClicks = 0; globalThis.zmuxOtherClicks = 0;
            for (const kind of ['pointerover','mouseover','pointerenter','mouseenter','pointermove','mousemove','pointerdown','mousedown','pointerup','mouseup','click']) document.getElementById('first').addEventListener(kind, event => globalThis.zmuxClickEvents.push(event.type));
            document.getElementById('first').addEventListener('click',()=>globalThis.zmuxClicks++);
            document.getElementById('other').addEventListener('click',()=>globalThis.zmuxOtherClicks++);
            for (const kind of ['pointerdown','mousedown']) document.getElementById('activate').addEventListener(kind,event=>event.preventDefault());
            for (const kind of ['click','input','change']) document.getElementById('activate').addEventListener(kind,event=>globalThis.zmuxActivation.push(event.type));
            document.getElementById('cancel').addEventListener('click',event=>{globalThis.zmuxCancelled.push('click');event.preventDefault()});
            for (const kind of ['input','change']) document.getElementById('cancel').addEventListener(kind,event=>globalThis.zmuxCancelled.push(event.type));
            sessionStorage.setItem('zmux-click-navigation-count','0');
            document.getElementById('next').addEventListener('click',()=>sessionStorage.setItem('zmux-click-navigation-count',String(Number(sessionStorage.getItem('zmux-click-navigation-count'))+1)));
            null
            """#
            _ = try await cli(["browser", "eval", setup] + target, false)
            let key = NSApp.keyWindow, responder = NSApp.keyWindow?.firstResponder
            @MainActor func click(_ selector: String, after: Bool = false) async throws -> Reply {
                let reply = try decode(Reply.self, await cli(["browser", "click", "--selector", selector] + target + (after ? ["--snapshot-after"] : []), false))
                guard reply.value.dispatched, reply.surfaceId == id, reply.hostId == host,
                      reply.documentGeneration == generation, reply.contentWorld == "isolated" else {
                    throw BridgeFailure(message: "Click completion lost its exact native host/document receipt")
                }
                return reply
            }
            @MainActor func observe() async throws -> Observation {
                let script = "({events:globalThis.zmuxClickEvents,activation:globalThis.zmuxActivation,checked:document.getElementById('activate').checked,cancelled:document.getElementById('cancel').checked,cancelledEvents:globalThis.zmuxCancelled,otherClicks:globalThis.zmuxOtherClicks,clicks:globalThis.zmuxClicks,focused:document.activeElement.id,documentHasFocus:document.hasFocus(),readyState:document.readyState,visibility:document.visibilityState})"
                return try decode(Envelope<Observation>.self, await cli(["browser", "eval", script] + target, false)).value
            }
            let first = try await click(".click-many", after: true)
            let observed = try await observe()
            let expected = ["pointerover", "mouseover", "pointerenter", "mouseenter", "pointermove", "mousemove", "pointerdown", "mousedown", "pointerup", "mouseup", "click"]
            guard observed.events == expected, observed.clicks == 1, observed.otherClicks == 0,
                  observed.focused == "first", first.snapshot?.nodes.isEmpty == false else {
                throw BridgeFailure(message: "Click lost first-match, event sequence, DOM focus or once-only snapshot behavior: " + diagnostic(observed))
            }
            _ = try await click(first.value.ref)
            _ = try await click("#activate")
            _ = try await click("#cancel")
            let activation = try await observe()
            guard activation.clicks == 2, activation.checked, activation.activation == ["click", "input", "change"],
                  !activation.cancelled, activation.cancelledEvents == ["click"] else {
                throw BridgeFailure(message: "Click changed native default activation or cancellation semantics: " + diagnostic(activation))
            }
            for (selector, code) in [("#disabled", "element_disabled"), ("[", "invalid_selector"), ("#missing", "element_not_found")] {
                let failed = try await cli(["browser", "click", selector] + target, true)
                guard String(decoding: failed, as: UTF8.self).contains(code) else { throw BridgeFailure(message: "Click failed for an unrelated selector reason") }
            }
            // The admitted click is sent exactly once. Its callback may finish
            // before navigation, or document fencing may report uncertainty.
            do { _ = try await click("#next") }
            catch {
                let message = error.localizedDescription
                guard message.contains("outcome may be unknown"), message.contains("not retried"),
                      ["document_changed", "stale_browser_owner", "javascript_failed", "browser_timeout", "exceeded the client deadline"].contains(where: { message.contains($0) }) else { throw error }
            }
            try await wait("Click did not navigate its exact hidden host") {
                view.webView.title == "Second fixture" && view.webView.isLoading == false &&
                snapshot()?.model.workspaces.flatMap({ $0.root.panes }).flatMap({ $0.surfaces }).contains(where: { $0.id == id && $0.title == "Second fixture" }) == true
            }
            let count = try decode(Envelope<Int>.self, await cli(["browser", "eval", "Number(sessionStorage.getItem('zmux-click-navigation-count'))"] + target, false)).value
            let stale = try await cli(["browser", "click", first.value.ref] + target, true)
            guard count == 1, String(decoding: stale, as: UTF8.self).contains("stale_reference"),
                  view.hostID == host, view.documentGeneration > generation,
                  selection(snapshot()) == selected, NSApp.keyWindow === key, key?.firstResponder === responder else {
                throw BridgeFailure(message: "Navigation click replayed, reused a stale ref or changed app selection/focus")
            }
            _ = try await cli(["close-surface", "--surface", String(id)], false)
            hidden = nil
            FileHandle.standardError.write(Data("Native browser click: first match/ref, events/default cancellation, one navigation, fencing and hidden focus passed\n".utf8))
        } catch {
            if let hidden {
                do { _ = try await cli(["close-surface", "--surface", String(hidden)], false) }
                catch { FileHandle.standardError.write(Data(("Click fixture cleanup: " + error.localizedDescription + "\n").utf8)) }
            }
            throw error
        }
    }
}

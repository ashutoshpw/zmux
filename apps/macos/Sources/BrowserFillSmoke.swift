import AppKit

/// Exercise the public bundled CLI against a never-selected, real WebKit host.
@MainActor
enum BrowserFillSmoke {
    private struct Created: Decodable {
        struct Target: Decodable { let surfaceId: UInt64 }
        let createdSurfaces: [Target]
    }
    private struct Reply: Decodable {
        struct Value: Decodable { let filled: Bool; let ref: String }
        struct Snapshot: Decodable { let nodes: [BrowserJSON] }
        let value: Value
        let snapshot: Snapshot?
        let surfaceId: UInt64
        let hostId: UUID
        let documentGeneration: UInt64
        let contentWorld: String
    }
    private struct Envelope<T: Decodable>: Decodable { let value: T }
    private struct Observation: Decodable {
        let values: [String]
        let notes: String
        let editor: String
        let html: String
        let events: [String]
    }
    private static func decode<T: Decodable>(_ type: T.Type, _ data: Data) throws -> T {
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(type, from: data)
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
        cli: @escaping @MainActor ([String], Bool) async throws -> Data,
        snapshot: @MainActor () -> DesktopSnapshot?,
        browser: @MainActor (UInt64) -> BrowserView?
    ) async throws {
        let selected = selection(snapshot())
        let created = try decode(Created.self, await cli(["browser", "open", "--no-focus", "about:blank"], false))
        guard let id = created.createdSurfaces.first?.surfaceId else { throw BridgeFailure(message: "Fill fixture omitted its hidden browser") }
        var hidden: UInt64? = id
        do {
            let deadline = ContinuousClock.now + .seconds(8)
            while browser(id)?.webView.title == nil || browser(id)?.webView.isLoading != false {
                guard ContinuousClock.now < deadline else { throw BridgeFailure(message: "Fill fixture browser did not become ready") }
                try await Task.sleep(for: .milliseconds(25))
            }
            guard let view = browser(id) else { throw BridgeFailure(message: "Fill fixture lost its actual WebKit host") }
            let host = view.hostID, generation = view.documentGeneration
            let target = ["--surface", String(id)]
            let setup = #"""
            document.body.innerHTML = '<input id="first" class="fill-many" value="initial"><input class="fill-many" value="other"><textarea id="notes"></textarea><div id="editor" contenteditable="true">old editor</div>';
            globalThis.zmuxFillEvents = [];
            for (const kind of ['beforeinput','input','change']) document.getElementById('first').addEventListener(kind, event => {
              globalThis.zmuxFillEvents.push(event.type);
              if (event.type === 'beforeinput' && event.data === 'DENY') event.preventDefault();
            });
            null
            """#
            _ = try await cli(["browser", "eval", setup] + target, false)
            let key = NSApp.keyWindow, responder = NSApp.keyWindow?.firstResponder
            @MainActor func observe() async throws -> Observation {
                let script = "({values:Array.from(document.querySelectorAll('input'),node=>node.value),notes:document.getElementById('notes').value,editor:document.getElementById('editor').textContent,html:document.getElementById('editor').innerHTML,events:globalThis.zmuxFillEvents})"
                return try decode(Envelope<Observation>.self, await cli(["browser", "eval", script] + target, false)).value
            }
            @MainActor func fill(_ selector: String, _ text: String, after: Bool = false) async throws -> Reply {
                let args = ["browser", "fill", "--selector", selector, "--text", text] + target + (after ? ["--snapshot-after"] : [])
                let reply = try decode(Reply.self, await cli(args, false))
                guard reply.value.filled, reply.surfaceId == id, reply.hostId == host,
                      reply.documentGeneration == generation, reply.contentWorld == "isolated" else {
                    throw BridgeFailure(message: "Fill completion lost its exact native host/document receipt")
                }
                return reply
            }
            let filled = try await fill(".fill-many", " replacement ", after: true)
            let first = try await observe()
            guard first.values == [" replacement ", "other"], first.events == ["beforeinput", "input", "change"],
                  filled.snapshot?.nodes.isEmpty == false else { throw BridgeFailure(message: "Fill failed first-match, event ordering or single-mutation snapshot behavior") }
            _ = try await fill(filled.value.ref, "")
            let cleared = try await observe()
            guard cleared.values == ["", "other"], cleared.events.count == 6 else { throw BridgeFailure(message: "Empty ref fill did not clear exactly once") }
            let rejected = try await cli(["browser", "fill", filled.value.ref, "DENY"] + target, true)
            let cancelled = try await observe()
            guard String(decoding: rejected, as: UTF8.self).contains("input_rejected"), cancelled.values == ["", "other"],
                  cancelled.events == cleared.events + ["beforeinput"] else { throw BridgeFailure(message: "Fill ignored the page's cancelable beforeinput") }
            let unicode = "  résumé\n雪 🪟 \" \\ "
            _ = try await fill("#notes", unicode)
            _ = try await fill("#editor", "literal <b>text</b>")
            let text = try await observe()
            guard text.notes == unicode, text.editor == "literal <b>text</b>", !text.html.contains("<b>") else {
                throw BridgeFailure(message: "Fill changed Unicode/whitespace or parsed replacement text as HTML")
            }
            _ = try await cli(["browser", "eval", "document.getElementById('first').remove();null"] + target, false)
            for (selector, code) in [(filled.value.ref, "stale_reference"), ("[", "invalid_selector"), ("#missing", "element_not_found")] {
                let failed = try await cli(["browser", "fill", selector, "x"] + target, true)
                guard String(decoding: failed, as: UTF8.self).contains(code) else { throw BridgeFailure(message: "Fill failed for an unrelated selector reason") }
            }
            guard view.hostID == host, view.documentGeneration == generation, selection(snapshot()) == selected,
                  NSApp.keyWindow === key, key?.firstResponder === responder else {
                throw BridgeFailure(message: "Hidden fill changed app focus, selection or native document identity")
            }
            _ = try await cli(["close-surface", "--surface", String(id)], false)
            hidden = nil
            FileHandle.standardError.write(Data("Native browser fill: first match, refs, page cancellation/events, Unicode, snapshot and hidden focus passed\n".utf8))
        } catch {
            if let hidden {
                do { _ = try await cli(["close-surface", "--surface", String(hidden)], false) }
                catch { FileHandle.standardError.write(Data(("Fill fixture cleanup: " + error.localizedDescription + "\n").utf8)) }
            }
            throw error
        }
    }
}

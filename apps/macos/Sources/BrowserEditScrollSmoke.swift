import AppKit

/// Public CLI edits/scrolling on a never-selected real WebKit document.
@MainActor
enum BrowserEditScrollSmoke {
    private struct Created: Decodable {
        struct Target: Decodable { let surfaceId: UInt64 }
        let createdSurfaces: [Target]
    }
    private struct Reply: Decodable {
        struct Value: Decodable { let dispatched: Bool; let ref: String? }
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
        let typed: String
        let editorText: String
        let other: String
        let events: [String]
        let inputTypes: [String]
        let chunks: [String]
        let keyboard: [String]
        let selected: String
        let selectionEvents: [String]
        let active: String
        let left: Double
        let top: Double
        let windowX: Double
        let windowY: Double
        let documentHasFocus: Bool
        let readyState: String
        let visibility: String
    }
    private static func decode<T: Decodable>(_ type: T.Type, _ data: Data) throws -> T {
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(type, from: data)
    }
    private static func diagnostic(_ value: Observation) -> String {
        do { return String(decoding: try JSONEncoder().encode(value), as: UTF8.self) }
        catch { return "Observation encoding failed: " + error.localizedDescription }
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
        var ids = [model.focusedWindow ?? 0, model.focusedWorkspace ?? 0]
        for window in model.windows { ids += [window.id, window.selectedWorkspace ?? 0] }
        for workspace in model.workspaces {
            ids += [workspace.id, workspace.focusedPane]
            for pane in workspace.root.panes { ids += [pane.id, pane.selectedSurface] }
        }
        return ids
    }
    static func validate(
        origin: String,
        cli: @escaping @MainActor ([String], Bool) async throws -> Data,
        snapshot: @MainActor () -> DesktopSnapshot?,
        browser: @MainActor (UInt64) -> BrowserView?
    ) async throws {
        let selected = selection(snapshot())
        let created = try decode(Created.self, await cli(["browser", "open", "--no-focus", origin + "/"], false))
        guard let id = created.createdSurfaces.first?.surfaceId else { throw BridgeFailure(message: "Edit/scroll fixture omitted its hidden browser") }
        var hidden: UInt64? = id
        do {
            try await wait("Edit/scroll fixture did not load") { browser(id)?.webView.title == "First fixture" && browser(id)?.webView.isLoading == false }
            guard let view = browser(id) else { throw BridgeFailure(message: "Edit/scroll fixture lost its real WebKit host") }
            let host = view.hostID, generation = view.documentGeneration
            let target = ["--surface", String(id)]
            let key = NSApp.keyWindow, responder = key?.firstResponder
            let setup = #"""
            document.body.innerHTML = '<input class="many" id="entry" value="seed"><input class="many" id="other" value="other"><div id="editor" contenteditable="true">seed</div><select id="choice"><option value="a">A</option><option value="b">B</option></select><div id="scroller" style="width:150px;height:100px;overflow:scroll"><div style="width:1000px;height:1500px">Content</div></div><div style="position:absolute;left:5000px;top:10000px;width:1px;height:1px"></div>';
            globalThis.zmuxEditScroll = {events:[],inputTypes:[],chunks:[],keyboard:[],selectionEvents:[]};
            const entry = document.getElementById('entry');
            for (const kind of ['beforeinput','input','change']) entry.addEventListener(kind,event=>{zmuxEditScroll.events.push(event.type);if(typeof event.inputType==='string'){zmuxEditScroll.inputTypes.push(event.inputType);zmuxEditScroll.chunks.push(event.data)}});
            for (const kind of ['keydown','keypress','keyup']) entry.addEventListener(kind,event=>zmuxEditScroll.keyboard.push(event.type));
            for (const kind of ['beforeinput','click','input','change']) document.getElementById('choice').addEventListener(kind,event=>zmuxEditScroll.selectionEvents.push(event.type));
            entry.focus();entry.setSelectionRange(0,2);null
            """#
            _ = try await cli(["browser", "eval", setup] + target, false)
            @MainActor func retained() throws {
                guard view.hostID == host, view.documentGeneration == generation,
                      selection(snapshot()) == selected, NSApp.keyWindow === key, key?.firstResponder === responder else {
                    throw BridgeFailure(message: "Edit/scroll changed app selection, native focus or document ownership")
                }
            }
            @MainActor func action(_ args: [String], after: Bool = false) async throws -> Reply {
                let reply = try decode(Reply.self, await cli(["browser"] + args + target + (after ? ["--snapshot-after"] : []), false))
                guard reply.value.dispatched, reply.surfaceId == id, reply.hostId == host,
                      reply.documentGeneration == generation, reply.contentWorld == "isolated" else {
                    throw BridgeFailure(message: "Edit/scroll completion lost its real host/document identity")
                }
                try retained()
                return reply
            }
            @MainActor func observe() async throws -> Observation {
                let script = "({...zmuxEditScroll,typed:document.getElementById('entry').value,editorText:document.getElementById('editor').textContent,other:document.getElementById('other').value,selected:document.getElementById('choice').value,active:document.activeElement.id,left:document.getElementById('scroller').scrollLeft,top:document.getElementById('scroller').scrollTop,windowX:scrollX,windowY:scrollY,documentHasFocus:document.hasFocus(),readyState:document.readyState,visibility:document.visibilityState})"
                return try decode(Envelope<Observation>.self, await cli(["browser", "eval", script] + target, false)).value
            }
            let typed = try await action(["type", "--selector", ".many", "--text", "  雪  "], after: true)
            let appended = try await observe()
            guard appended.typed == "seed雪", appended.other == "other", appended.events == ["beforeinput", "input", "change"],
                  appended.inputTypes == ["insertText", "insertText"], appended.chunks == ["雪", "雪"],
                  appended.keyboard.isEmpty, typed.snapshot?.nodes.isEmpty == false, let ref = typed.value.ref else {
                throw BridgeFailure(message: "Type lost trimming, whole-value append, first-match or synthetic edit events: " + diagnostic(appended))
            }
            _ = try await action(["type", ref, "!"])
            _ = try await cli(["browser", "eval", "document.getElementById('entry').addEventListener('beforeinput',event=>event.preventDefault(),{once:true});null"] + target, false)
            let rejected = try await cli(["browser", "type", ref, "denied", "--snapshot-after"] + target, true)
            let cancelled = try await observe()
            guard String(decoding: rejected, as: UTF8.self).contains("input_rejected"), cancelled.typed == "seed雪!", cancelled.events.count == 7 else {
                throw BridgeFailure(message: "Canceled type forced or replayed the edit: " + diagnostic(cancelled))
            }
            _ = try await cli(["browser", "eval", "document.getElementById('entry').addEventListener('beforeinput',()=>{document.getElementById('entry').value='page-change'},{once:true});document.getElementById('editor').addEventListener('beforeinput',()=>{document.getElementById('editor').textContent='page-change'},{once:true});null"] + target, false)
            _ = try await action(["type", ref, "tail"])
            _ = try await action(["type", "#editor", "tail"])
            let editing = try await observe()
            guard editing.typed == "seed雪!tail", editing.editorText == "page-changetail" else {
                throw BridgeFailure(message: "Append lost value/textContent beforeinput ordering: " + diagnostic(editing))
            }
            let selectionReply = try await action(["select", "#choice", "--value", " b "], after: true)
            let choice = try await observe()
            guard choice.selected == "b", choice.selectionEvents == ["input", "change"], choice.active == editing.active,
                  selectionReply.snapshot?.nodes.isEmpty == false, let choiceRef = selectionReply.value.ref else {
                throw BridgeFailure(message: "Select lost native setter, trim or no-focus behavior: " + diagnostic(choice))
            }
            _ = try await action(["select", choiceRef, "b"])
            _ = try await action(["select", choiceRef, "missing"])
            let missingOption = try await observe()
            guard missingOption.selected.isEmpty, missingOption.selectionEvents == ["input", "change", "input", "change", "input", "change"] else {
                throw BridgeFailure(message: "Select incorrectly suppressed unchanged events or promised option matching: " + diagnostic(missingOption))
            }
            let scrolled = try await action(["scroll", "--selector", "#scroller", "--dx", "120", "--dy", "240"], after: true)
            let element = try await observe()
            guard element.left == 120, element.top == 240, element.active == missingOption.active,
                  scrolled.snapshot?.nodes.isEmpty == false, let scrollRef = scrolled.value.ref else {
                throw BridgeFailure(message: "Element pixel scroll changed focus or repeated its mutation: " + diagnostic(element))
            }
            _ = try await action(["scroll", "--selector", scrollRef, "--dx", "-20", "--dy", "-40"])
            let reversed = try await observe()
            guard reversed.left == 100, reversed.top == 200 else { throw BridgeFailure(message: "Negative element deltas lost exact ref/offset behavior: " + diagnostic(reversed)) }
            _ = try await cli(["browser", "eval", "window.scrollTo({left:0,top:0,behavior:'instant'});null"] + target, false)
            let windowScroll = try await action(["scroll", "--dx", "120", "240"], after: true)
            let windowState = try await observe()
            guard windowState.windowX == 120, windowState.windowY == 240, windowState.active == reversed.active,
                  windowScroll.value.ref == nil, windowScroll.snapshot?.nodes.isEmpty == false else {
                throw BridgeFailure(message: "Window scroll did not retain omitted-selector or hidden-focus behavior: " + diagnostic(windowState))
            }
            _ = try await cli(["browser", "eval", "document.getElementById('entry').remove();null"] + target, false)
            let stale = try await cli(["browser", "type", ref, "x"] + target, true)
            guard String(decoding: stale, as: UTF8.self).contains("stale_reference") else { throw BridgeFailure(message: "Type reused a disconnected ref") }
            try retained()
            _ = try await cli(["close-surface", "--surface", String(id)], false)
            hidden = nil
            FileHandle.standardError.write(Data("Native browser edit/scroll: append and cancellation, value selection, exact pixel scroll/ref/window and hidden focus passed\n".utf8))
        } catch {
            if let hidden {
                do { _ = try await cli(["close-surface", "--surface", String(hidden)], false) }
                catch { FileHandle.standardError.write(Data(("Edit/scroll fixture cleanup: " + error.localizedDescription + "\n").utf8)) }
            }
            throw error
        }
    }
}

import AppKit

/// Actual hidden WebKit event/state checks through the packaged public CLI.
@MainActor
enum BrowserInteractionSmoke {
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
        let doubleEvents: [String]
        let doubleDetail: Int
        let otherClicks: Int
        let boxClicks: Int
        let boxChecked: Bool
        let hoverEvents: [String]
        let focused: String
        let focusTop: Double
        let height: Double
        let checked: Bool
        let checkEvents: [String]
        let radioChecked: Bool
        let radioEvents: [String]
        let cancelled: Bool
        let cancelledClicks: Int
        let centered: Bool
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
        guard let id = created.createdSurfaces.first?.surfaceId else { throw BridgeFailure(message: "Interaction fixture omitted its hidden browser") }
        var hidden: UInt64? = id
        do {
            try await wait("Interaction fixture did not load") { browser(id)?.webView.title == "First fixture" && browser(id)?.webView.isLoading == false }
            guard let view = browser(id) else { throw BridgeFailure(message: "Interaction fixture lost its actual WebKit host") }
            let host = view.hostID, generation = view.documentGeneration
            let target = ["--surface", String(id)]
            let setup = #"""
            document.body.innerHTML = '<button id="double" class="many">Double</button><button id="other" class="many">Other</button><input id="double-box" type="checkbox"><button id="hover" disabled>Hover</button><input id="check" type="checkbox"><input id="radio" type="radio" checked><input id="cancel" type="checkbox"><div id="focus" tabindex="-1" style="position:absolute;top:5000px">Focus</div><div id="center" style="position:absolute;left:2500px;top:7500px;width:80px;height:60px"></div><div style="position:absolute;left:5000px;top:10000px;width:1px;height:1px"></div>';
            globalThis.zmuxInteractions = {doubleEvents:[],doubleDetail:0,otherClicks:0,boxClicks:0,hoverEvents:[],checkEvents:[],radioEvents:[],cancelledClicks:0};
            for (const kind of ['pointerover','mouseover','pointerenter','mouseenter','pointermove','mousemove','pointerdown','mousedown','pointerup','mouseup','click','dblclick']) document.getElementById('double').addEventListener(kind,event=>{zmuxInteractions.doubleEvents.push(event.type);if(event.type==='dblclick')zmuxInteractions.doubleDetail=event.detail});
            document.getElementById('other').addEventListener('click',()=>zmuxInteractions.otherClicks++);
            document.getElementById('double-box').addEventListener('click',()=>zmuxInteractions.boxClicks++);
            for (const kind of ['pointerover','mouseover','pointerenter','mouseenter','pointermove','mousemove','pointerdown','mousedown','pointerup','mouseup','click','focus']) document.getElementById('hover').addEventListener(kind,event=>zmuxInteractions.hoverEvents.push(event.type));
            for (const id of ['check','radio']) for (const kind of ['pointerdown','mousedown','click','input','change']) document.getElementById(id).addEventListener(kind,event=>zmuxInteractions[id+'Events'].push(event.type));
            document.getElementById('cancel').addEventListener('click',event=>{zmuxInteractions.cancelledClicks++;event.preventDefault()});
            null
            """#
            _ = try await cli(["browser", "eval", setup] + target, false)
            let key = NSApp.keyWindow, responder = key?.firstResponder
            @MainActor func retained() throws {
                guard view.hostID == host, view.documentGeneration == generation,
                      selection(snapshot()) == selected, NSApp.keyWindow === key, key?.firstResponder === responder else {
                    throw BridgeFailure(message: "DOM interaction changed native selection/focus or host/document ownership")
                }
            }
            @MainActor func interaction(_ operation: String, _ selector: String, after: Bool = false) async throws -> Reply {
                let reply = try decode(Reply.self, await cli(["browser", operation, "--selector", selector] + target + (after ? ["--snapshot-after"] : []), false))
                guard reply.value.dispatched, reply.surfaceId == id, reply.hostId == host,
                      reply.documentGeneration == generation, reply.contentWorld == "isolated" else {
                    throw BridgeFailure(message: "Interaction completion lost its actual host/document receipt")
                }
                try retained()
                return reply
            }
            @MainActor func observe() async throws -> Observation {
                let script = #"(() => {const box=document.getElementById('center').getBoundingClientRect();return {...zmuxInteractions,boxChecked:document.getElementById('double-box').checked,focused:document.activeElement.id,focusTop:document.getElementById('focus').getBoundingClientRect().top,height:innerHeight,checked:document.getElementById('check').checked,radioChecked:document.getElementById('radio').checked,cancelled:document.getElementById('cancel').checked,documentHasFocus:document.hasFocus(),readyState:document.readyState,visibility:document.visibilityState,centered:Math.abs(box.left+box.width/2-document.documentElement.clientWidth/2)<=1&&Math.abs(box.top+box.height/2-document.documentElement.clientHeight/2)<=1}})()"#
                return try decode(Envelope<Observation>.self, await cli(["browser", "eval", script] + target, false)).value
            }
            let doubled = try await interaction("dblclick", ".many", after: true)
            let doubleState = try await observe()
            let attempt = ["pointerover", "mouseover", "pointerenter", "mouseenter", "pointermove", "mousemove", "pointerdown", "mousedown", "pointerup", "mouseup", "click"]
            guard doubleState.doubleEvents == attempt + attempt + ["dblclick"], doubleState.doubleDetail == 2,
                  doubleState.otherClicks == 0, doubled.snapshot?.nodes.isEmpty == false else {
                throw BridgeFailure(message: "Double click did not produce exactly two first-match activations and a detail-2 event: " + diagnostic(doubleState))
            }
            _ = try await interaction("dblclick", "#double-box")
            let boxState = try await observe()
            guard boxState.boxClicks == 2, !boxState.boxChecked else { throw BridgeFailure(message: "Double click did not preserve two checkbox default activations: " + diagnostic(boxState)) }
            _ = try await interaction("hover", "#hover", after: true)
            let hoverState = try await observe()
            guard hoverState.hoverEvents == ["pointerover", "mouseover", "pointerenter", "mouseenter", "pointermove", "mousemove"],
                  hoverState.focused == boxState.focused else { throw BridgeFailure(message: "Disabled hover activated or focused its target: " + diagnostic(hoverState)) }
            _ = try await interaction("focus", "#focus")
            let focusState = try await observe()
            guard focusState.focused == "focus", focusState.focusTop >= 0, focusState.focusTop < focusState.height else { throw BridgeFailure(message: "DOM focus did not use ordinary focus scrolling: " + diagnostic(focusState)) }
            let checked = try await interaction("check", "#check", after: true)
            let checkState = try await observe()
            guard checkState.checked, checkState.checkEvents == ["click", "input", "change"], checked.snapshot?.nodes.isEmpty == false else { throw BridgeFailure(message: "Check did not use exactly one native default activation: " + diagnostic(checkState)) }
            _ = try await interaction("check", checked.value.ref)
            let sameState = try await observe()
            guard sameState.checkEvents == checkState.checkEvents else { throw BridgeFailure(message: "Already checked target emitted another activation: " + diagnostic(sameState)) }
            _ = try await interaction("uncheck", checked.value.ref)
            let uncheckState = try await observe()
            guard !uncheckState.checked, uncheckState.checkEvents == checkState.checkEvents + ["click", "input", "change"] else { throw BridgeFailure(message: "Uncheck did not activate its checkbox once: " + diagnostic(uncheckState)) }
            _ = try await interaction("uncheck", "#radio")
            let radioState = try await observe()
            guard !radioState.radioChecked, radioState.radioEvents == ["input", "change"] else { throw BridgeFailure(message: "Radio uncheck dispatched a click or lost native state replacement: " + diagnostic(radioState)) }
            let cancelled = try await cli(["browser", "check", "#cancel", "--snapshot-after"] + target, true)
            let cancelState = try await observe()
            guard String(decoding: cancelled, as: UTF8.self).contains("not_changed"), !cancelState.cancelled, cancelState.cancelledClicks == 1 else { throw BridgeFailure(message: "Canceled check was forced or replayed instead of failing state validation: " + diagnostic(cancelState)) }
            let centered = try await interaction("scroll-into-view", "#center", after: true)
            let centerState = try await observe()
            guard centerState.centered, centered.snapshot?.nodes.isEmpty == false, centerState.focused == cancelState.focused else { throw BridgeFailure(message: "Centered scroll changed focus or did not center the target: " + diagnostic(centerState)) }
            _ = try await interaction("scrollintoview", centered.value.ref)
            _ = try await interaction("scrollinto", centered.value.ref)
            for (operation, selector, code) in [("dblclick", "#hover", "element_disabled"), ("check", "#hover", "not_checkable"), ("focus", "[", "invalid_selector"), ("hover", "#missing", "element_not_found")] {
                let failure = try await cli(["browser", operation, selector] + target, true)
                guard String(decoding: failure, as: UTF8.self).contains(code) else { throw BridgeFailure(message: "Interaction failed for an unrelated reason") }
            }
            _ = try await cli(["browser", "eval", "document.getElementById('double').remove(); null"] + target, false)
            let stale = try await cli(["browser", "focus", doubled.value.ref] + target, true)
            guard String(decoding: stale, as: UTF8.self).contains("stale_reference") else { throw BridgeFailure(message: "Interaction reused a disconnected document ref") }
            try retained()
            _ = try await cli(["close-surface", "--surface", String(id)], false)
            hidden = nil
            FileHandle.standardError.write(Data("Native browser interactions: double activation, disabled hover, DOM focus, checked state/cancellation, centered aliases and hidden native focus passed\n".utf8))
        } catch {
            if let hidden {
                do { _ = try await cli(["close-surface", "--surface", String(hidden)], false) }
                catch { FileHandle.standardError.write(Data(("Interaction fixture cleanup: " + error.localizedDescription + "\n").utf8)) }
            }
            throw error
        }
    }
}

import AppKit

/// Actual bundled CLI/native WebKit keyboard delivery on a never-selected host.
/// This is distinct from the standalone feasibility app and cannot be faked by DOM tests.
@MainActor
enum BrowserKeyboardSmoke {
    private struct Created: Decodable {
        struct Target: Decodable { let surfaceId: UInt64 }
        let createdSurfaces: [Target]
    }
    private struct Event: Codable { let type, key: String; let trusted, shift: Bool }
    private struct Observation: Codable {
        let value: String
        let checked: Bool
        let submits: Int
        let start: Int
        let end: Int
        let events: [Event]
    }
    private struct Envelope<T: Decodable>: Decodable { let value: T }
    private struct Reply: Decodable {
        struct Value: Decodable { let dispatched: Bool; let input: String; let nativeDefaults: Bool?; let nativeEventObserved: Bool? }
        let value: Value
        let surfaceId: UInt64
        let hostId: UUID
        let documentGeneration: UInt64
        let contentWorld: String
        let snapshot: BrowserJSON?
    }
    private static func decode<T: Decodable>(_ type: T.Type, _ data: Data) throws -> T {
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(type, from: data)
    }
    private static func selection(_ snapshot: DesktopSnapshot?) -> [UInt64] {
        guard let model = snapshot?.model else { return [] }
        var value = [model.focusedWindow ?? 0, model.focusedWorkspace ?? 0]
        for window in model.windows { value += [window.id, window.selectedWorkspace ?? 0] }
        for workspace in model.workspaces {
            value += [workspace.id, workspace.focusedPane]
            for pane in workspace.root.panes { value += [pane.id, pane.selectedSurface] }
        }
        return value
    }
    static func validate(origin: String, cli: @escaping @MainActor ([String], Bool) async throws -> Data,
                         snapshot: @MainActor () -> DesktopSnapshot?, browser: @MainActor (UInt64) -> BrowserView?) async throws {
        let selected = selection(snapshot())
        let keyWindow = NSApp.keyWindow, responder = keyWindow?.firstResponder
        let created = try decode(Created.self, await cli(["browser", "open", "--no-focus", origin + "/"], false))
        guard let id = created.createdSurfaces.first?.surfaceId else { throw BridgeFailure(message: "Keyboard fixture omitted its hidden browser") }
        let target = ["--surface", String(id)]
        var freshID: UInt64?
        do {
            let deadline = ContinuousClock.now + .seconds(8)
            while browser(id)?.webView.title != "First fixture" || browser(id)?.webView.isLoading != false {
                guard ContinuousClock.now < deadline else { throw BridgeFailure(message: "Keyboard fixture did not load") }
                try await Task.sleep(for: .milliseconds(25))
            }
            guard let view = browser(id), view.webView.window == nil,
                  let events = (NSApp as? BrowserInputApplication)?.browserKeyboardEvents else {
                throw BridgeFailure(message: "Keyboard fixture did not retain detached hosting/containment")
            }
            let host = view.hostID
            @MainActor func retained() throws {
                guard view.hostID == host, view.webView.window == nil, selection(snapshot()) == selected,
                      NSApp.keyWindow === keyWindow, keyWindow?.firstResponder === responder else {
                    throw BridgeFailure(message: "Keyboard input changed hosting, app selection or unrelated native responder")
                }
            }
            @MainActor func setup(_ focus: String = "entry") async throws {
                let script = #"""
                globalThis.zmuxKeyObservers?.abort();globalThis.zmuxKeyObservers=new AbortController();
                document.body.innerHTML='<form id="form"><input id="entry" value="ab"><button type="submit">Submit</button></form><input id="box" type="checkbox">';
                globalThis.zmuxKeyEvents=[];globalThis.zmuxSubmits=0;
                for(const type of ['keydown','keyup'])document.addEventListener(type,event=>zmuxKeyEvents.push({type:event.type,key:event.key,trusted:event.isTrusted,shift:event.shiftKey}),{capture:true,signal:zmuxKeyObservers.signal});
                document.getElementById('form').addEventListener('submit',event=>{zmuxSubmits++;event.preventDefault()});
                document.getElementById('entry').addEventListener('input',()=>{document.title='keyboard-'+document.getElementById('entry').value});
                document.getElementById(FOCUS).focus();document.getElementById('entry').setSelectionRange(2,2);null
                """#
                let literal = String(decoding: try JSONEncoder().encode(focus), as: UTF8.self)
                _ = try await cli(["browser", "eval", script.replacingOccurrences(of: "FOCUS", with: literal)] + target, false)
                try retained()
            }
            @MainActor func observe() async throws -> Observation {
                try decode(Envelope<Observation>.self, await cli(["browser", "eval", "({value:document.getElementById('entry').value,checked:document.getElementById('box').checked,submits:zmuxSubmits,start:document.getElementById('entry').selectionStart,end:document.getElementById('entry').selectionEnd,events:zmuxKeyEvents})"] + target, false)).value
            }
            var actionCase = 0
            @MainActor func action(_ operation: String, _ token: String, synthetic: Bool = false, after: Bool = false, snapshotTitle: String? = nil) async throws {
                actionCase += 1
                let generation = view.documentGeneration
                let unownedGeneration = events.unownedKeyGeneration
                let reply: Reply
                do {
                    reply = try decode(Reply.self, await cli(["browser", operation, "--key", token] + target + (after ? ["--snapshot-after"] : []), false))
                } catch {
                    // These keys and DOM events belong only to the generated fixture;
                    // production mismatch messages redact printable user key content.
                    let observed: String
                    do { observed = String(decoding: try JSONEncoder().encode(try await observe()), as: UTF8.self) }
                    catch { observed = "Fixture observation unavailable: " + error.localizedDescription }
                    throw BridgeFailure(message: "Keyboard fixture case=\(actionCase), operation=\(operation), key=\(token), snapshotAfter=\(after), hostRetained=\(view.hostID == host), generationBefore=\(generation), generationAfter=\(view.documentGeneration), unownedBefore=\(unownedGeneration), unownedAfter=\(events.unownedKeyGeneration), appActive=\(NSApp.isActive), keyWindowRetained=\(NSApp.keyWindow === keyWindow), responderRetained=\(keyWindow?.firstResponder === responder), detached=\(view.webView.window == nil); callback=\(error.localizedDescription); fixtureObservation=\(String(observed.prefix(4096)))")
                }
                guard reply.value.dispatched, reply.value.input == (synthetic ? "synthetic" : "native"),
                      !synthetic || reply.value.nativeDefaults == false,
                      synthetic || reply.value.nativeEventObserved == true,
                      reply.contentWorld == (synthetic ? "isolated" : "native"), reply.hostId == host,
                      reply.surfaceId == id, reply.documentGeneration == generation,
                      !after || reply.snapshot != nil else {
                    throw BridgeFailure(message: "Keyboard receipt lost input kind, snapshot or exact host/document ownership")
                }
                if let snapshotTitle {
                    let actualTitle: String?
                    if case .object(let fields)? = reply.snapshot, case .string(let title)? = fields["title"] { actualTitle = title }
                    else { actualTitle = nil }
                    guard actualTitle == snapshotTitle else {
                        let observed: String
                        do {
                            let value = try await observe()
                            observed = String(decoding: try JSONEncoder().encode(value), as: UTF8.self)
                        }
                        catch { observed = "Diagnostic DOM observation failed: " + error.localizedDescription }
                        throw BridgeFailure(message: "Keyboard snapshot preceded the native input/default handler: snapshotTitle=\(actualTitle ?? "<missing>"), expected=\(snapshotTitle), subsequentObservation=\(String(observed.prefix(4096))), appActive=\(NSApp.isActive), keyWindowRetained=\(NSApp.keyWindow === keyWindow), detached=\(view.webView.window == nil)")
                    }
                }
                try retained()
            }
            @MainActor func check(_ predicate: (Observation) -> Bool, _ message: String) async throws -> Observation {
                let observed = try await observe()
                guard predicate(observed) else {
                    let data = try JSONEncoder().encode(observed)
                    throw BridgeFailure(message: message + ": " + String(decoding: data, as: UTF8.self))
                }
                return observed
            }
            try await setup()
            try await action("press", "x", after: true, snapshotTitle: "keyboard-abx")
            _ = try await check({ $0.value == "abx" && $0.events.map(\.type) == ["keydown", "keyup"] && $0.events.allSatisfy(\.trusted) }, "Native text defaults/trusted pair failed")
            try await setup()
            _ = try await cli(["browser", "eval", "document.getElementById('entry').addEventListener('keydown',event=>event.preventDefault(),{once:true});null"] + target, false)
            try await action("press", "x")
            _ = try await check({ $0.value == "ab" && $0.events.allSatisfy(\.trusted) }, "Canceled native default was forced or downgraded")
            try await setup()
            _ = try await cli(["browser", "eval", "window.addEventListener('keydown',event=>event.stopImmediatePropagation(),{capture:true,once:true});null"] + target, false)
            let suppressed = String(decoding: try await cli(["--timeout", "2000", "browser", "press", "x"] + target, true), as: UTF8.self)
            guard suppressed.contains("native_input_unacknowledged"), suppressed.contains("outcome may be unknown") else {
                throw BridgeFailure(message: "Suppressed native acknowledgment fabricated success or lost mutation uncertainty: " + suppressed)
            }
            let poisoned = String(decoding: try await cli(["browser", "press", "x"] + target, true), as: UTF8.self)
            guard poisoned.contains("native_keyboard_state_unknown") else {
                throw BridgeFailure(message: "Unacknowledged native input admitted another key in the same document: " + poisoned)
            }
            _ = try await cli(["browser", "reload"] + target, false)
            try await setup()
            try await action("press", "x")
            try await action("press", "Backspace")
            try await action("press", "ArrowLeft")
            _ = try await check({ $0.value == "ab" && $0.start == 1 && $0.end == 1 }, "Native backspace/caret default failed")
            try await setup()
            try await action("press", "!")
            try await action("press", "+")
            _ = try await check({ $0.value == "ab!+" && $0.events.map(\.key) == ["!", "!", "+", "+"] && $0.events.allSatisfy { $0.trusted && !$0.shift } }, "Literal symbols lost key/default or explicitly held-modifier semantics")
            try await setup("box")
            try await action("key", " ")
            _ = try await check({ $0.checked && $0.events.allSatisfy(\.trusted) }, "Literal Space failed native checkbox default")
            try await setup()
            try await action("press", "Enter")
            _ = try await check({ $0.submits == 1 }, "Enter failed native form default")
            try await setup()
            try await action("keydown", "x")
            _ = try await check({ $0.value == "abx" && $0.events.map(\.type) == ["keydown"] }, "Keydown was not independently observed")
            try await action("keyup", "x")
            _ = try await check({ $0.value == "abx" && $0.events.map(\.type) == ["keydown", "keyup"] }, "Keyup repeated insertion or lost its pair")
            try await setup()
            try await action("keydown", "Shift")
            try await action("press", "x")
            try await action("keyup", "Shift")
            try await action("press", "x")
            _ = try await check({ value in
                let sequence = value.events.map { $0.type + ":" + $0.key }
                return value.value == "abXx" && value.events.allSatisfy(\.trusted)
                    && sequence == ["keydown:Shift", "keydown:X", "keyup:X", "keyup:Shift", "keydown:x", "keyup:x"]
                    && value.events[1].shift && !value.events[4].shift
            }, "Cross-request held Shift order/release failed")
            try await setup()
            // Challenge the actual secondary entrypoints with an owned event:
            // both must consume it before unrelated native shortcut dispatch.
            guard let nativeWindow = keyWindow as? SidebarFocusWindow, let commandW = BrowserNativeKey.resolve("w") else {
                throw BridgeFailure(message: "Keyboard fixture lacks the actual guarded main window")
            }
            let guardedDown = try events.event(commandW, down: true, flags: .maskCommand)
            let guardedUp = try events.event(commandW, down: false, flags: .maskCommand)
            guard nativeWindow.performKeyEquivalent(with: guardedDown) else { throw BridgeFailure(message: "Window key-equivalent entry did not contain an owned browser event") }
            nativeWindow.sendEvent(guardedDown); nativeWindow.sendEvent(guardedUp)
            try retained()
            let beforeResend = events.asynchronousCommandWResends
            try await action("keydown", "Meta")
            try await action("press", "w")
            try await action("keyup", "Meta")
            let resendDeadline = ContinuousClock.now + .seconds(2)
            while events.asynchronousCommandWResends == beforeResend, ContinuousClock.now < resendDeadline {
                try await Task.sleep(for: .milliseconds(25))
            }
            guard events.asynchronousCommandWResends > beforeResend else { throw BridgeFailure(message: "Production containment did not observe an actual asynchronous unhandled Command-W keydown resend") }
            _ = try await check({ $0.value == "ab" }, "Unhandled Command-W mutated the target")
            try retained()
            try await setup()
            try await action("press", "OpaqueKeyboardToken", synthetic: true)
            _ = try await check({ $0.value == "ab" && $0.events.count == 2 && $0.events.allSatisfy { !$0.trusted && $0.key == "OpaqueKeyboardToken" } }, "Opaque fallback claimed native input/defaults")
            try await setup()
            let unavailable = try await cli(["browser", "press", "CapsLock"] + target, true)
            _ = try await check({ $0.value == "ab" && $0.events.isEmpty }, "Unsupported named native key dispatched fallback")
            guard String(decoding: unavailable, as: UTF8.self).contains("native_key_unavailable") else { throw BridgeFailure(message: "Unsupported named native key did not fail explicitly") }
            try await action("keydown", "Shift")
            _ = try await cli(["browser", "reload"] + target, false)
            try await setup()
            try await action("press", "x")
            _ = try await check({ $0.value == "abx" && $0.events.allSatisfy { !$0.shift } }, "Document replacement retained old held modifiers")
            // Retiring a host with held state must never transfer it to a new host.
            try await action("keydown", "Shift")
            _ = try await cli(["close-surface", "--surface", String(id)], false)
            let fresh = try decode(Created.self, await cli(["browser", "open", "--no-focus", origin + "/"], false))
            guard let next = fresh.createdSurfaces.first?.surfaceId else { throw BridgeFailure(message: "Keyboard replacement fixture omitted its surface") }
            freshID = next
            let nextTarget = ["--surface", String(next)]
            let freshDeadline = ContinuousClock.now + .seconds(8)
            while browser(next)?.webView.isLoading != false || browser(next)?.webView.title != "First fixture" {
                guard ContinuousClock.now < freshDeadline else { throw BridgeFailure(message: "Keyboard replacement host did not load") }
                try await Task.sleep(for: .milliseconds(25))
            }
            _ = try await cli(["browser", "eval", "document.body.innerHTML='<input id=\"fresh\" value=\"\">';document.getElementById('fresh').focus();null"] + nextTarget, false)
            _ = try decode(Reply.self, await cli(["browser", "press", "x"] + nextTarget, false))
            let freshValue = try decode(Envelope<String>.self, await cli(["browser", "eval", "document.getElementById('fresh').value"] + nextTarget, false)).value
            guard freshValue == "x", let replacement = browser(next), replacement.hostID != host,
                  replacement.webView.window == nil, selection(snapshot()) == selected,
                  NSApp.keyWindow === keyWindow, keyWindow?.firstResponder === responder else {
                throw BridgeFailure(message: "Host retirement transferred held state or changed unrelated focus")
            }
            _ = try await cli(["close-surface", "--surface", String(next)], false)
            freshID = nil
        } catch {
            if let freshID {
                do { _ = try await cli(["close-surface", "--surface", String(freshID)], false) }
                catch { FileHandle.standardError.write(Data(("Keyboard replacement cleanup: " + error.localizedDescription + "\n").utf8)) }
            }
            do { _ = try await cli(["close-surface", "--surface", String(id)], false) }
            catch { FileHandle.standardError.write(Data(("Keyboard fixture cleanup: " + error.localizedDescription + "\n").utf8)) }
            throw error
        }
    }
}

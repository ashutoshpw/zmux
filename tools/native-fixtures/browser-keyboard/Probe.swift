import AppKit
import CoreGraphics
import Darwin
import Foundation
import WebKit

private struct ProbeFailure: Error, CustomStringConvertible {
    let description: String
}
private struct EventRecord: Codable, Sendable {
    let type: String
    let key: String?
    let code: String?
    let trusted: Bool
    let shift: Bool
    let command: Bool
}
private struct PageState: Codable, Sendable {
    let value: String
    let checked: Bool
    let submits: Int
    let clicks: Int
    let start: Int?
    let end: Int?
    let active: String
    let hasFocus: Bool
    let ready: String
    let events: [EventRecord]
}
private struct NativeState: Codable, Sendable {
    let applicationActive: Bool
    let frontmostProcessID: Int32?
    let keyWindow: Int?
    let targetWindow: Int?
    let sentinelWindow: Int
    let keyResponder: String?
    let sentinelResponder: String?
    let sentinelText: String
    let targetResponder: String?
}
private struct ResendRecord: Codable, Sendable {
    let route: String
    let phase: String
    let originPhase: String
    let eventType: UInt
    let keyCode: UInt16
    let command: Bool
    let windowNumber: Int
    let duringDirectDelivery: Bool
    let uptime: Double
}
private struct CaseRecord: Codable, Sendable {
    let mode: String
    let name: String
    var before: PageState?
    var afterDown: PageState?
    var after: PageState?
    var nativeBefore: NativeState?
    var nativeAfter: NativeState?
    var focusRetained = false
    var passed = false
    var error: String?
}
private struct Report: Encodable, Sendable {
    let schemaVersion = 1
    let sourceSHA: String
    let osVersion: String
    let architecture: String
    let processID: Int32
    var phase = "starting"
    var state = "running"
    var applicationActive = false
    var frontmostProcessID: Int32?
    var activationRequestProcessID: Int32?
    var activationError: String?
    var cases: [CaseRecord] = []
    var resends: [ResendRecord] = []
    var escapedShortcuts = 0
    var resendLogLimitReached = false
    var visibleDefaultsVerified = false
    var detachedDefaultsVerified = false
    var asyncResendObserved = false
    var asyncResendContained = false
    let productionAccepted = false
    var error: String?
}

/// The probe owns this application. Tagged events never reach its menus/windows
/// through application dispatch. Direct delivery uses only the target WebKit view.
@objc(ZmuxKeyboardProbeApplication)
@MainActor
private final class ProbeApplication: NSApplication {
    private var ownedTags: [Int64: String] = [:]
    private var nextTag = Int64.random(in: 1...(Int64.max / 2))
    var deliveryDepth = 0
    var phase = "startup"
    var resends: [ResendRecord] = []
    var resendLogLimitReached = false
    var escapedShortcuts = 0

    func tag(_ event: CGEvent) throws {
        guard ownedTags.count < 256 else { throw ProbeFailure(description: "Owned native-event tag bound exceeded") }
        nextTag += 1
        ownedTags[nextTag] = phase
        event.setIntegerValueField(.eventSourceUserData, value: nextTag)
    }
    func owns(_ event: NSEvent) -> Bool {
        guard let cgEvent = event.cgEvent else { return false }
        return ownedTags[cgEvent.getIntegerValueField(.eventSourceUserData)] != nil
    }
    func contain(_ event: NSEvent, route: String) -> Bool {
        guard let cgEvent = event.cgEvent,
              let origin = ownedTags[cgEvent.getIntegerValueField(.eventSourceUserData)] else { return false }
        if resends.count < 128 {
            resends.append(ResendRecord(route: route, phase: phase, originPhase: origin,
                eventType: event.type.rawValue, keyCode: event.keyCode,
                command: event.modifierFlags.contains(.command),
                windowNumber: event.windowNumber, duringDirectDelivery: deliveryDepth > 0,
                uptime: ProcessInfo.processInfo.systemUptime))
        } else { resendLogLimitReached = true }
        return true
    }
    override func sendEvent(_ event: NSEvent) {
        if contain(event, route: "application.sendEvent") { return }
        super.sendEvent(event)
    }
    @objc func forbiddenShortcut(_ sender: Any?) { escapedShortcuts += 1 }
}

@MainActor
private final class ProbeWindow: NSWindow {
    weak var interception: ProbeApplication?
    override func sendEvent(_ event: NSEvent) {
        if interception?.contain(event, route: "window.sendEvent") == true { return }
        super.sendEvent(event)
    }
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if interception?.contain(event, route: "window.performKeyEquivalent") == true { return true }
        return super.performKeyEquivalent(with: event)
    }
}

@MainActor
private final class ProbeWebView: WKWebView {
    weak var interception: ProbeApplication?
    private var directDepth = 0
    func deliver(_ event: NSEvent) throws {
        guard let interception, interception.owns(event) else { throw ProbeFailure(description: "Native event lacks the probe's ownership tag") }
        directDepth += 1; interception.deliveryDepth += 1
        defer { directDepth -= 1; interception.deliveryDepth -= 1 }
        switch event.type {
        case .keyDown: super.keyDown(with: event)
        case .keyUp: super.keyUp(with: event)
        case .flagsChanged: super.flagsChanged(with: event)
        default: throw ProbeFailure(description: "Unexpected direct native event type")
        }
    }
    override func keyDown(with event: NSEvent) {
        if directDepth == 0, interception?.contain(event, route: "webview.keyDown") == true { return }
        super.keyDown(with: event)
    }
    override func keyUp(with event: NSEvent) {
        if directDepth == 0, interception?.contain(event, route: "webview.keyUp") == true { return }
        super.keyUp(with: event)
    }
    override func flagsChanged(with event: NSEvent) {
        if directDepth == 0, interception?.contain(event, route: "webview.flagsChanged") == true { return }
        super.flagsChanged(with: event)
    }
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if directDepth == 0, interception?.contain(event, route: "webview.performKeyEquivalent") == true { return true }
        return super.performKeyEquivalent(with: event)
    }
}

@MainActor
private final class ProbeRunner {
    private let app: ProbeApplication
    private let output: URL
    private var report: Report
    private let sentinel: ProbeWindow
    private let sentinelField = NSTextField(string: "UNRELATED")
    private var browserWindow: ProbeWindow?
    private var retainedHosts: [NSView] = []
    private let source = CGEventSource(stateID: .privateState)

    init(app: ProbeApplication, output: URL) {
        self.app = app; self.output = output
        let environment = ProcessInfo.processInfo.environment
        report = Report(sourceSHA: environment["ZMUX_KEYBOARD_SOURCE_SHA"] ?? "unknown",
            osVersion: ProcessInfo.processInfo.operatingSystemVersionString,
            architecture: environment["ZMUX_KEYBOARD_ARCH"] ?? "unknown", processID: getpid())
        sentinel = ProbeWindow(contentRect: NSRect(x: 80, y: 80, width: 420, height: 140),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        sentinel.isReleasedWhenClosed = false
        sentinel.title = "Zmux keyboard probe: unrelated responder"
        sentinel.interception = app
        sentinelField.frame = NSRect(x: 20, y: 50, width: 380, height: 24)
        sentinel.contentView?.addSubview(sentinelField)
    }
    private func save() throws {
        report.applicationActive = app.isActive
        report.frontmostProcessID = NSWorkspace.shared.frontmostApplication?.processIdentifier
        report.resends = app.resends
        report.escapedShortcuts = app.escapedShortcuts
        report.resendLogLimitReached = app.resendLogLimitReached
        let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = try encoder.encode(report)
        guard data.count <= 1_048_576 else { throw ProbeFailure(description: "Probe report exceeded 1 MiB") }
        try data.write(to: output, options: .atomic)
    }
    private func phase(_ value: String) throws {
        report.phase = value; app.phase = value
        try save()
        FileHandle.standardError.write(Data((value + "\n").utf8))
    }
    private func evaluate(_ expression: String, on view: WKWebView) async throws -> String {
        try await withCheckedThrowingContinuation { continuation in
            view.evaluateJavaScript(expression) { value, error in
                if let error { continuation.resume(throwing: error) }
                else if let text = value as? String { continuation.resume(returning: text) }
                else { continuation.resume(throwing: ProbeFailure(description: "WebKit observation was not a JSON string")) }
            }
        }
    }
    private func pause(_ milliseconds: Int = 120) async throws {
        try await Task.sleep(for: .milliseconds(milliseconds))
    }
    private func activateOwnedApplication() async throws {
        let current = NSRunningApplication.current
        let bundle = Bundle.main.bundleURL.standardizedFileURL
        guard current.processIdentifier == getpid(), current.bundleURL?.standardizedFileURL == bundle,
              let identifier = current.bundleIdentifier,
              NSWorkspace.shared.runningApplications.filter({ $0.bundleIdentifier == identifier }).map(\.processIdentifier) == [getpid()] else {
            throw ProbeFailure(description: "Launch Services did not identify this exact unique running probe")
        }
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true
        configuration.createsNewApplicationInstance = false
        configuration.allowsRunningApplicationSubstitution = false
        // No --output arguments are supplied: an unexpected new instance cannot
        // run the fixture or write over the original process's observations.
        configuration.arguments = []
        let processID: Int32 = try await withCheckedThrowingContinuation { continuation in
            NSWorkspace.shared.openApplication(at: bundle, configuration: configuration) { running, error in
                if let error { continuation.resume(throwing: error) }
                else if let running { continuation.resume(returning: running.processIdentifier) }
                else { continuation.resume(throwing: ProbeFailure(description: "Launch Services returned no running application")) }
            }
        }
        report.activationRequestProcessID = processID
        guard processID == getpid() else {
            throw ProbeFailure(description: "Launch Services activation did not return this exact running probe PID")
        }
        let deadline = ContinuousClock.now + .seconds(3)
        while !app.isActive {
            guard ContinuousClock.now < deadline else { throw ProbeFailure(description: "Exact-self activation was not granted within three seconds") }
            try await pause(25)
        }
    }
    private func observe(_ view: WKWebView) async throws -> PageState {
        let text = try await evaluate("JSON.stringify({value:document.getElementById('entry').value,checked:document.getElementById('box').checked,submits:globalThis.submits,clicks:globalThis.clicks,start:document.getElementById('entry').selectionStart,end:document.getElementById('entry').selectionEnd,active:document.activeElement.id,hasFocus:document.hasFocus(),ready:document.readyState,events:globalThis.events})", on: view)
        return try JSONDecoder().decode(PageState.self, from: Data(text.utf8))
    }
    private var sentinelText: String {
        (sentinelField.currentEditor() as? NSTextView)?.string ?? sentinelField.stringValue
    }
    private func nativeState(_ view: WKWebView) -> NativeState {
        NativeState(applicationActive: app.isActive,
            frontmostProcessID: NSWorkspace.shared.frontmostApplication?.processIdentifier,
            keyWindow: app.keyWindow?.windowNumber, targetWindow: view.window?.windowNumber,
            sentinelWindow: sentinel.windowNumber,
            keyResponder: app.keyWindow?.firstResponder.map { String(describing: ObjectIdentifier($0)) },
            sentinelResponder: sentinel.firstResponder.map { String(describing: ObjectIdentifier($0)) },
            sentinelText: sentinelText,
            targetResponder: view.window?.firstResponder.map { String(describing: ObjectIdentifier($0)) })
    }
    private func makeEvent(code: CGKeyCode, down: Bool, characters: String,
                           flags: CGEventFlags = [], modifier: Bool = false) throws -> NSEvent {
        guard let source, let cgEvent = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: down) else {
            throw ProbeFailure(description: "Could not create CGEvent-backed key")
        }
        cgEvent.flags = flags
        if modifier { cgEvent.type = .flagsChanged }
        if !characters.isEmpty {
            let units = Array(characters.utf16)
            try units.withUnsafeBufferPointer { buffer in
                guard let address = buffer.baseAddress else { throw ProbeFailure(description: "Native key has no UTF-16 storage") }
                cgEvent.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: address)
            }
        }
        try app.tag(cgEvent)
        guard let event = NSEvent(cgEvent: cgEvent), event.cgEvent != nil, app.owns(event) else {
            throw ProbeFailure(description: "Native event conversion lost its CGEvent/ownership tag")
        }
        return event
    }
    private func press(_ view: ProbeWebView, code: CGKeyCode, characters: String, flags: CGEventFlags = []) throws {
        // Create both halves before emitting either; event creation failure must
        // not strand a native key-down without its corresponding key-up.
        let down = try makeEvent(code: code, down: true, characters: characters, flags: flags)
        let up = try makeEvent(code: code, down: false, characters: characters, flags: flags)
        try view.deliver(down); try view.deliver(up)
    }
    private func reset(_ view: WKWebView, focus: String) async throws {
        let script = #"""
        document.body.innerHTML='<form id="form"><input id="entry" value="ab"><button type="submit">Submit</button></form><input id="box" type="checkbox"><div id="editor" contenteditable="true">editable</div>';
        globalThis.events=[];globalThis.submits=0;globalThis.clicks=0;
        for(const type of ['keydown','keypress','keyup','beforeinput','input','change','click','submit']) document.addEventListener(type,event=>{
          if(globalThis.events.length<128)globalThis.events.push({type:event.type,key:event.key??null,code:event.code??null,trusted:event.isTrusted,shift:!!event.shiftKey,command:!!event.metaKey});
        },{capture:true,signal:globalThis.listenerController.signal});
        document.getElementById('form').addEventListener('submit',event=>{globalThis.submits++;event.preventDefault()});
        document.getElementById('box').addEventListener('click',()=>globalThis.clicks++);
        document.getElementById(FOCUS).focus();document.getElementById('entry').setSelectionRange(2,2);JSON.stringify({reset:true})
        """#
        // Reset per-case observers without replacing the WebKit host/document.
        _ = try await evaluate("globalThis.listenerController?.abort();globalThis.listenerController=new AbortController();JSON.stringify({ready:true})", on: view)
        let literal = String(decoding: try JSONEncoder().encode(focus), as: UTF8.self)
        _ = try await evaluate(script.replacingOccurrences(of: "FOCUS", with: literal), on: view)
        try await pause()
        if let browserWindow, view.window === browserWindow {
            let deadline = ContinuousClock.now + .seconds(2)
            while try await evaluate("JSON.stringify(document.hasFocus())", on: view) != "true" {
                guard ContinuousClock.now < deadline else { return }
                try await pause(25)
            }
        }
    }
    private func loadedHost(mode: String) async throws -> ProbeWebView {
        let configuration = WKWebViewConfiguration(); configuration.websiteDataStore = .nonPersistent()
        let view = ProbeWebView(frame: NSRect(x: 0, y: 0, width: 640, height: 480), configuration: configuration)
        view.interception = app
        let host = NSView(frame: view.frame); host.addSubview(view)
        view.autoresizingMask = [.width, .height]
        retainedHosts.append(host)
        if mode == "visible" {
            let window = ProbeWindow(contentRect: NSRect(x: 120, y: 240, width: 640, height: 480),
                styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            window.interception = app; window.title = "Zmux keyboard probe: selected browser"
            window.contentView = host; browserWindow = window
            window.makeKeyAndOrderFront(nil)
        } else {
            browserWindow?.orderOut(nil)
            sentinel.makeKeyAndOrderFront(nil)
            guard sentinel.makeFirstResponder(sentinelField) else { throw ProbeFailure(description: "Unrelated sentinel could not accept focus") }
            sentinelField.selectText(nil)
            guard host.superview == nil, view.window == nil else { throw ProbeFailure(description: "Detached host unexpectedly acquired a native window") }
        }
        view.loadHTMLString("<!doctype html><title>Keyboard fixture</title><body></body>", baseURL: URL(string: "https://keyboard.invalid/"))
        let deadline = ContinuousClock.now + .seconds(8)
        while view.title != "Keyboard fixture" || view.isLoading {
            guard ContinuousClock.now < deadline else { throw ProbeFailure(description: "Real WebKit document did not finish loading") }
            try await pause(25)
        }
        if mode == "visible" {
            guard let browserWindow, browserWindow.makeFirstResponder(view) else {
                throw ProbeFailure(description: "Loaded visible host could not accept its content responder")
            }
        }
        return view
    }
    private func runCase(mode: String, name: String, view: ProbeWebView) async throws {
        try phase(mode + "/" + name + "/before")
        var row = CaseRecord(mode: mode, name: name)
        do {
            try await reset(view, focus: name == "space" ? "box" : "entry")
            let keyWindow = app.keyWindow, responder = keyWindow?.firstResponder, sentinelResponder = sentinel.firstResponder
            row.before = try await observe(view); row.nativeBefore = nativeState(view)
            guard report.activationError == nil, report.activationRequestProcessID == getpid() else {
                throw ProbeFailure(description: "Exact-self activation was not established: " + (report.activationError ?? "PID mismatch"))
            }
            if mode == "visible" {
                guard app.keyWindow === browserWindow, let native = browserWindow?.firstResponder as? NSView,
                      native === view || native.isDescendant(of: view), row.before?.hasFocus == true else {
                    throw ProbeFailure(description: "Visible-selected precondition lacks actual key/window/document focus")
                }
            } else {
                guard view.window == nil, app.keyWindow === sentinel,
                      sentinel.firstResponder === sentinelField.currentEditor(), sentinelText == "UNRELATED" else {
                    throw ProbeFailure(description: "Detached precondition changed hosting or unrelated text responder")
                }
            }
            try phase(mode + "/" + name + "/dispatch")
            switch name {
            case "text": try press(view, code: 7, characters: "x")
            case "backspace": try press(view, code: 51, characters: "\u{7f}")
            case "left": try press(view, code: 123, characters: String(UnicodeScalar(NSLeftArrowFunctionKey)!))
            case "space": try press(view, code: 49, characters: " ")
            case "enter": try press(view, code: 36, characters: "\r")
            case "down_up":
                let down = try makeEvent(code: 7, down: true, characters: "x")
                let up = try makeEvent(code: 7, down: false, characters: "x")
                try view.deliver(down); try await pause()
                row.afterDown = try await observe(view)
                try view.deliver(up)
            case "modifier_release":
                let shiftDown = try makeEvent(code: 56, down: true, characters: "", flags: .maskShift, modifier: true)
                let shiftUp = try makeEvent(code: 56, down: false, characters: "", modifier: true)
                let upperDown = try makeEvent(code: 7, down: true, characters: "X", flags: .maskShift)
                let upperUp = try makeEvent(code: 7, down: false, characters: "X", flags: .maskShift)
                let lowerDown = try makeEvent(code: 7, down: true, characters: "x")
                let lowerUp = try makeEvent(code: 7, down: false, characters: "x")
                for event in [shiftDown, upperDown, upperUp, shiftUp, lowerDown, lowerUp] { try view.deliver(event) }
            case "unhandled_command": try press(view, code: 13, characters: "w", flags: .maskCommand)
            default: throw ProbeFailure(description: "Unknown probe case")
            }
            try await pause(250)
            row.after = try await observe(view); row.nativeAfter = nativeState(view)
            row.focusRetained = app.keyWindow === keyWindow && keyWindow?.firstResponder === responder
                && sentinel.firstResponder === sentinelResponder && sentinelText == "UNRELATED"
            guard let after = row.after else { throw ProbeFailure(description: "Missing native observation") }
            let down = after.events.filter { $0.type == "keydown" }
            let up = after.events.filter { $0.type == "keyup" }
            let trustedPair = !down.isEmpty && !up.isEmpty && down.allSatisfy(\.trusted) && up.allSatisfy(\.trusted)
            let behavior: Bool
            switch name {
            case "text": behavior = after.value == "abx" && trustedPair && after.events.contains { $0.type == "input" && $0.trusted }
            case "backspace": behavior = after.value == "a" && trustedPair
            case "left": behavior = after.value == "ab" && after.start == 1 && after.end == 1 && trustedPair
            case "space": behavior = after.checked && after.clicks == 1 && trustedPair && after.events.contains { $0.type == "click" && $0.trusted }
            case "enter": behavior = after.submits == 1 && trustedPair && after.events.contains { $0.type == "submit" && $0.trusted }
            case "down_up": behavior = row.afterDown?.value == "abx" && row.afterDown?.events.contains(where: { $0.type == "keyup" }) == false && after.value == "abx" && up.count == 1 && trustedPair
            case "modifier_release": behavior = after.value == "abXx" && trustedPair && down.contains { $0.key == "X" && $0.shift } && down.last?.key == "x" && down.last?.shift == false && up.contains { $0.key == "Shift" && !$0.shift }
            case "unhandled_command": behavior = app.escapedShortcuts == 0 && after.value == "ab"
            default: behavior = false
            }
            row.passed = behavior && row.focusRetained
            if !row.passed { row.error = "Observed native events/defaults or unrelated responder retention did not meet the case contract" }
        } catch { row.error = String(describing: error) }
        report.cases.append(row)
        try phase(mode + "/" + name + "/recorded")
    }
    func run() async {
        do {
            try phase("initializing-owned-windows")
            let menu = NSMenu(title: "Keyboard probe")
            let root = NSMenuItem(); menu.addItem(root)
            let appMenu = NSMenu(title: "Keyboard probe")
            for (name, key) in [("Record unexpected Close", "w"), ("Record unexpected Quit", "q")] {
                let item = appMenu.addItem(withTitle: name, action: #selector(ProbeApplication.forbiddenShortcut(_:)), keyEquivalent: key)
                item.target = app
            }
            root.submenu = appMenu; app.mainMenu = menu
            sentinel.makeKeyAndOrderFront(nil)
            try phase("requesting-exact-self-activation")
            do { try await activateOwnedApplication() }
            catch { report.activationError = String(describing: error) }
            // Refused activation remains observable through the original case
            // preconditions. It cannot be promoted into a successful case.
            try phase("exact-self-activation-observed")
            let names = ["text", "backspace", "left", "space", "enter", "down_up", "modifier_release", "unhandled_command"]
            for mode in ["visible", "detached"] {
                try phase(mode + "/loading")
                let view = try await loadedHost(mode: mode)
                for name in names { try await runCase(mode: mode, name: name, view: view) }
            }
            let defaults = Set(names.filter { $0 != "unhandled_command" })
            report.visibleDefaultsVerified = report.cases.filter { $0.mode == "visible" && defaults.contains($0.name) }.allSatisfy(\.passed)
            report.detachedDefaultsVerified = report.cases.filter { $0.mode == "detached" && defaults.contains($0.name) }.allSatisfy(\.passed)
            report.asyncResendObserved = app.resends.contains {
                !$0.duringDirectDelivery && $0.originPhase.hasSuffix("/unhandled_command/dispatch")
                    && $0.eventType == NSEvent.EventType.keyDown.rawValue && $0.keyCode == 13 && $0.command
            }
            report.asyncResendContained = report.asyncResendObserved && app.escapedShortcuts == 0 && !app.resendLogLimitReached
            report.state = "completed_feasibility_observation"
            try phase("complete")
        } catch {
            report.state = "probe_failed"; report.error = String(describing: error)
            do { try save() }
            catch { FileHandle.standardError.write(Data(("Probe receipt write failed: " + error.localizedDescription + "\n").utf8)) }
        }
        browserWindow?.close(); sentinel.close()
        app.terminate(nil)
    }
}

@MainActor
private final class ProbeDelegate: NSObject, NSApplicationDelegate {
    let runner: ProbeRunner
    init(runner: ProbeRunner) { self.runner = runner }
    func applicationDidFinishLaunching(_ notification: Notification) {
        Task { await runner.run() }
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
}

@main
private enum Main {
    @MainActor static func main() {
        guard CommandLine.arguments.count == 3, CommandLine.arguments[1] == "--output" else {
            FileHandle.standardError.write(Data("Expected --output RECEIPT_PATH\n".utf8)); exit(2)
        }
        guard let app = ProbeApplication.shared as? ProbeApplication else {
            FileHandle.standardError.write(Data("NSApplication principal class was not the isolated probe subclass\n".utf8)); exit(2)
        }
        app.setActivationPolicy(.regular)
        let runner = ProbeRunner(app: app, output: URL(fileURLWithPath: CommandLine.arguments[2]))
        let delegate = ProbeDelegate(runner: runner)
        app.delegate = delegate
        withExtendedLifetime(delegate) { app.run() }
    }
}

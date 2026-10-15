import AppKit
import CoreGraphics
import WebKit

/// Process-owned containment for input sent directly to one browser. Tags remain
/// recognizable for the process lifetime without an ever-growing event registry.
@MainActor
final class BrowserKeyboardEvents {
    private let prefix = Int64.random(in: 1...0x7fff_ffff) << 32
    private var sequence: UInt32 = 0
    private let source = CGEventSource(stateID: .privateState)
    private(set) var asynchronousCommandWResends: UInt64 = 0
    var directDeliveryDepth = 0
    func owns(_ event: NSEvent) -> Bool {
        guard let cg = event.cgEvent else { return false }
        let tag = cg.getIntegerValueField(.eventSourceUserData)
        let count = UInt32(truncatingIfNeeded: tag)
        return tag & ~Int64(UInt32.max) == prefix && count > 0 && count <= sequence
    }
    func contain(_ event: NSEvent, applicationDispatch: Bool = false) -> Bool {
        guard owns(event) else { return false }
        if applicationDispatch, directDeliveryDepth == 0, event.type == .keyDown, event.keyCode == 13, event.modifierFlags.contains(.command) {
            asynchronousCommandWResends = min(UInt64.max - 1, asynchronousCommandWResends) + 1
        }
        return true
    }
    func event(_ key: BrowserNativeKey, down: Bool, flags: CGEventFlags, repeated: Bool = false) throws -> NSEvent {
        guard sequence < UInt32.max, let source,
              let cg = CGEvent(keyboardEventSource: source, virtualKey: key.code, keyDown: down) else {
            throw BridgeFailure(message: "Could not prepare an owned native browser key event")
        }
        cg.flags = flags
        if !key.modifier.isEmpty { cg.type = .flagsChanged }
        cg.setIntegerValueField(.keyboardEventAutorepeat, value: repeated ? 1 : 0)
        var characters = key.characters
        if flags.contains(.maskShift), characters.count == 1,
           characters.first?.isASCII == true { characters = BrowserNativeKey.shifted(characters) }
        if !characters.isEmpty {
            let units = Array(characters.utf16)
            try units.withUnsafeBufferPointer { buffer in
                guard let address = buffer.baseAddress else { throw BridgeFailure(message: "Native key has no UTF-16 storage") }
                cg.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: address)
            }
        }
        sequence += 1
        cg.setIntegerValueField(.eventSourceUserData, value: prefix | Int64(sequence))
        guard let event = NSEvent(cgEvent: cg), owns(event) else {
            throw BridgeFailure(message: "Native browser event conversion lost its ownership")
        }
        return event
    }
}

@objc(ZmuxBrowserInputApplication)
@MainActor
final class BrowserInputApplication: NSApplication {
    let browserKeyboardEvents = BrowserKeyboardEvents()
    override func sendEvent(_ event: NSEvent) {
        if browserKeyboardEvents.contain(event, applicationDispatch: true) { return }
        super.sendEvent(event)
    }
}

/// Ordinary physical input is unchanged. Only explicitly owned direct events
/// and their asynchronous WebKit resends are intercepted.
@MainActor
final class BrowserKeyboardWebView: WKWebView {
    private var directDepth = 0
    private var events: BrowserKeyboardEvents? { (NSApp as? BrowserInputApplication)?.browserKeyboardEvents }
    func deliver(_ event: NSEvent) throws {
        guard let events, events.owns(event) else { throw BridgeFailure(message: "Native browser input containment is unavailable") }
        directDepth += 1
        events.directDeliveryDepth += 1
        defer { directDepth -= 1; events.directDeliveryDepth -= 1 }
        switch event.type {
        case .keyDown: super.keyDown(with: event)
        case .keyUp: super.keyUp(with: event)
        case .flagsChanged: super.flagsChanged(with: event)
        default: throw BridgeFailure(message: "Unsupported native browser event")
        }
    }
    override func keyDown(with event: NSEvent) {
        if directDepth == 0, events?.contain(event) == true { return }
        super.keyDown(with: event)
    }
    override func keyUp(with event: NSEvent) {
        if directDepth == 0, events?.contain(event) == true { return }
        super.keyUp(with: event)
    }
    override func flagsChanged(with event: NSEvent) {
        if directDepth == 0, events?.contain(event) == true { return }
        super.flagsChanged(with: event)
    }
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if directDepth == 0, events?.contain(event) == true { return true }
        return super.performKeyEquivalent(with: event)
    }
}

struct BrowserNativeKey {
    let code: CGKeyCode
    let characters: String
    var modifier: CGEventFlags = []
    private static let shiftedSymbols = ["1":"!","2":"@","3":"#","4":"$","5":"%","6":"^","7":"&","8":"*","9":"(","0":")","-":"_","=":"+","[":"{","]":"}","\\":"|",";":":","'":"\"",",":"<",".":">","/":"?","`":"~"]
    static func shifted(_ characters: String) -> String {
        shiftedSymbols[characters] ?? characters.uppercased()
    }
    static func requiresUnavailableNativeMapping(_ token: String) -> Bool {
        ["CapsLock", "NumLock", "ScrollLock", "Insert", "PrintScreen", "Pause", "ContextMenu", "Clear", "Fn", "Help", "Cancel", "Print", "Select", "Execute", "Undo", "Redo", "Cut", "Copy", "Paste", "Find", "Again", "Props", "AltGraph", "FnLock", "Hyper", "Super", "Symbol", "SymbolLock", "Unidentified"].contains(token)
            || token.hasPrefix("Numpad") && token != "NumpadEnter"
            || token.hasPrefix("F") && Int(token.dropFirst()).map { (21...35).contains($0) } == true
    }
    static func resolve(_ token: String) -> Self? {
        let modifiers: [String: (CGKeyCode, CGEventFlags)] = [
            "Shift": (56,.maskShift), "ShiftLeft": (56,.maskShift), "ShiftRight": (60,.maskShift),
            "Control": (59,.maskControl), "ControlLeft": (59,.maskControl), "ControlRight": (62,.maskControl),
            "Alt": (58,.maskAlternate), "AltLeft": (58,.maskAlternate), "AltRight": (61,.maskAlternate), "Option": (58,.maskAlternate),
            "Meta": (55,.maskCommand), "MetaLeft": (55,.maskCommand), "MetaRight": (54,.maskCommand), "Command": (55,.maskCommand)
        ]
        if let (code, flag) = modifiers[token] { return Self(code: code, characters: "", modifier: flag) }
        let named: [String: (CGKeyCode, UInt32)] = [
            "Enter":(36,13), "Return":(36,13), "NumpadEnter":(76,13), "Tab":(48,9),
            "Escape":(53,27), "Esc":(53,27), "Backspace":(51,127), "Delete":(117,0xf728),
            "ArrowLeft":(123,0xf702), "Left":(123,0xf702), "ArrowRight":(124,0xf703), "Right":(124,0xf703),
            "ArrowDown":(125,0xf701), "Down":(125,0xf701), "ArrowUp":(126,0xf700), "Up":(126,0xf700),
            "Home":(115,0xf729), "End":(119,0xf72b), "PageUp":(116,0xf72c), "PageDown":(121,0xf72d),
            "Space":(49,32), "Spacebar":(49,32), "space":(49,32)
        ]
        if let (code, scalar) = named[token], let value = UnicodeScalar(scalar) { return Self(code: code, characters: String(value)) }
        let functions: [CGKeyCode] = [122,120,99,118,96,97,98,100,101,109,103,111,105,107,113,106,64,79,80,90]
        if token.hasPrefix("F"), let number = Int(token.dropFirst()), (1...20).contains(number), token == "F\(number)",
           let scalar = UnicodeScalar(UInt32(0xf703 + number)) {
            return Self(code: functions[number - 1], characters: String(scalar))
        }
        let ascii: [String: CGKeyCode] = ["a":0,"s":1,"d":2,"f":3,"h":4,"g":5,"z":6,"x":7,"c":8,"v":9,"b":11,"q":12,"w":13,"e":14,"r":15,"y":16,"t":17,"1":18,"2":19,"3":20,"4":21,"6":22,"5":23,"=":24,"9":25,"7":26,"-":27,"8":28,"0":29,"]":30,"o":31,"u":32,"[":33,"i":34,"p":35,"l":37,"j":38,"'":39,"k":40,";":41,"\\":42,",":43,"/":44,"n":45,"m":46,".":47," ":49,"`":50]
        var value = token
        if token.hasPrefix("Key"), token.count == 4, let scalar = token.unicodeScalars.last, (65...90).contains(scalar.value) { value = String(token.suffix(1)).lowercased() }
        if token.hasPrefix("Digit"), token.count == 6, let scalar = token.unicodeScalars.last, (48...57).contains(scalar.value) { value = String(token.suffix(1)) }
        if value.utf8.count == 1, let code = ascii[value.lowercased()] { return Self(code: code, characters: value) }
        if let base = shiftedSymbols.first(where: { $0.value == token })?.key, let code = ascii[base] { return Self(code: code, characters: token) }
        return nil
    }
}

/// One host owns at most 64 held native keys. State never follows a replacement
/// host or document, and native creation/delivery failure never selects fallback.
@MainActor
final class BrowserKeyboardState {
    private var held: [CGKeyCode: BrowserNativeKey] = [:]
    var flags: CGEventFlags { held.values.reduce(CGEventFlags()) { $0.union($1.modifier) } }
    var modifiers: [String: Bool] { ["shift": flags.contains(.maskShift), "control": flags.contains(.maskControl), "alt": flags.contains(.maskAlternate), "meta": flags.contains(.maskCommand)] }
    func clear() { held.removeAll() }
    func execute(_ operation: BrowserKeyboardOperation, key: BrowserNativeKey, on view: BrowserKeyboardWebView, willDeliver: () throws -> Void) throws {
        guard let events = (NSApp as? BrowserInputApplication)?.browserKeyboardEvents else {
            throw BridgeFailure(message: "Native browser input containment is unavailable")
        }
        var next = held
        switch operation {
        case .down:
            guard next[key.code] != nil || next.count < 64 else { throw BridgeFailure(message: "Browser held-key capacity reached") }
            next[key.code] = key
        case .up: next.removeValue(forKey: key.code)
        case .press:
            guard held[key.code] == nil else { throw BridgeFailure(message: "Release this held key before pressing it") }
        }
        let nextFlags = next.values.reduce(CGEventFlags()) { $0.union($1.modifier) }
        if operation == .press {
            let down = try events.event(key, down: true, flags: flags.union(key.modifier))
            let up = try events.event(key, down: false, flags: flags)
            try willDeliver()
            try view.deliver(down); try view.deliver(up)
        } else {
            let event = try events.event(key, down: operation == .down, flags: nextFlags, repeated: operation == .down && held[key.code] != nil)
            try willDeliver()
            try view.deliver(event)
            held = next
        }
    }
}

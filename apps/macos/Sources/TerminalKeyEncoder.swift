import AppKit

enum TerminalKeyAction: String, Sendable {
    case press
    case repeated = "repeat"
    case release
}

struct TerminalKeyEvent: Sendable {
    let key: String
    let action: TerminalKeyAction
    let modifiers: Int
    let legacyControl: Int?
}

/// Map original native physical key codes to semantic host IDs. Rust's terminal
/// worker owns byte encoding against current terminal modes and protocol state.
enum TerminalKeyEncoder {
    private static let keys: [UInt16: String] = [
        123: "arrow_left", 124: "arrow_right", 125: "arrow_down", 126: "arrow_up",
        115: "home", 119: "end", 117: "delete", 116: "page_up", 121: "page_down",
        122: "f1", 120: "f2", 99: "f3", 118: "f4", 96: "f5", 97: "f6",
        98: "f7", 100: "f8", 101: "f9", 109: "f10", 103: "f11", 111: "f12",
        36: "enter", 76: "numpad_enter", 51: "backspace", 53: "escape", 48: "tab"
    ]

    static func event(keyCode: UInt16, modifiers: NSEvent.ModifierFlags, action: TerminalKeyAction, charactersIgnoringModifiers: String? = nil) -> TerminalKeyEvent? {
        guard let key = keys[keyCode] else { return nil }
        let flags = (modifiers.contains(.shift) ? 1 : 0)
            + (modifiers.contains(.option) ? 2 : 0) + (modifiers.contains(.control) ? 4 : 0)
        let legacyControl: Int?
        if modifiers.contains(.control), [36, 76, 51, 53, 48].contains(keyCode),
           let scalar = charactersIgnoringModifiers?.unicodeScalars.first, scalar.value < 128 {
            legacyControl = Int(scalar.value & 31)
        } else { legacyControl = nil }
        return TerminalKeyEvent(key: key, action: action, modifiers: flags, legacyControl: legacyControl)
    }
}

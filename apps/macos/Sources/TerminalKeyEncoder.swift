import AppKit

/// Original legacy xterm special-key encoding. Text composition and application
/// shortcuts remain the native view's responsibility, before this byte mapping.
enum TerminalKeyEncoder {
    private static let cursorFinals: [UInt16: String] = [
        123: "D", 124: "C", 125: "B", 126: "A", // Left, Right, Down, Up
        115: "H", 119: "F" // Home, End
    ]
    private static let functionFinals: [UInt16: String] = [
        122: "P", 120: "Q", 99: "R", 118: "S" // F1–F4
    ]
    private static let tildeNumbers: [UInt16: Int] = [
        117: 3, 116: 5, 121: 6, // Delete, Page Up, Page Down
        96: 15, 97: 17, 98: 18, 100: 19, // F5–F8
        101: 20, 109: 21, 103: 23, 111: 24 // F9–F12
    ]

    static func sequence(keyCode: UInt16, modifiers: NSEvent.ModifierFlags, applicationCursor: Bool) -> String? {
        let modifier = 1 + (modifiers.contains(.shift) ? 1 : 0)
            + (modifiers.contains(.option) ? 2 : 0) + (modifiers.contains(.control) ? 4 : 0)
        if let final = cursorFinals[keyCode] {
            return modifier > 1 ? "\u{1b}[1;\(modifier)\(final)" : "\u{1b}\(applicationCursor ? "O" : "[")\(final)"
        }
        if let final = functionFinals[keyCode] {
            return modifier > 1 ? "\u{1b}[1;\(modifier)\(final)" : "\u{1b}O\(final)"
        }
        if let number = tildeNumbers[keyCode] {
            return "\u{1b}[\(number)" + (modifier > 1 ? ";\(modifier)" : "") + "~"
        }
        return nil
    }
}

import AppKit
import ApplicationServices
import Darwin

/// External, permission-checked UI input for the real remote save panel.
/// No destination or completion callback is injected into the download owner.
@main
enum SavePanelDriver {
    struct Failure: Error, CustomStringConvertible {
        let description: String
    }

    @MainActor
    static func main() {
        do { try drive() }
        catch {
            FileHandle.standardError.write(Data(("Save-panel UI driver: \(error)\n").utf8))
            exit(1)
        }
    }

    @MainActor
    private static func drive() throws {
        let arguments = CommandLine.arguments
        guard arguments.count == 7, ["select", "accept"].contains(arguments[1]),
              let pid = Int32(arguments[2]), pid > 0, pid != getpid() else {
            throw Failure(description: "Expected select|accept APP_PID EXECUTABLE CURRENT_NAME DIRECTORY NEW_NAME")
        }
        // Never prompt, grant permissions or alter TCC. A host without this
        // authorization cannot establish native save acceptance.
        guard AXIsProcessTrusted() else {
            print("{\"accessibility_trusted\":false,\"native_save_acceptance\":\"gated\"}")
            throw Failure(description: "External Accessibility input is not trusted; native download acceptance remains gated. No permission prompt or TCC change was attempted.")
        }
        guard CGPreflightPostEventAccess() else {
            throw Failure(description: "Targeted native key input is not authorized; native download acceptance remains gated. No permission request was attempted.")
        }
        guard let running = NSRunningApplication(processIdentifier: pid),
              running.executableURL?.standardizedFileURL.path == URL(fileURLWithPath: arguments[3]).standardizedFileURL.path else {
            throw Failure(description: "Target PID is not the exact packaged application executable")
        }
        let app = AXUIElementCreateApplication(pid)
        var actual: pid_t = 0
        guard AXUIElementGetPid(app, &actual) == .success, actual == pid else {
            throw Failure(description: "Accessibility application identity differs from the target PID")
        }
        let deadline = ContinuousClock.now + .seconds(10)
        // The public AX attribute has no kAXSheetsAttribute export in this SDK.
        let sheetsAttribute = "AXSheets"
        func checkDeadline() throws {
            guard ContinuousClock.now < deadline else { throw Failure(description: "Accessibility UI input exceeded its deadline") }
        }
        func attribute(_ element: AXUIElement, _ key: String) throws -> CFTypeRef? {
            try checkDeadline()
            guard AXUIElementSetMessagingTimeout(element, 0.25) == .success else {
                throw Failure(description: "Could not bound Accessibility messaging")
            }
            var value: CFTypeRef?
            let status = AXUIElementCopyAttributeValue(element, key as CFString, &value)
            if status == .attributeUnsupported || status == .noValue { return nil }
            guard status == .success else { throw Failure(description: "Accessibility read \(key) failed (\(status.rawValue))") }
            return value
        }
        func text(_ element: AXUIElement, _ key: String) throws -> String? {
            try attribute(element, key) as? String
        }
        func children(_ element: AXUIElement, _ key: String = kAXChildrenAttribute) throws -> [AXUIElement] {
            guard let value = try attribute(element, key) else { return [] }
            guard let result = value as? [AXUIElement], result.count <= 256 else {
                throw Failure(description: "Accessibility children exceeded their bound or type")
            }
            return result
        }
        func descendants(_ root: AXUIElement) throws -> [AXUIElement] {
            var queue = [root], result: [AXUIElement] = []
            while !queue.isEmpty {
                let element = queue.removeFirst()
                if result.contains(where: { CFEqual($0, element) }) { continue }
                guard result.count < 256 else { throw Failure(description: "Save-panel Accessibility tree exceeded its bound") }
                result.append(element)
                queue.append(contentsOf: try children(element))
                queue.append(contentsOf: try children(element, sheetsAttribute))
                guard queue.count <= 512 else { throw Failure(description: "Save-panel Accessibility traversal exceeded its queue bound") }
            }
            return result
        }
        func set(_ element: AXUIElement, _ key: String, _ value: CFTypeRef) throws {
            try checkDeadline()
            let status = AXUIElementSetAttributeValue(element, key as CFString, value)
            guard status == .success else { throw Failure(description: "Accessibility write \(key) failed (\(status.rawValue))") }
        }
        func wait(_ label: String, _ predicate: () throws -> Bool) throws {
            while try !predicate() {
                try checkDeadline()
                Thread.sleep(forTimeInterval: 0.025)
            }
            print("Confirmed \(label)")
        }
        var matches: [(window: AXUIElement, sheet: AXUIElement, name: AXUIElement, save: AXUIElement)] = []
        for window in try children(app, kAXWindowsAttribute) {
            var windowPID: pid_t = 0
            guard AXUIElementGetPid(window, &windowPID) == .success, windowPID == pid else {
                throw Failure(description: "Accessibility window does not belong to the exact target app")
            }
            let sheets = try children(window, sheetsAttribute)
            for sheet in sheets {
                guard try text(sheet, kAXRoleAttribute) == kAXSheetRole else { continue }
                let controls = try descendants(sheet)
                let names = try controls.filter { try text($0, kAXRoleAttribute) == kAXTextFieldRole && text($0, kAXValueAttribute) == arguments[4] }
                let saves = try controls.filter { try text($0, kAXRoleAttribute) == kAXButtonRole && text($0, kAXTitleAttribute) == "Save" }
                if names.count == 1 && saves.count == 1 { matches.append((window, sheet, names[0], saves[0])) }
            }
        }
        guard matches.count == 1, let target = matches.first else {
            throw Failure(description: "Expected one actual save sheet with the exact current filename and Save button; found \(matches.count)")
        }
        if arguments[1] == "accept" {
            guard arguments[4] == arguments[6], try attribute(target.save, kAXEnabledAttribute) as? Bool == true else {
                throw Failure(description: "Save control is disabled or filename differs from the selected fixture destination")
            }
            let status = AXUIElementPerformAction(target.save, kAXPressAction as CFString)
            guard status == .success else { throw Failure(description: "Actual save button AXPress failed (\(status.rawValue))") }
        } else {
            try set(app, kAXFrontmostAttribute, kCFBooleanTrue)
            guard AXUIElementPerformAction(target.window, kAXRaiseAction as CFString) == .success else {
                throw Failure(description: "Could not raise the exact save-sheet owner")
            }
            try set(target.name, kAXValueAttribute, arguments[6] as CFString)
            try set(target.name, kAXFocusedAttribute, kCFBooleanTrue)
            try wait("exact target save filename focus") {
                guard let focused = try attribute(app, kAXFocusedUIElementAttribute),
                      CFGetTypeID(focused) == AXUIElementGetTypeID() else { return false }
                return CFEqual(unsafeDowncast(focused, to: AXUIElement.self), target.name)
            }
            func key(_ code: CGKeyCode, flags: CGEventFlags = []) throws {
                for down in [true, false] {
                    guard let event = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down) else {
                        throw Failure(description: "Could not create target-specific native key input")
                    }
                    event.flags = flags
                    event.postToPid(pid)
                }
            }
            // Standard Go to Folder shortcut, delivered only to this app PID.
            // There is no frontmost-app/global event posting fallback.
            try key(5, flags: [.maskCommand, .maskShift])
            var pathField: AXUIElement?
            try wait("target save sheet Go to Folder input") {
                guard let value = try attribute(app, kAXFocusedUIElementAttribute),
                      CFGetTypeID(value) == AXUIElementGetTypeID() else { return false }
                let element = unsafeDowncast(value, to: AXUIElement.self)
                let role = try text(element, kAXRoleAttribute)
                guard !CFEqual(element, target.name), role == kAXTextFieldRole || role == kAXComboBoxRole else { return false }
                // The focused entry must be reachable from this exact sheet,
                // including its standard Go to Folder sub-sheet.
                guard try descendants(target.sheet).contains(where: { CFEqual($0, element) }) else { return false }
                pathField = element
                return true
            }
            guard let pathField else { throw Failure(description: "Go to Folder omitted its text entry") }
            try set(pathField, kAXValueAttribute, arguments[5] as CFString)
            guard try text(pathField, kAXValueAttribute) == arguments[5] else {
                throw Failure(description: "Go to Folder did not retain the exact fixture directory")
            }
            try key(36)
            try wait("Go to Folder completion") {
                guard let value = try attribute(app, kAXFocusedUIElementAttribute),
                      CFGetTypeID(value) == AXUIElementGetTypeID() else { return false }
                return !CFEqual(unsafeDowncast(value, to: AXUIElement.self), pathField)
            }
            guard try text(target.name, kAXValueAttribute) == arguments[6] else {
                throw Failure(description: "Save panel did not retain the requested fixture filename")
            }
        }
        print("{\"accessibility_trusted\":true,\"post_event_access\":true,\"target_pid\":\(pid),\"exact_save_sheet\":true,\"phase\":\"\(arguments[1])\"}")
    }
}

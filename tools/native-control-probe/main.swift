import AppKit
import Darwin

@MainActor
final class Receiver: NSObject {
    var count = 0
    @objc func receive(_ sender: Any?) { count += 1 }
}

@MainActor
final class ProbeControl: NSControl {
    var mouseDownCount = 0
    var mouseSendResult: Bool?
    override func mouseDown(with event: NSEvent) {
        mouseDownCount += 1
        mouseSendResult = sendAction(action, to: target)
    }
}

@MainActor
func describe(_ control: ProbeControl, receiver: Receiver) -> [String: Any] {
    ["cell_type": control.cell.map { String(describing: type(of: $0)) } ?? "nil",
     "target_present": control.target != nil,
     "target_is_receiver": control.target === receiver,
     "action": control.action.map(NSStringFromSelector) ?? "nil",
     "target_invocation_count": receiver.count]
}

@MainActor
func runProbe() throws {
    let receiver = Receiver()
    let plain = ProbeControl(frame: NSRect(x: 20, y: 20, width: 200, height: 50))
    let plainBefore = describe(plain, receiver: receiver)
    plain.target = receiver
    plain.action = #selector(Receiver.receive(_:))
    let plainConfigured = describe(plain, receiver: receiver)
    let plainSent = plain.sendAction(plain.action, to: plain.target)
    let plainAfter = describe(plain, receiver: receiver)

    let explicit = ProbeControl(frame: NSRect(x: 20, y: 100, width: 200, height: 50))
    explicit.cell = NSActionCell()
    explicit.target = receiver
    explicit.action = #selector(Receiver.receive(_:))
    let explicitBefore = describe(explicit, receiver: receiver)
    let explicitSent = explicit.sendAction(explicit.action, to: explicit.target)
    let explicitAfter = describe(explicit, receiver: receiver)

    let window = NSWindow(contentRect: NSRect(x: 100, y: 100, width: 300, height: 200),
                          styleMask: [.titled, .closable], backing: .buffered, defer: false)
    window.isReleasedWhenClosed = false
    let label = NSTextField(labelWithString: "Action cell pointer probe")
    label.frame = NSRect(x: 5, y: 5, width: 185, height: 30)
    explicit.addSubview(label)
    window.contentView!.addSubview(explicit)
    window.makeKeyAndOrderFront(nil)
    window.contentView!.layoutSubtreeIfNeeded()
    window.displayIfNeeded()
    let point = explicit.convert(NSPoint(x: 100, y: 25), to: nil)
    let hit = window.contentView!.hitTest(window.contentView!.convert(point, from: nil))
    let beforePointer = receiver.count
    guard let down = NSEvent.mouseEvent(with: .leftMouseDown, location: point, modifierFlags: [],
                                        timestamp: ProcessInfo.processInfo.systemUptime,
                                        windowNumber: window.windowNumber, context: nil,
                                        eventNumber: 1, clickCount: 1, pressure: 1),
          let up = NSEvent.mouseEvent(with: .leftMouseUp, location: point, modifierFlags: [],
                                      timestamp: ProcessInfo.processInfo.systemUptime + 0.01,
                                      windowNumber: window.windowNumber, context: nil,
                                      eventNumber: 2, clickCount: 1, pressure: 0) else {
        throw NSError(domain: "ControlProbe", code: 1)
    }
    // Queue the real AppKit mouse-up before dispatching mouse-down, so a control
    // tracking loop can consume it without waiting for another producer.
    NSApp.postEvent(up, atStart: false)
    window.sendEvent(down)
    let result: [String: Any] = [
        "plain_before": plainBefore, "plain_configured": plainConfigured,
        "plain_send_action_bool": plainSent, "plain_after": plainAfter,
        "explicit_configured": explicitBefore, "explicit_send_action_bool": explicitSent,
        "explicit_after": explicitAfter,
        "pointer": ["hit_type": hit.map { String(describing: type(of: $0)) } ?? "nil",
                    "mouse_down_count": explicit.mouseDownCount,
                    "send_action_bool": explicit.mouseSendResult.map { $0 as Any } ?? NSNull(),
                    "actual_target_invocations": receiver.count - beforePointer],
        "total_actual_target_invocations": receiver.count]
    let data = try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
    try data.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
    print(String(decoding: data, as: UTF8.self))
    window.close()
}

let application = NSApplication.shared
application.setActivationPolicy(.regular)
application.finishLaunching()
application.activate()
DispatchQueue.main.async {
    do {
        try runProbe()
        exit(0)
    } catch {
        FileHandle.standardError.write(Data("Control probe failed: \(error)\n".utf8))
        exit(1)
    }
}
application.run()

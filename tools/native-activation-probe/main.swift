import AppKit

@MainActor
final class Delegate: NSObject, NSApplicationDelegate {
    let window = NSWindow(contentRect: NSRect(x: 160, y: 160, width: 600, height: 360),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
    let output: URL
    var results: [[String: Any]] = []
    let menu = NSMenu(title: "Activation")
    var status: NSStatusItem?
    var menuActions = 0

    init(output: URL) { self.output = output; super.init() }
    func applicationDidFinishLaunching(_ notification: Notification) {
        window.title = "zmux activation probe"
        window.isReleasedWhenClosed = false
        menu.autoenablesItems = false
        let show = menu.addItem(withTitle: "Show probe", action: #selector(showFromMenu), keyEquivalent: "")
        show.target = self
        status = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        status?.menu = menu
        status?.button?.image = NSImage(systemSymbolName: "terminal", accessibilityDescription: "Activation probe")
        window.makeKeyAndOrderFront(nil)
        Task {
            do {
                try await activateThroughLaunchServices()
                record("initial")
                for scenario in ["menu-action", "order-then-activate", "activate-then-order", "await-active-then-order", "running-application"] {
                    NSApp.hide(nil)
                    await wait { NSApp.isHidden }
                    record(scenario + ":hidden")
                    switch scenario {
                    case "menu-action": menu.performActionForItem(at: 0)
                    case "order-then-activate":
                        NSApp.unhide(nil); window.makeKeyAndOrderFront(nil); NSApp.activate()
                    case "activate-then-order":
                        NSApp.unhide(nil); NSApp.activate(); window.makeKeyAndOrderFront(nil)
                    case "await-active-then-order":
                        NSApp.unhide(nil); NSApp.activate(); await wait { NSApp.isActive }
                        window.makeKeyAndOrderFront(nil)
                    default:
                        NSApp.unhide(nil)
                        let admitted = NSRunningApplication.current.activate(options: [])
                        record(scenario + ":admitted=\(admitted)")
                        await wait { NSApp.isActive }
                        window.makeKeyAndOrderFront(nil)
                    }
                    await wait { !NSApp.isHidden && NSApp.isActive && self.window.isKeyWindow }
                    record(scenario + ":result")
                    try await activateThroughLaunchServices()
                    record(scenario + ":launch-services-control")
                }
                try JSONSerialization.data(withJSONObject: ["pid": getpid(), "observations": results],
                    options: [.prettyPrinted, .sortedKeys]).write(to: output, options: .atomic)
                if let status { NSStatusBar.system.removeStatusItem(status) }
                exit(0)
            } catch {
                record("failure: " + error.localizedDescription)
                do {
                    try JSONSerialization.data(withJSONObject: ["pid": getpid(), "observations": results],
                        options: [.prettyPrinted, .sortedKeys]).write(to: output, options: .atomic)
                } catch { FileHandle.standardError.write(Data("Probe receipt failed: \(error)\n".utf8)) }
                exit(1)
            }
        }
    }
    @objc private func showFromMenu() {
        menuActions += 1
        NSApp.unhide(nil); window.makeKeyAndOrderFront(nil); NSApp.activate()
    }
    private func wait(_ predicate: @MainActor () -> Bool) async {
        let deadline = ContinuousClock.now + .seconds(2)
        while !predicate(), ContinuousClock.now < deadline {
            do { try await Task.sleep(for: .milliseconds(20)) }
            catch { return }
        }
    }
    private func record(_ stage: String) {
        var state: [String: Any] = ["stage": stage, "active": NSApp.isActive, "hidden": NSApp.isHidden,
            "window_key": window.isKeyWindow, "window_visible": window.isVisible,
            "key_window": NSApp.keyWindow?.windowNumber ?? -1,
            "own_window": window.windowNumber, "frontmost_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1]
        state["menu_actions"] = menuActions
        results.append(state)
        FileHandle.standardError.write(Data((String(describing: state) + "\n").utf8))
    }
    private func activateThroughLaunchServices() async throws {
        let current = NSRunningApplication.current
        let bundle = Bundle.main.bundleURL.standardizedFileURL
        guard current.processIdentifier == getpid(), current.bundleURL?.standardizedFileURL == bundle,
              let identifier = current.bundleIdentifier,
              NSWorkspace.shared.runningApplications.filter({ $0.bundleIdentifier == identifier }).map(\.processIdentifier) == [getpid()] else {
            throw NSError(domain: "ActivationProbe", code: 1, userInfo: [NSLocalizedDescriptionKey: "Probe identity is not uniquely registered"])
        }
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true
        configuration.createsNewApplicationInstance = false
        configuration.allowsRunningApplicationSubstitution = false
        configuration.arguments = []
        let pid: Int32 = try await withCheckedThrowingContinuation { continuation in
            NSWorkspace.shared.openApplication(at: bundle, configuration: configuration) { @Sendable app, error in
                if let error { continuation.resume(throwing: error) }
                else if let app { continuation.resume(returning: app.processIdentifier) }
                else { continuation.resume(throwing: NSError(domain: "ActivationProbe", code: 2)) }
            }
        }
        guard pid == getpid() else { throw NSError(domain: "ActivationProbe", code: 3) }
        await wait { NSApp.isActive }
        window.makeKeyAndOrderFront(nil)
        await wait { self.window.isKeyWindow }
    }
}

@main
@MainActor
enum Probe {
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        let delegate = Delegate(output: URL(fileURLWithPath: CommandLine.arguments[1]))
        app.delegate = delegate
        withExtendedLifetime(delegate) { app.run() }
    }
}

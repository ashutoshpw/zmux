import AppKit
import Darwin

private enum UICommand: Sendable {
    case action(String, UInt64?, String?, String?, UInt64?)
    case input(UInt64, String)
    case resize(UInt64, Int, Int)
}

@MainActor
final class AppController: NSObject {
    let window: NSWindow
    private let bridge = NativeBridge()
    private let sidebar = NSStackView()
    private let canvas = NSView()
    private let status = NSTextField(labelWithString: "Starting shell…")
    private let stream: AsyncStream<UICommand>
    private let continuation: AsyncStream<UICommand>.Continuation
    private var commandTask: Task<Void, Never>?
    private var pollingTask: Task<Void, Never>?
    private var layoutKey = ""
    private var modelRevision: UInt64 = .max
    private var terminals: [UInt64: TerminalView] = [:]
    private var buttons: [NSButton: UInt64] = [:]
    private var workspaceCount = 1
    private(set) var snapshot: DesktopSnapshot?

    override init() {
        (stream, continuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingOldest(256))
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1100, height: 720), styleMask: [.titled,.closable,.miniaturizable,.resizable], backing: .buffered, defer: false)
        super.init()
        window.title = "Zmux DEV"
        window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 600, height: 360)
        window.appearance = NSAppearance(named: .darkAqua)
        window.center()
        buildLayout()
        buildMenus()
    }
    func start() async throws {
        try await bridge.start()
        commandTask = Task { [weak self, stream] in
            for await command in stream {
                guard let self, !Task.isCancelled else { break }
                do {
                    switch command {
                    case .action(let action, let pane, let axis, let title, let workspace):
                        try await self.bridge.action(action, pane: pane, axis: axis, title: title, workspace: workspace)
                    case .input(let session, let text): try await self.bridge.input(session: session, text: text)
                    case .resize(let session, let rows, let cols): try await self.bridge.resize(session: session, rows: rows, cols: cols)
                    }
                } catch { self.status.stringValue = error.localizedDescription }
            }
        }
        pollingTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { break }
                do {
                    if let snapshot = try await self.bridge.poll() { self.present(snapshot) }
                    try await Task.sleep(for: .milliseconds(32))
                } catch is CancellationError { break }
                catch {
                    self.status.stringValue = error.localizedDescription
                    do { try await Task.sleep(for: .milliseconds(250)) } catch { break }
                }
            }
        }
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
    }
    func shutdown() async throws {
        continuation.finish()
        commandTask?.cancel(); pollingTask?.cancel()
        try await bridge.close()
    }
    private func enqueue(_ command: UICommand) {
        if case .dropped = continuation.yield(command) { status.stringValue = "Command queue is full; the new command was not accepted" }
    }
    private func buildLayout() {
        let content = NSView()
        window.contentView = content
        let header = NSStackView()
        header.orientation = .horizontal; header.spacing = 8
        let title = NSTextField(labelWithString: "Zmux")
        title.font = .systemFont(ofSize: 15, weight: .semibold)
        header.addArrangedSubview(title)
        for (label, selector) in [("New Workspace",#selector(newWorkspace)),("Split Right",#selector(splitRight)),("Split Down",#selector(splitDown)),("Rename",#selector(renameWorkspace)),("Close Pane",#selector(closePane))] {
            let button = NSButton(title: label, target: self, action: selector)
            header.addArrangedSubview(button)
        }
        let body = NSStackView()
        body.orientation = .horizontal; body.alignment = .top; body.spacing = 0
        let sidebarContainer = NSView()
        sidebar.orientation = .vertical; sidebar.alignment = .leading; sidebar.spacing = 6
        sidebar.translatesAutoresizingMaskIntoConstraints = false
        sidebarContainer.addSubview(sidebar)
        NSLayoutConstraint.activate([sidebar.leadingAnchor.constraint(equalTo: sidebarContainer.leadingAnchor, constant: 10),sidebar.trailingAnchor.constraint(equalTo: sidebarContainer.trailingAnchor, constant: -10),sidebar.topAnchor.constraint(equalTo: sidebarContainer.topAnchor, constant: 12)])
        body.addArrangedSubview(sidebarContainer); body.addArrangedSubview(canvas)
        for view in [header, body, status] { view.translatesAutoresizingMaskIntoConstraints = false; content.addSubview(view) }
        status.font = .systemFont(ofSize: 11); status.textColor = .secondaryLabelColor
        status.lineBreakMode = .byTruncatingMiddle
        NSLayoutConstraint.activate([
            header.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),header.topAnchor.constraint(equalTo: content.topAnchor, constant: 8),header.heightAnchor.constraint(equalToConstant: 32),
            body.leadingAnchor.constraint(equalTo: content.leadingAnchor),body.trailingAnchor.constraint(equalTo: content.trailingAnchor),body.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 8),body.bottomAnchor.constraint(equalTo: status.topAnchor, constant: -4),
            sidebarContainer.widthAnchor.constraint(equalToConstant: 180),sidebarContainer.heightAnchor.constraint(equalTo: body.heightAnchor),canvas.heightAnchor.constraint(equalTo: body.heightAnchor),
            status.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),status.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),status.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -5),status.heightAnchor.constraint(equalToConstant: 18)
        ])
    }
    private func buildMenus() {
        let menu = NSMenu()
        let appItem = NSMenuItem(); menu.addItem(appItem)
        let appMenu = NSMenu(title: "Zmux")
        appMenu.addItem(withTitle: "Quit Zmux DEV", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu
        let workspaceItem = NSMenuItem(title:"Workspace",action:nil,keyEquivalent:""); menu.addItem(workspaceItem)
        let workspaceMenu = NSMenu(title: "Workspace")
        let items: [(String, Selector, String)] = [("New Workspace",#selector(newWorkspace),"n"),("Split Right",#selector(splitRight),"d"),("Split Down",#selector(splitDown),"D"),("Rename Workspace",#selector(renameWorkspace),""),("Close Pane",#selector(closePane),"w")]
        for (title, action, key) in items { let item = workspaceMenu.addItem(withTitle: title, action: action, keyEquivalent: key); item.target = self }
        workspaceItem.submenu = workspaceMenu
        let editItem = NSMenuItem(title:"Edit",action:nil,keyEquivalent:""); menu.addItem(editItem)
        let edit = NSMenu(title: "Edit")
        for (title, action, key) in [("Copy",#selector(copySelection),"c"),("Paste",#selector(pasteText),"v"),("Select All",#selector(selectAllText),"a")] {
            let item = edit.addItem(withTitle: title, action: action, keyEquivalent: key); item.target = self
        }
        editItem.submenu = edit
        NSApp.mainMenu = menu
    }
    private func present(_ snapshot: DesktopSnapshot) {
        self.snapshot = snapshot
        status.stringValue = snapshot.lastError ?? "Local shells · \(snapshot.model.workspaces.count) workspace(s) · ⌘D split · ⌘N workspace"
        if modelRevision != snapshot.model.revision {
            modelRevision = snapshot.model.revision
            sidebar.arrangedSubviews.forEach { sidebar.removeArrangedSubview($0); $0.removeFromSuperview() }
            buttons.removeAll()
            for workspace in snapshot.model.workspaces {
                let button = NSButton(title: workspace.name, target: self, action: #selector(workspaceSelected(_:)))
                button.contentTintColor = workspace.id == snapshot.model.focusedWorkspace ? .systemMint : .labelColor
                sidebar.addArrangedSubview(button); buttons[button] = workspace.id
                button.widthAnchor.constraint(equalTo: sidebar.widthAnchor).isActive = true
            }
        }
        let key = snapshot.workspace?.root.key ?? "empty"
        if key != layoutKey {
            layoutKey = key
            canvas.subviews.forEach { $0.removeFromSuperview() }
            terminals.removeAll()
            if let workspace = snapshot.workspace {
                let root = paneView(workspace.root)
                root.frame = canvas.bounds; root.autoresizingMask = [.width,.height]
                canvas.addSubview(root)
                initializeSplits(root)
            } else {
                let empty = NSTextField(labelWithString: "Create a workspace to open a shell")
                empty.frame = NSRect(x: 20,y: 20,width: 400,height: 40); canvas.addSubview(empty)
            }
            canvas.layoutSubtreeIfNeeded()
        }
        for frame in snapshot.frames { terminals[frame.sessionId]?.update(frame) }
        if let pane = snapshot.workspace?.root.panes.first(where: { $0.id == snapshot.workspace?.focusedPane }), let terminal = terminals[pane.sessionId], window.firstResponder !== terminal, !(window.firstResponder is NSTextView) {
            window.makeFirstResponder(terminal)
        }
    }
    private func paneView(_ tree: PaneTree) -> NSView {
        switch tree {
        case .leaf(let pane):
            let view = TerminalView(pane: pane)
            view.onInput = { [weak self] text in self?.enqueue(.input(pane.sessionId, text)) }
            view.onResize = { [weak self] rows, cols in self?.enqueue(.resize(pane.sessionId, rows, cols)) }
            view.onFocus = { [weak self] in self?.enqueue(.action("focus_pane", pane.id, nil, nil, nil)) }
            terminals[pane.sessionId] = view
            return view
        case .split(let axis, let first, let second):
            let split = NSSplitView()
            split.isVertical = axis == "horizontal"; split.dividerStyle = .thin
            split.addArrangedSubview(paneView(first)); split.addArrangedSubview(paneView(second))
            split.adjustSubviews()
            return split
        }
    }
    private func initializeSplits(_ view: NSView) {
        if let split = view as? NSSplitView {
            split.adjustSubviews()
            split.setPosition((split.isVertical ? split.bounds.width : split.bounds.height) / 2, ofDividerAt:0)
            split.subviews.forEach(initializeSplits)
        }
    }
    @objc func newWorkspace() { workspaceCount += 1; enqueue(.action("create_workspace",nil,nil,"Workspace \(workspaceCount)",nil)) }
    @objc func splitRight() { split("horizontal") }
    @objc func splitDown() { split("vertical") }
    private func split(_ axis: String) { if let pane = snapshot?.workspace?.focusedPane { enqueue(.action("split_pane",pane,axis,nil,nil)) } }
    @objc func closePane() { if let pane = snapshot?.workspace?.focusedPane { enqueue(.action("close_pane",pane,nil,nil,nil)) } }
    @objc func workspaceSelected(_ sender: NSButton) { if let id = buttons[sender] { selectWorkspace(id) } }
    func selectWorkspace(_ id: UInt64) {
        if let workspace = snapshot?.model.workspaces.first(where: { $0.id == id }) { enqueue(.action("focus_pane",workspace.focusedPane,nil,nil,nil)) }
    }
    @objc func renameWorkspace() {
        guard let workspace = snapshot?.workspace else { return }
        let alert = NSAlert(); alert.messageText = "Rename workspace"
        let field = NSTextField(string: workspace.name); field.frame = NSRect(x:0,y:0,width:280,height:24)
        alert.accessoryView = field; alert.addButton(withTitle: "Rename"); alert.addButton(withTitle: "Cancel")
        if alert.runModal() == .alertFirstButtonReturn { enqueue(.action("rename_workspace",nil,nil,field.stringValue,workspace.id)) }
    }
    private var focusedTerminal: TerminalView? {
        guard let snapshot, let pane = snapshot.workspace?.root.panes.first(where: { $0.id == snapshot.workspace?.focusedPane }) else { return nil }
        return terminals[pane.sessionId]
    }
    @objc func copySelection(_ sender: Any?) { if let terminal = window.firstResponder as? TerminalView { terminal.copySelection(sender) } else { NSApp.sendAction(NSSelectorFromString("copy:"),to:nil,from:sender) } }
    @objc func pasteText(_ sender: Any?) { if let terminal = window.firstResponder as? TerminalView { terminal.pasteFromClipboard(sender) } else { NSApp.sendAction(NSSelectorFromString("paste:"),to:nil,from:sender) } }
    @objc func selectAllText(_ sender: Any?) { if let terminal = window.firstResponder as? TerminalView { terminal.selectViewport(sender) } else { NSApp.sendAction(NSSelectorFromString("selectAll:"),to:nil,from:sender) } }

    func smokeTest(report: URL) async throws {
        try await waitUntil { $0.frames.count == 1 && $0.frames.allSatisfy { $0.snapshot?.state == "running" } }
        guard let first = snapshot?.workspace else { throw BridgeFailure(message: "No initial workspace") }
        focusedTerminal?.insertText("printf '\\033[31mZMUX_NATIVE_SMOKE\\033[0m\\n'\r",replacementRange:NSRange(location:NSNotFound,length:0))
        try await waitUntil { snapshot in snapshot.frames.contains { frame in frame.snapshot?.viewport.lines.contains { row in row.contains { $0.text.contains("ZMUX_NATIVE_SMOKE") && $0.style.foreground.index == 1 } } == true } }
        splitRight()
        try await waitUntil { $0.workspace?.root.panes.count == 2 && $0.frames.count == 2 && $0.frames.allSatisfy { $0.snapshot?.state == "running" } }
        window.setContentSize(NSSize(width:1250,height:820)); window.contentView?.layoutSubtreeIfNeeded()
        try await waitUntil { [weak self] snapshot in
            guard let terminal = self?.focusedTerminal, let frame = snapshot.frames.first(where: { $0.sessionId == terminal.pane.sessionId }), let size = frame.snapshot?.viewport.size else { return false }
            return size.rows > 1 && size.cols > 1 && terminal.requestedSize == (size.rows,size.cols)
        }
        guard let size = focusedTerminal?.requestedSize else { throw BridgeFailure(message: "No resized native terminal") }
        focusedTerminal?.insertText("stty size; printf '\\nZMUX_SPLIT_OK\\n'\r",replacementRange:NSRange(location:NSNotFound,length:0))
        try await waitUntil { snapshot in snapshot.frames.contains { frame in
            guard let contents = frame.snapshot?.viewport.contents else { return false }
            return contents.components(separatedBy:"\n").contains("\(size.0) \(size.1)") && contents.components(separatedBy:"\n").contains("ZMUX_SPLIT_OK")
        } }
        newWorkspace()
        try await waitUntil { $0.model.workspaces.count == 2 && $0.frames.count == 1 && $0.frames.first?.snapshot?.state == "running" }
        selectWorkspace(first.id)
        try await waitUntil { $0.model.focusedWorkspace == first.id && $0.frames.count == 2 }
        closePane()
        try await waitUntil { $0.workspace?.root.panes.count == 1 && $0.frames.count == 1 }
        guard let view = window.contentView, let image = view.bitmapImageRepForCachingDisplay(in:view.bounds) else { throw BridgeFailure(message: "Native drawing surface unavailable") }
        view.displayIfNeeded()
        view.effectiveAppearance.performAsCurrentDrawingAppearance {
            view.cacheDisplay(in:view.bounds,to:image)
        }
        guard let png = image.representation(using:.png,properties:[:]), png.count > 4096 else { throw BridgeFailure(message: "Native render did not produce a usable PNG") }
        let directory = report.deletingLastPathComponent()
        try png.write(to: directory.appendingPathComponent("Zmux-native-smoke.png"),options:.atomic)
        // WindowServer capture includes composited native controls omitted by view caching.
        // Run the external tool off AppKit; availability depends on runner screen permissions.
        let windowNumber = String(window.windowNumber)
        let capturePath = directory.appendingPathComponent("Zmux-native-window.png").path
        let captureStatus = try await Task.detached {
            let capture = Process()
            capture.executableURL = URL(fileURLWithPath:"/usr/sbin/screencapture")
            capture.arguments = ["-x","-o","-l",windowNumber,capturePath]
            try capture.run()
            capture.waitUntilExit()
            return capture.terminationStatus
        }.value
        let evidence: [String: Any] = ["success":true,"native_input":true,"ansi_color":true,"split_panes":true,"kernel_resize":true,"workspace_switch":true,"pane_close":true,"native_render_png_bytes":png.count,"window_capture_status":captureStatus,"source_sha":Bundle.main.object(forInfoDictionaryKey:"ZmuxSourceSHA") as? String ?? "local"]
        try JSONSerialization.data(withJSONObject:evidence,options:[.prettyPrinted,.sortedKeys]).write(to:report,options:.atomic)
    }
    private func waitUntil(_ predicate: (DesktopSnapshot) -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(20)
        while ContinuousClock.now < deadline {
            if let snapshot, predicate(snapshot) { return }
            try await Task.sleep(for:.milliseconds(32))
        }
        throw BridgeFailure(message:"Native smoke condition timed out: \(status.stringValue)")
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var controller: AppController?
    private var terminating = false
    func applicationDidFinishLaunching(_ notification: Notification) {
        let controller = AppController(); self.controller = controller
        Task {
            do {
                try await controller.start()
                if let index = CommandLine.arguments.firstIndex(of:"--smoke-test"), CommandLine.arguments.count > index + 1 {
                    let report = URL(fileURLWithPath:CommandLine.arguments[index+1])
                    do {
                        try await controller.smokeTest(report:report)
                        try await controller.shutdown()
                        exit(0)
                    } catch {
                        FileHandle.standardError.write(Data("Native smoke failed: \(error.localizedDescription)\n".utf8))
                        do { try await controller.shutdown() } catch { FileHandle.standardError.write(Data("Cleanup failed: \(error)\n".utf8)) }
                        exit(1)
                    }
                }
            } catch {
                FileHandle.standardError.write(Data("Zmux startup failed: \(error)\n".utf8))
                if CommandLine.arguments.contains("--smoke-test") { exit(1) }
                let alert = NSAlert(); alert.messageText = "Unable to start Zmux"; alert.informativeText = error.localizedDescription
                alert.runModal(); NSApp.terminate(nil)
            }
        }
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let controller else { return .terminateNow }
        if terminating { return .terminateLater }
        terminating = true
        Task {
            do { try await controller.shutdown() } catch { FileHandle.standardError.write(Data("Zmux shutdown: \(error)\n".utf8)) }
            sender.reply(toApplicationShouldTerminate:true)
        }
        return .terminateLater
    }
}

@main
@MainActor
enum ZmuxMain {
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        let delegate = AppDelegate(); app.delegate = delegate
        withExtendedLifetime(delegate) { app.run() }
    }
}

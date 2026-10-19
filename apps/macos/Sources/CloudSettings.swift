import AppKit
import AuthenticationServices

@MainActor
final class CloudWindowController: NSWindowController, NSWindowDelegate, ASWebAuthenticationPresentationContextProviding {
    private weak var appController: MainWindowHost?
    private let endpoint: CloudEndpoint
    private let client: CloudClient
    private let terminalHost: CloudTerminalHost
    private let authenticationPresentationAnchor: NSWindow
    private let accountLabel = NSTextField(labelWithString: "Connect your workspace")
    private let detailLabel = NSTextField(wrappingLabelWithString: "Sign in to register this Mac and manage your devices.")
    private let message = NSTextField(wrappingLabelWithString: "")
    private let devices = NSStackView()
    private let signInButton = NSButton(title: "Sign in through browser", target: nil, action: nil)
    private let signOutButton = NSButton(title: "Sign out", target: nil, action: nil)
    private let registerButton = NSButton(title: "Register this Mac", target: nil, action: nil)
    private let refreshButton = NSButton()
    private let sharingToggle = NSButton(checkboxWithTitle: "Share terminal access from this Mac", target: nil, action: nil)
    private let sharingConsent = NSTextField(wrappingLabelWithString: "When enabled, other registered devices on this account can view output, resize existing terminals on this Mac to fit their viewer, and send keys or paste text. Input can run commands in your shell. Sharing stays off until you turn it on.")
    private let sharingStatus = NSTextField(wrappingLabelWithString: "Terminal sharing is off.")
    private var authentication: ASWebAuthenticationSession?
    private var operation: Task<Void, Never>?
    private var generation: UInt64 = 0
    private var busy = false
    private var signedIn = false
    private var localDeviceID: String?
    private var devicesByID: [String: CloudDevice] = [:]
    private var connectIDs: [NSButton: String] = [:]
    private var sharingObserver: UUID?

    init(owner: MainWindowHost) {
        self.appController = owner
        endpoint = owner.cloudEndpoint
        client = owner.cloudClient
        terminalHost = owner.cloudTerminalHost
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 520, height: 640),
                              styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        authenticationPresentationAnchor = window
        window.title = "zmux Cloud"
        window.isReleasedWhenClosed = false
        window.center()
        super.init(window: window)
        window.delegate = self
        buildContent()
    }
    override func showWindow(_ sender: Any?) {
        super.showWindow(sender)
        if sharingObserver == nil {
            sharingObserver = terminalHost.observeSharing { [weak self] in self?.renderSharingState($0) }
        }
        if authentication == nil { refresh(nil) }
    }
    required init?(coder: NSCoder) { nil }
    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        authenticationPresentationAnchor
    }
    func windowWillClose(_ notification: Notification) {
        generation &+= 1
        if let sharingObserver { terminalHost.removeSharingObserver(sharingObserver); self.sharingObserver = nil }
        authentication?.cancel(); authentication = nil
        operation?.cancel(); operation = nil
        setBusy(false)
    }
    private func buildContent() {
        guard let content = window?.contentView else { return }
        let stack = NSStackView()
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 16
        stack.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(stack)
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 28),
                                     stack.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -28),
                                     stack.topAnchor.constraint(equalTo: content.topAnchor, constant: 28)])
        let heading = NSStackView()
        heading.orientation = .horizontal; heading.spacing = 12
        let symbol = NSImageView(image: NSImage(systemSymbolName: "cloud", accessibilityDescription: "Cloud account")!)
        symbol.contentTintColor = .controlAccentColor
        symbol.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 28, weight: .regular)
        heading.addArrangedSubview(symbol)
        accountLabel.font = .systemFont(ofSize: 20, weight: .semibold)
        heading.addArrangedSubview(accountLabel)
        stack.addArrangedSubview(heading)
        detailLabel.font = .systemFont(ofSize: 13); detailLabel.textColor = .secondaryLabelColor
        stack.addArrangedSubview(detailLabel)
        let origin = NSTextField(labelWithString: endpoint.origin.host ?? endpoint.origin.absoluteString)
        origin.font = .monospacedSystemFont(ofSize: 11, weight: .regular); origin.textColor = .tertiaryLabelColor
        stack.addArrangedSubview(origin)
        let actions = NSStackView()
        actions.orientation = .horizontal; actions.spacing = 8
        signInButton.target = self; signInButton.action = #selector(signIn(_:)); signInButton.bezelStyle = .rounded
        signOutButton.target = self; signOutButton.action = #selector(signOut(_:)); signOutButton.bezelStyle = .rounded
        registerButton.target = self; registerButton.action = #selector(registerMac(_:)); registerButton.bezelStyle = .rounded
        refreshButton.image = NSImage(systemSymbolName: "arrow.clockwise", accessibilityDescription: "Refresh cloud account")
        refreshButton.imagePosition = .imageOnly; refreshButton.bezelStyle = .rounded
        refreshButton.toolTip = "Refresh cloud account"; refreshButton.setAccessibilityLabel("Refresh cloud account")
        refreshButton.target = self; refreshButton.action = #selector(refresh(_:))
        for button in [signInButton, registerButton, signOutButton, refreshButton] { actions.addArrangedSubview(button) }
        stack.addArrangedSubview(actions)
        let separator = NSBox(); separator.boxType = .separator
        stack.addArrangedSubview(separator); separator.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        sharingToggle.target = self; sharingToggle.action = #selector(toggleTerminalSharing(_:))
        sharingToggle.setAccessibilityLabel("Share terminal access from this Mac")
        stack.addArrangedSubview(sharingToggle)
        sharingConsent.font = .systemFont(ofSize: 12); sharingConsent.textColor = .secondaryLabelColor
        stack.addArrangedSubview(sharingConsent)
        sharingConsent.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        sharingStatus.font = .systemFont(ofSize: 12); sharingStatus.textColor = .secondaryLabelColor
        stack.addArrangedSubview(sharingStatus)
        sharingStatus.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        let deviceSeparator = NSBox(); deviceSeparator.boxType = .separator
        stack.addArrangedSubview(deviceSeparator); deviceSeparator.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        let title = NSTextField(labelWithString: "YOUR DEVICES")
        title.font = .systemFont(ofSize: 10, weight: .semibold); title.textColor = .secondaryLabelColor
        stack.addArrangedSubview(title)
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true; scroll.drawsBackground = false
        devices.orientation = .vertical; devices.alignment = .leading; devices.spacing = 12
        devices.translatesAutoresizingMaskIntoConstraints = false
        scroll.documentView = devices
        stack.addArrangedSubview(scroll)
        scroll.heightAnchor.constraint(equalToConstant: 175).isActive = true
        scroll.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        devices.widthAnchor.constraint(equalTo: scroll.contentView.widthAnchor).isActive = true
        message.font = .systemFont(ofSize: 12); message.textColor = .secondaryLabelColor
        stack.addArrangedSubview(message)
        message.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        setSignedIn(false)
    }
    private func setSignedIn(_ signedIn: Bool) {
        self.signedIn = signedIn
        signInButton.isHidden = signedIn
        signOutButton.isHidden = !signedIn
        registerButton.isHidden = !signedIn
        sharingToggle.isEnabled = signedIn && localDeviceID != nil && !busy
        sharingToggle.isHidden = !signedIn
        sharingConsent.isHidden = !signedIn
        sharingStatus.isHidden = !signedIn
    }
    private func setBusy(_ value: Bool) {
        busy = value
        for button in [signInButton, signOutButton, registerButton, refreshButton] { button.isEnabled = !value }
        sharingToggle.isEnabled = signedIn && localDeviceID != nil && !value
        for (button, id) in connectIDs { button.isEnabled = !value && canConnect(devicesByID[id]) }
        for button in deviceIDs.keys { button.isEnabled = !value }
    }
    private func begin(_ text: String) -> UInt64 {
        generation &+= 1
        operation?.cancel()
        message.stringValue = text
        setBusy(true)
        return generation
    }
    @objc private func refresh(_ sender: Any?) {
        let current = begin("Connecting…")
        operation = Task { [weak self, client] in
            do {
                let account = try await client.account()
                let localDevice = try await client.localDevice()
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.present(account, localDevice: localDevice)
            } catch {
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.presentFailure(error)
            }
        }
    }
    @objc private func signIn(_ sender: Any?) {
        guard !busy else { return }
        do {
            authentication?.cancel()
            let authorization = try CloudAuthorization(endpoint: endpoint)
            let current = begin("Complete sign-in in your browser…")
            // AuthenticationServices can complete on Safari's XPC queue. Keep this outer
            // callback nonisolated; an inherited MainActor check would trap before the hop.
            let session = ASWebAuthenticationSession(url: authorization.url, callbackURLScheme: "zmux-dev") { @Sendable [weak self] callback, error in
                Task { @MainActor [weak self] in
                    guard let self, self.generation == current else { return }
                    self.authentication = nil
                    guard let callback, error == nil else {
                        self.message.stringValue = error.map { "Unable to complete browser sign-in: " + $0.localizedDescription }
                            ?? "Sign-in ended without an authorization callback. Please try again."
                        self.setBusy(false); return
                    }
                    do {
                        let code = try authorization.code(from: callback)
                        self.operation = Task { [weak self, client = self.client] in
                            do {
                                let account = try await client.complete(code: code, verifier: authorization.verifier)
                                let localDevice = try await client.localDevice()
                                guard let self, self.generation == current, !Task.isCancelled else { return }
                                self.present(account, localDevice: localDevice)
                            } catch {
                                guard let self, self.generation == current, !Task.isCancelled else { return }
                                self.presentFailure(error)
                            }
                        }
                    } catch { self.presentFailure(error) }
                }
            }
            session.presentationContextProvider = self
            authentication = session
            if !session.start() { authentication = nil; message.stringValue = "Unable to start browser sign-in."; setBusy(false) }
        } catch { message.stringValue = error.localizedDescription; setBusy(false) }
    }
    @objc private func signOut(_ sender: Any?) {
        let current = begin("Signing out…")
        operation = Task { [weak self, client, terminalHost] in
            let sharingWarning = await terminalHost.stopForCredentialChange()
            let result: String
            do { result = try await client.signOut() }
            catch {
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.presentFailure(error)
                return
            }
            guard let self, self.generation == current, !Task.isCancelled else { return }
            self.setSignedIn(false); self.setBusy(false)
            self.localDeviceID = nil
            self.accountLabel.stringValue = "Connect your workspace"
            self.detailLabel.stringValue = "Sign in to register this Mac and manage your devices."
            self.devices.arrangedSubviews.forEach { self.devices.removeArrangedSubview($0); $0.removeFromSuperview() }
            self.deviceIDs.removeAll()
            self.connectIDs.removeAll()
            self.devicesByID.removeAll()
            self.message.stringValue = [sharingWarning, result].compactMap { $0 }.joined(separator: " ")
            self.appController?.updateCloudStatus("Sign in to Cloud", connected: false)
        }
    }
    @objc private func registerMac(_ sender: Any?) {
        let name = Host.current().localizedName ?? "Mac"
        let current = begin("Registering this Mac…")
        operation = Task { [weak self, client] in
            do {
                _ = try await client.registerDevice(name: name)
                let account = try await client.account()
                let localDevice = try await client.localDevice()
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.present(account, localDevice: localDevice)
            } catch {
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.presentFailure(error)
            }
        }
    }
    private var deviceIDs: [NSButton: String] = [:]
    @objc private func revokeDevice(_ sender: NSButton) {
        guard !busy, let id = deviceIDs[sender] else { return }
        let alert = NSAlert()
        alert.messageText = "Remove this device?"
        alert.informativeText = devicesByID[id]?.installationID != nil
            ? "This revokes the device's Cloud access. This build cannot restore a removed registration. Local terminals remain available."
            : "This removes its cloud registration. Local terminals remain available."
        alert.addButton(withTitle: "Remove"); alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let current = begin("Removing device…")
        let isLocal = id == localDeviceID
        operation = Task { [weak self, client, terminalHost] in
            do {
                let sharingWarning = isLocal ? await terminalHost.stopForCredentialChange() : nil
                try await client.revokeDevice(id: id)
                let account = try await client.account()
                let localDevice = try await client.localDevice()
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.present(account, localDevice: localDevice)
                if let sharingWarning { self.message.stringValue = "The device was removed. \(sharingWarning)" }
            } catch {
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.presentFailure(error)
            }
        }
    }
    private func presentFailure(_ error: any Error) {
        if error is CancellationError {
            setBusy(false)
            message.stringValue = ""
            return
        }
        message.stringValue = error.localizedDescription
        setBusy(false)
        if let failure = error as? CloudFailure, failure.requiresDeviceReauth {
            Task { [terminalHost] in _ = await terminalHost.stopForCredentialChange() }
        }
        if (error as? CloudFailure)?.requiresSignIn == true {
            Task { [terminalHost] in _ = await terminalHost.stopForCredentialChange() }
            setSignedIn(false)
            devices.arrangedSubviews.forEach { devices.removeArrangedSubview($0); $0.removeFromSuperview() }
            deviceIDs.removeAll()
            connectIDs.removeAll()
            devicesByID.removeAll()
            localDeviceID = nil
            accountLabel.stringValue = "Connect your workspace"
            detailLabel.stringValue = "Sign in to register this Mac and manage your devices."
            appController?.updateCloudStatus("Sign in to Cloud", connected: false)
        } else {
            appController?.updateCloudStatus("Cloud unavailable", connected: false)
        }
    }
    private func present(_ account: CloudAccount, localDevice: CloudDevice?) {
        setBusy(false)
        localDeviceID = localDevice?.id
        devicesByID = Dictionary(account.devices.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        setSignedIn(true)
        accountLabel.stringValue = account.user.name.isEmpty ? "Connected" : account.user.name
        detailLabel.stringValue = account.user.email
        appController?.updateCloudStatus(account.user.name.isEmpty ? "Cloud connected" : account.user.name, connected: true)
        devices.arrangedSubviews.forEach { devices.removeArrangedSubview($0); $0.removeFromSuperview() }
        deviceIDs.removeAll()
        connectIDs.removeAll()
        let active = account.devices.filter { $0.revokedAt == nil }
        for device in active {
            let row = NSStackView(); row.orientation = .horizontal; row.spacing = 10
            let image = NSImageView(image: NSImage(systemSymbolName: device.platform == "macos" ? "laptopcomputer" : "desktopcomputer", accessibilityDescription: device.platform)!)
            row.addArrangedSubview(image)
            let name = NSTextField(labelWithString: device.name); name.lineBreakMode = .byTruncatingTail
            row.addArrangedSubview(name)
            let local = device.id == localDeviceID
            let label = local ? "This Mac" : (device.installationID == nil ? "Earlier registration" : (device.online ? "Online" : "Offline"))
            let state = NSTextField(labelWithString: label)
            state.font = .systemFont(ofSize: 11); state.textColor = .secondaryLabelColor
            row.addArrangedSubview(state)
            if !local && canConnect(device) {
                let connect = NSButton(title: "Connect", target: self, action: #selector(connectDevice(_:)))
                connect.bezelStyle = .rounded; connect.setAccessibilityLabel("Connect to terminal sessions on \(device.name)")
                row.addArrangedSubview(connect); connectIDs[connect] = device.id
            }
            let remove = NSButton(image: NSImage(systemSymbolName: "minus.circle", accessibilityDescription: "Remove device")!, target: self, action: #selector(revokeDevice(_:)))
            remove.isBordered = false; remove.toolTip = "Remove \(device.name)"; remove.setAccessibilityLabel("Remove \(device.name)")
            row.addArrangedSubview(remove); deviceIDs[remove] = device.id
            devices.addArrangedSubview(row)
        }
        if active.isEmpty { devices.addArrangedSubview(NSTextField(labelWithString: "No devices registered yet.")) }
        renderSharingState(terminalHost.sharingState)
        message.stringValue = "Registered devices are listed separately, even when their names match."
        setBusy(false)
    }

    private func canConnect(_ device: CloudDevice?) -> Bool {
        guard let device else { return false }
        return device.revokedAt == nil && device.online && device.sharing && device.id != localDeviceID
    }

    private func renderSharingState(_ state: CloudTerminalSharingState) {
        sharingStatus.stringValue = state.message
        sharingToggle.state = state.isSharing ? .on : .off
        if case .starting = state { sharingToggle.state = .on }
        if case .stopping = state { sharingToggle.state = .off }
    }

    @objc private func toggleTerminalSharing(_ sender: NSButton) {
        guard signedIn, localDeviceID != nil, !busy else { return }
        let enabled = sender.state == .on
        let current = begin(enabled ? "Enabling terminal sharing…" : "Stopping terminal sharing…")
        operation = Task { [weak self, terminalHost] in
            do {
                try await terminalHost.setSharingEnabled(enabled)
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.setBusy(false)
                self.message.stringValue = ""
                self.renderSharingState(terminalHost.sharingState)
            } catch {
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.presentFailure(error)
                self.renderSharingState(terminalHost.sharingState)
            }
        }
    }

    @objc private func connectDevice(_ sender: NSButton) {
        guard !busy, let id = connectIDs[sender], let device = devicesByID[id], canConnect(device) else { return }
        let current = begin("Loading shared terminal sessions…")
        operation = Task { [weak self, terminalHost] in
            do {
                let sessions = try await terminalHost.listSessions(on: device)
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.setBusy(false)
                self.chooseSession(sessions, from: device)
            } catch {
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.presentFailure(error)
            }
        }
    }

    private func chooseSession(_ sessions: [CloudTerminalSessionDescriptor], from device: CloudDevice) {
        guard !sessions.isEmpty else {
            message.stringValue = "\(device.name) is sharing, but no existing terminal sessions are available."
            return
        }
        let alert = NSAlert()
        alert.messageText = "Choose a terminal on \(device.name)"
        alert.informativeText = "Opening the viewer resizes this terminal to fit its window. Keys and paste go to the selected terminal. The host tab keeps its current focus."
        alert.addButton(withTitle: "Connect")
        alert.addButton(withTitle: "Cancel")
        let picker = NSPopUpButton(frame: NSRect(x: 0, y: 0, width: 360, height: 26), pullsDown: false)
        for (index, session) in sessions.enumerated() {
            let title = session.title.isEmpty ? "Terminal \(session.scope.sessionId)" : session.title
            picker.addItem(withTitle: "\(index + 1). \(title)")
        }
        alert.accessoryView = picker
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let index = picker.indexOfSelectedItem
        guard sessions.indices.contains(index), let appController else { return }
        if appController.openCloudTerminal(sessions[index]) { message.stringValue = "Connected to \(device.name)." }
    }
}

@MainActor
extension MainWindowHost {
    @objc func installCommandLineTool(_ sender: Any?) {
        guard let resources = Bundle.main.resourceURL else { return }
        let executable = resources.appendingPathComponent("bin/zmux")
        let panel = NSOpenPanel()
        panel.title = "Install zmux Command Line Tool"
        panel.message = "Choose a writable directory on your PATH, such as ~/.local/bin. A symlink named zmux will point to this app's bundled CLI."
        panel.canChooseFiles = false; panel.canChooseDirectories = true; panel.canCreateDirectories = true
        panel.prompt = "Install"
        panel.beginSheetModal(for: window) { response in
            guard response == .OK, let directory = panel.url else { return }
            let target = directory.appendingPathComponent("zmux")
            do {
                // Never replace an existing executable or link without an explicit user action.
                try FileManager.default.createSymbolicLink(at: target, withDestinationURL: executable)
                let alert = NSAlert(); alert.messageText = "Command line tool installed"
                alert.informativeText = "Run zmux --help from a shell with \(directory.path) on PATH. Keep this app at its current location."
                alert.beginSheetModal(for: self.window)
            } catch {
                let alert = NSAlert(); alert.messageText = "Unable to install command line tool"
                alert.informativeText = error.localizedDescription
                alert.beginSheetModal(for: self.window)
            }
        }
    }
}

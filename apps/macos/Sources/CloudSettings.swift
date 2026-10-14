import AppKit
import AuthenticationServices

@MainActor
final class CloudWindowController: NSWindowController, NSWindowDelegate, ASWebAuthenticationPresentationContextProviding {
    private weak var owner: AppController?
    private let endpoint: CloudEndpoint
    private let client: CloudClient
    private let accountLabel = NSTextField(labelWithString: "Connect your workspace")
    private let detailLabel = NSTextField(wrappingLabelWithString: "Sign in to register this Mac and manage your devices.")
    private let message = NSTextField(wrappingLabelWithString: "")
    private let devices = NSStackView()
    private let signInButton = NSButton(title: "Sign in through browser", target: nil, action: nil)
    private let signOutButton = NSButton(title: "Sign out", target: nil, action: nil)
    private let registerButton = NSButton(title: "Register this Mac", target: nil, action: nil)
    private let refreshButton = NSButton()
    private var authentication: ASWebAuthenticationSession?
    private var operation: Task<Void, Never>?
    private var generation: UInt64 = 0
    private var busy = false

    init(owner: AppController) {
        self.owner = owner
        // Build validation supplies a valid owned origin. A malformed configuration fails visibly.
        do { endpoint = try CloudEndpoint(CloudEndpoint.configured) }
        catch { preconditionFailure("Invalid packaged cloud origin: \(error.localizedDescription)") }
        client = CloudClient(endpoint: endpoint)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 500, height: 540),
                              styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        window.title = "zmux Cloud"
        window.isReleasedWhenClosed = false
        window.center()
        super.init(window: window)
        window.delegate = self
        buildContent()
    }
    override func showWindow(_ sender: Any?) {
        super.showWindow(sender)
        if authentication == nil { refresh(nil) }
    }
    required init?(coder: NSCoder) { nil }
    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor { window! }
    func windowWillClose(_ notification: Notification) {
        generation &+= 1
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
        signInButton.isHidden = signedIn
        signOutButton.isHidden = !signedIn
        registerButton.isHidden = !signedIn
    }
    private func setBusy(_ value: Bool) {
        busy = value
        for button in [signInButton, signOutButton, registerButton, refreshButton] { button.isEnabled = !value }
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
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.present(account)
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
            let session = ASWebAuthenticationSession(url: authorization.url, callbackURLScheme: "zmux-dev") { [weak self] callback, error in
                Task { @MainActor [weak self] in
                    guard let self, self.generation == current else { return }
                    self.authentication = nil
                    guard let callback, error == nil else {
                        self.message.stringValue = "Sign-in was cancelled or could not open. Try again when cloud is available."
                        self.setBusy(false); return
                    }
                    do {
                        let code = try authorization.code(from: callback)
                        self.operation = Task { [weak self, client = self.client] in
                            do {
                                let account = try await client.complete(code: code, verifier: authorization.verifier)
                                guard let self, self.generation == current, !Task.isCancelled else { return }
                                self.present(account)
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
        operation = Task { [weak self, client] in
            let result: String
            do { result = try await client.signOut() }
            catch {
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.presentFailure(error)
                return
            }
            guard let self, self.generation == current, !Task.isCancelled else { return }
            self.setSignedIn(false); self.setBusy(false)
            self.accountLabel.stringValue = "Connect your workspace"
            self.detailLabel.stringValue = "Sign in to register this Mac and manage your devices."
            self.devices.arrangedSubviews.forEach { self.devices.removeArrangedSubview($0); $0.removeFromSuperview() }
            self.message.stringValue = result
            self.owner?.updateCloudStatus("Sign in to Cloud", connected: false)
        }
    }
    @objc private func registerMac(_ sender: Any?) {
        let name = Host.current().localizedName ?? "Mac"
        let current = begin("Registering this Mac…")
        operation = Task { [weak self, client] in
            do {
                try await client.registerDevice(name: name)
                let account = try await client.account()
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.present(account)
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
        alert.informativeText = "This removes its cloud registration. Local terminals remain available."
        alert.addButton(withTitle: "Remove"); alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let current = begin("Removing device…")
        operation = Task { [weak self, client] in
            do {
                try await client.revokeDevice(id: id)
                let account = try await client.account()
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.present(account)
            } catch {
                guard let self, self.generation == current, !Task.isCancelled else { return }
                self.presentFailure(error)
            }
        }
    }
    private func presentFailure(_ error: any Error) {
        message.stringValue = error.localizedDescription
        setBusy(false)
        if (error as? CloudFailure)?.requiresSignIn == true {
            setSignedIn(false)
            accountLabel.stringValue = "Connect your workspace"
            detailLabel.stringValue = "Sign in to register this Mac and manage your devices."
            owner?.updateCloudStatus("Sign in to Cloud", connected: false)
        } else {
            owner?.updateCloudStatus("Cloud unavailable", connected: false)
        }
    }
    private func present(_ account: CloudAccount) {
        setBusy(false); setSignedIn(true)
        accountLabel.stringValue = account.user.name.isEmpty ? "Connected" : account.user.name
        detailLabel.stringValue = account.user.email
        owner?.updateCloudStatus(account.user.name.isEmpty ? "Cloud connected" : account.user.name, connected: true)
        devices.arrangedSubviews.forEach { devices.removeArrangedSubview($0); $0.removeFromSuperview() }
        deviceIDs.removeAll()
        let active = account.devices.filter { $0.revokedAt == nil }
        for device in active {
            let row = NSStackView(); row.orientation = .horizontal; row.spacing = 10
            let image = NSImageView(image: NSImage(systemSymbolName: device.platform == "macos" ? "laptopcomputer" : "desktopcomputer", accessibilityDescription: device.platform)!)
            row.addArrangedSubview(image)
            let name = NSTextField(labelWithString: device.name); name.lineBreakMode = .byTruncatingTail
            row.addArrangedSubview(name)
            let remove = NSButton(image: NSImage(systemSymbolName: "minus.circle", accessibilityDescription: "Remove device")!, target: self, action: #selector(revokeDevice(_:)))
            remove.isBordered = false; remove.toolTip = "Remove \(device.name)"; remove.setAccessibilityLabel("Remove \(device.name)")
            row.addArrangedSubview(remove); deviceIDs[remove] = device.id
            devices.addArrangedSubview(row)
        }
        if active.isEmpty { devices.addArrangedSubview(NSTextField(labelWithString: "No devices registered yet.")) }
        message.stringValue = "Account and device discovery are connected. Remote terminal transport is not available in this build."
    }
}

@MainActor
extension AppController {
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

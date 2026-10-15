import AppKit

/// Presentation of the Rust-owned installation catalog. No website storage or
/// filesystem operation is performed here; pending work remains one exact receipt.
@MainActor
final class BrowserProfilesWindowController: NSWindowController {
    private let createProfile: @MainActor (String) async throws -> BrowserProfileReceipt
    private let openProfile: @MainActor (UUID) throws -> Void
    private let profiles = NSPopUpButton(frame: .zero, pullsDown: false)
    private let name = NSTextField(string: "")
    private let message = NSTextField(wrappingLabelWithString: "")
    private let createButton = NSButton(title: "Create Profile", target: nil, action: nil)
    private let openButton = NSButton(title: "Open Profile", target: nil, action: nil)
    private var state: BrowserProfileState?
    private var selectedID: UUID?
    private var renderedProfiles: [BrowserProfile] = []
    private var renderedAvailable: Bool?
    private var creating = false
    private var feedback: String?
    private var operation: Task<Void, Never>?

    init(create: @escaping @MainActor (String) async throws -> BrowserProfileReceipt,
         open: @escaping @MainActor (UUID) throws -> Void) {
        createProfile = create
        openProfile = open
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 480, height: 400),
                              styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Browser Profiles"
        window.isReleasedWhenClosed = false
        window.center()
        super.init(window: window)
        buildContent()
        render()
    }
    required init?(coder: NSCoder) { nil }

    func update(_ state: BrowserProfileState) {
        let wasAvailable = available
        self.state = state
        if !wasAvailable && available && !creating { message.stringValue = "" }
        render()
    }

    private var available: Bool {
        state?.enabled == true && state?.loading == false && state?.error == nil
    }
    private func buildContent() {
        guard let content = window?.contentView else { return }
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 14
        stack.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 24),
            stack.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -24),
            stack.topAnchor.constraint(equalTo: content.topAnchor, constant: 24)
        ])
        let detail = NSTextField(wrappingLabelWithString: "Keep cookies and website data separate for each browser profile on this Mac.")
        detail.textColor = .secondaryLabelColor
        stack.addArrangedSubview(detail)
        profiles.target = self
        profiles.action = #selector(selectionChanged(_:))
        profiles.setAccessibilityLabel("Browser profile")
        stack.addArrangedSubview(profiles)
        openButton.target = self
        openButton.action = #selector(openSelected(_:))
        openButton.bezelStyle = .rounded
        stack.addArrangedSubview(openButton)
        let separator = NSBox()
        separator.boxType = .separator
        stack.addArrangedSubview(separator)
        name.placeholderString = "New profile name"
        name.setAccessibilityLabel("New profile name")
        stack.addArrangedSubview(name)
        createButton.target = self
        createButton.action = #selector(create(_:))
        createButton.bezelStyle = .rounded
        stack.addArrangedSubview(createButton)
        message.setAccessibilityLabel("Profile status")
        message.textColor = .secondaryLabelColor
        message.font = .systemFont(ofSize: 12)
        stack.addArrangedSubview(message)
        let fullWidth: [NSView] = [detail, profiles, separator, name, message]
        for view in fullWidth {
            view.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        }
    }

    private func render() {
        // Preserve immutable selection across refreshes; a missing ID must never
        // silently select a different profile at the same list position.
        let catalog = available ? state?.profiles ?? [] : []
        if renderedProfiles != catalog || renderedAvailable != available {
            profiles.removeAllItems()
            profiles.addItem(withTitle: "Choose a profile")
            for profile in catalog {
                let item = NSMenuItem(title: profile.name, action: nil, keyEquivalent: "")
                item.representedObject = profile.id.uuidString
                profiles.menu?.addItem(item)
            }
            renderedProfiles = catalog
            renderedAvailable = available
        }
        if let selectedID, let item = profiles.itemArray.first(where: {
            ($0.representedObject as? String) == selectedID.uuidString
        }) {
            profiles.select(item)
        } else {
            selectedID = nil
            profiles.selectItem(at: 0)
        }
        profiles.isEnabled = available && !creating
        openButton.isEnabled = available && selectedID != nil && !creating
        name.isEnabled = available && !creating && (state?.profiles.count ?? 32) < 32
        createButton.isEnabled = name.isEnabled
        if creating { return }
        if let feedback { message.stringValue = feedback }
        else if state?.loading == true { message.stringValue = "Loading profiles…" }
        else if let error = state?.error { message.stringValue = error }
        else if state?.enabled != true { message.stringValue = "Profiles are unavailable for this session." }
        else if (state?.profiles.count ?? 0) >= 32 { message.stringValue = "The profile limit of 32 has been reached." }
        else { message.stringValue = catalog.isEmpty ? "Create a profile to get started." : "" }
    }

    @objc private func selectionChanged(_ sender: NSPopUpButton) {
        selectedID = (sender.selectedItem?.representedObject as? String).flatMap(UUID.init(uuidString:))
        feedback = nil
        render()
    }
    @objc private func openSelected(_ sender: NSButton) {
        guard available, !creating, let selectedID,
              state?.profiles.contains(where: { $0.id == selectedID }) == true else {
            message.stringValue = "Choose an available profile first."
            return
        }
        do { try openProfile(selectedID) }
        catch { feedback = error.localizedDescription; render() }
    }
    @objc private func create(_ sender: NSButton) {
        guard available, !creating else { return }
        let submittedName = name.stringValue
        // Rust owns validation, including UTF-8 bounds and duplicate names.
        creating = true
        feedback = nil
        message.stringValue = "Creating profile…"
        render()
        let deadline = ContinuousClock.now + .seconds(20)
        operation = Task { [weak self] in
            guard let self else { return }
            defer { self.creating = false; self.operation = nil; self.render() }
            do {
                let admission = try await self.createProfile(submittedName)
                while ContinuousClock.now < deadline {
                    try Task.checkCancellation()
                    guard self.state?.epoch == admission.epoch else {
                        throw BridgeFailure(message: "The profile runtime changed. Check the profile list before trying again.")
                    }
                    if let receipt = self.state?.operations.first(where: {
                        $0.epoch == admission.epoch && $0.operationId == admission.operationId
                    }) {
                        switch receipt.state {
                        case "completed":
                            guard let profile = receipt.profile,
                                  self.state?.profiles.contains(where: { $0.id == profile.id && $0.name == profile.name }) == true else {
                                throw BridgeFailure(message: "The completed profile is unavailable. Check the profile list before trying again.")
                            }
                            self.selectedID = profile.id
                            self.name.stringValue = ""
                            self.feedback = "Profile created."
                            return
                        case "failed":
                            throw BridgeFailure(message: receipt.error ?? "Profile creation failed.")
                        case "pending": break
                        default:
                            throw BridgeFailure(message: "The profile receipt could not be read. Check the profile list before trying again.")
                        }
                    }
                    try await Task.sleep(for: .milliseconds(50))
                }
                throw BridgeFailure(message: "Profile creation timed out. Check the profile list before trying again; creation was not replayed.")
            } catch {
                self.feedback = error.localizedDescription
            }
        }
    }
}

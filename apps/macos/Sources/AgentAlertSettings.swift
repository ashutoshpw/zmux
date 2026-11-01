import AppKit

@MainActor
final class AgentAlertSettingsWindowController: NSWindowController, NSWindowDelegate {
    private let delivery: NotificationDelivery
    private let soundSwitch = NSSwitch()
    private let statusLabel = NSTextField(wrappingLabelWithString: "")
    private var sounds: [AgentAlertKind: NSPopUpButton] = [:]
    private var previewButtons: [NSButton: AgentAlertKind] = [:]

    init(delivery: NotificationDelivery) {
        self.delivery = delivery
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 620, height: 390),
                              styleMask: [.titled, .closable, .miniaturizable],
                              backing: .buffered, defer: false)
        window.title = "Alerts"
        window.isReleasedWhenClosed = false
        window.center()
        super.init(window: window)
        window.delegate = self
        buildContent()
        render()
    }

    required init?(coder: NSCoder) { nil }

    private func buildContent() {
        guard let content = window?.contentView else { return }
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 18
        stack.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 30),
            stack.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -30),
            stack.topAnchor.constraint(equalTo: content.topAnchor, constant: 26)
        ])

        let section = NSTextField(labelWithString: "SOUNDS")
        section.font = .systemFont(ofSize: 11, weight: .semibold)
        section.textColor = .secondaryLabelColor
        stack.addArrangedSubview(section)

        let master = NSStackView()
        master.orientation = .horizontal
        master.alignment = .centerY
        master.distribution = .fill
        master.spacing = 16
        let masterText = NSStackView()
        masterText.orientation = .vertical
        masterText.alignment = .leading
        masterText.spacing = 5
        let masterTitle = NSTextField(labelWithString: "Play sounds")
        masterTitle.font = .systemFont(ofSize: 14, weight: .semibold)
        let explanation = NSTextField(wrappingLabelWithString: "A sound plays when an agent needs you or finishes a run. Visual alerts remain available when sounds are off.")
        explanation.font = .systemFont(ofSize: 12)
        explanation.textColor = .secondaryLabelColor
        masterText.addArrangedSubview(masterTitle)
        masterText.addArrangedSubview(explanation)
        masterText.setContentHuggingPriority(.defaultLow, for: .horizontal)
        master.addArrangedSubview(masterText)
        soundSwitch.target = self
        soundSwitch.action = #selector(toggleSounds(_:))
        soundSwitch.setAccessibilityLabel("Play agent alert sounds")
        master.addArrangedSubview(soundSwitch)
        stack.addArrangedSubview(master)
        master.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true

        let separator = NSBox()
        separator.boxType = .separator
        stack.addArrangedSubview(separator)
        separator.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true

        for kind in AgentAlertKind.allCases {
            let row = soundRow(kind)
            stack.addArrangedSubview(row)
            row.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        }

        let settingsNote = NSTextField(wrappingLabelWithString: "Sound choices are stored locally in Zmux and do not change provider settings.")
        settingsNote.font = .systemFont(ofSize: 12)
        settingsNote.textColor = .secondaryLabelColor
        stack.addArrangedSubview(settingsNote)
        settingsNote.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true

        statusLabel.font = .systemFont(ofSize: 12)
        statusLabel.textColor = .secondaryLabelColor
        statusLabel.setAccessibilityLabel("Alert sound availability")
        stack.addArrangedSubview(statusLabel)
        statusLabel.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
    }

    private func soundRow(_ kind: AgentAlertKind) -> NSView {
        let row = NSStackView()
        row.orientation = .horizontal
        row.alignment = .centerY
        row.spacing = 12

        let labels = NSStackView()
        labels.orientation = .vertical
        labels.alignment = .leading
        labels.spacing = 4
        let title = NSTextField(labelWithString: kind.title)
        title.font = .systemFont(ofSize: 13, weight: .semibold)
        let detail = NSTextField(labelWithString: "Built-in macOS sound")
        detail.font = .systemFont(ofSize: 11)
        detail.textColor = .secondaryLabelColor
        labels.addArrangedSubview(title)
        labels.addArrangedSubview(detail)
        labels.setContentHuggingPriority(.defaultLow, for: .horizontal)
        row.addArrangedSubview(labels)

        let popup = NSPopUpButton(frame: .zero, pullsDown: false)
        for (index, sound) in AgentAlertSound.allCases.enumerated() {
            popup.addItem(withTitle: sound.title)
            popup.lastItem?.tag = index
        }
        popup.target = self
        popup.action = #selector(selectSound(_:))
        popup.setAccessibilityLabel("Sound for \(kind.title.lowercased())")
        popup.tag = kind == .needsUser ? 0 : 1
        popup.widthAnchor.constraint(greaterThanOrEqualToConstant: 120).isActive = true
        sounds[kind] = popup
        row.addArrangedSubview(popup)

        let preview = NSButton(title: "Play", target: self, action: #selector(previewSound(_:)))
        preview.bezelStyle = .rounded
        preview.setAccessibilityLabel("Preview sound for \(kind.title.lowercased())")
        preview.tag = kind == .needsUser ? 0 : 1
        previewButtons[preview] = kind
        row.addArrangedSubview(preview)
        return row
    }

    private func render() {
        let preferences = delivery.agentAlertPreferences
        soundSwitch.state = preferences.playSounds ? .on : .off
        for (kind, popup) in sounds {
            let selected = preferences.sound(for: kind)
            if let index = AgentAlertSound.allCases.firstIndex(of: selected) { popup.selectItem(at: index) }
        }
        renderAvailability()
    }

    private func renderAvailability(_ preview: AgentAlertPlaybackResult? = nil) {
        if let preview {
            switch preview {
            case .requested:
                statusLabel.stringValue = "macOS accepted the sound request. Zmux cannot confirm that it was audible."
            case .suppressed:
                statusLabel.stringValue = "Sounds are off. " + visualAvailabilityDescription
            case .unavailable(let reason):
                statusLabel.stringValue = reason + ". " + visualAvailabilityDescription
            }
            return
        }

        if !delivery.agentAlertPreferences.playSounds {
            statusLabel.stringValue = "Sounds are off. " + visualAvailabilityDescription
        } else {
            var messages: [String] = []
            if let issue = AgentAlertKind.allCases.compactMap({ delivery.agentAlertAvailability(for: $0) }).first {
                messages.append(issue)
            } else {
                messages.append("Playback follows the macOS output volume; Zmux cannot confirm that a sound was heard.")
            }
            if delivery.authorizationForSettings == .denied {
                messages.append("System notifications are unavailable until enabled in macOS settings.")
            }
            messages.append(visualAvailabilityDescription)
            statusLabel.stringValue = messages.joined(separator: " ")
        }
    }

    private var visualAvailabilityDescription: String {
        if delivery.authorizationForSettings == .denied {
            return "In-app alerts remain available; system notifications are disabled."
        } else {
            return "In-app alerts remain available; eligible system notifications follow macOS settings."
        }
    }

    @objc private func toggleSounds(_ sender: NSSwitch) {
        delivery.agentAlertPreferences.playSounds = sender.state == .on
        renderAvailability()
    }

    @objc private func selectSound(_ sender: NSPopUpButton) {
        let kind: AgentAlertKind = sender.tag == 0 ? .needsUser : .completed
        guard let index = sender.selectedItem?.tag,
              AgentAlertSound.allCases.indices.contains(index) else { return }
        let sound = AgentAlertSound.allCases[index]
        switch kind {
        case .needsUser: delivery.agentAlertPreferences.needsUserSound = sound
        case .completed: delivery.agentAlertPreferences.completedSound = sound
        }
        renderAvailability()
    }

    @objc private func previewSound(_ sender: NSButton) {
        guard let kind = previewButtons[sender] else { return }
        renderAvailability(delivery.previewAgentAlertSound(kind))
    }
}

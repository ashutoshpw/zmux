import AppKit

enum AgentSidebarSection: String, CaseIterable, Sendable, Hashable {
    case needsAttention
    case working
    case idle
    case unknown
    case disconnected

    init(record: AgentLifecycleRecord) {
        if record.connection == "disconnected" {
            self = .disconnected
        } else if record.connection != "connected" {
            self = .unknown
        } else {
            switch record.activity {
            case "waiting": self = .needsAttention
            case "working": self = .working
            case "idle": self = .idle
            default: self = .unknown
            }
        }
    }

    var title: String {
        switch self {
        case .needsAttention: return "Needs attention"
        case .working: return "Working"
        case .idle: return "Idle"
        case .unknown: return "Unknown"
        case .disconnected: return "Disconnected"
        }
    }

    var symbol: String {
        switch self {
        case .needsAttention: return "exclamationmark.circle.fill"
        case .working: return "circle.dashed"
        case .idle: return "circle"
        case .unknown: return "questionmark.circle"
        case .disconnected: return "wifi.slash"
        }
    }

    @MainActor var color: NSColor {
        switch self {
        case .needsAttention: return .systemOrange
        case .working: return ZmuxPalette.success
        case .idle, .unknown, .disconnected: return ZmuxPalette.inactive
        }
    }

    static var displayOrder: [Self] { [.needsAttention, .working, .idle, .unknown, .disconnected] }
}

struct AgentSidebarPresentation: Sendable, Hashable {
    let record: AgentLifecycleRecord
    let workspaceName: String
    let surfaceTitle: String
    let surfaceContext: String
    let isSelected: Bool

    var identity: AgentSidebarIdentity { AgentSidebarIdentity(record) }
    var section: AgentSidebarSection { AgentSidebarSection(record: record) }
    var providerName: String {
        switch record.provider {
        case "claude_local", "claude_remote": return "Claude Code"
        case "codex_local", "codex_remote": return "Codex"
        case "opencode_local", "opencode_remote": return "OpenCode"
        default: return record.provider.replacingOccurrences(of: "_", with: " ").capitalized
        }
    }

    var title: String {
        let value = surfaceTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        return value.isEmpty || value == "Terminal" ? surfaceContext : value
    }

    var subtitle: String {
        let value = surfaceTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        return value.isEmpty || value == "Terminal"
            ? "\(workspaceName) · \(providerName)"
            : "\(providerName) · \(workspaceName) · \(surfaceContext)"
    }

    var status: String {
        if record.connection == "disconnected" { return "Disconnected" }
        guard record.connection == "connected" else { return "Connection unknown" }
        switch record.activity {
        case "waiting": return "Waiting for you"
        case "working": return "Working"
        case "idle": return "Idle"
        default: return "Activity unknown"
        }
    }
}

struct AgentSidebarIdentity: Hashable, Sendable {
    let provider: String
    let identityNamespace: UUID
    let windowId: UInt64
    let windowUuid: UUID
    let workspaceId: UInt64
    let workspaceUuid: UUID
    let paneId: UInt64
    let paneUuid: UUID
    let surfaceId: UInt64
    let surfaceUuid: UUID
    let sessionId: UInt64
    let runtimeId: UUID
    let generation: UUID
    let providerSessionId: UUID

    init(_ record: AgentLifecycleRecord) {
        provider = record.provider
        identityNamespace = record.identityNamespace
        windowId = record.windowId
        windowUuid = record.windowUuid
        workspaceId = record.workspaceId
        workspaceUuid = record.workspaceUuid
        paneId = record.paneId
        paneUuid = record.paneUuid
        surfaceId = record.surfaceId
        surfaceUuid = record.surfaceUuid
        sessionId = record.sessionId
        runtimeId = record.runtimeId
        generation = record.generation
        providerSessionId = record.providerSessionId
    }
}

@MainActor
final class SidebarSectionHeaderView: NSView {
    private let titleLabel = NSTextField(labelWithString: "")

    init(title: String, trailingView: NSView? = nil) {
        super.init(frame: .zero)
        titleLabel.stringValue = title
        titleLabel.font = .systemFont(ofSize: 11.5, weight: .medium)
        titleLabel.textColor = ZmuxPalette.subdued
        titleLabel.lineBreakMode = .byTruncatingTail
        titleLabel.translatesAutoresizingMaskIntoConstraints = false
        addSubview(titleLabel)
        if let trailingView {
            trailingView.translatesAutoresizingMaskIntoConstraints = false
            addSubview(trailingView)
            NSLayoutConstraint.activate([
                titleLabel.trailingAnchor.constraint(lessThanOrEqualTo: trailingView.leadingAnchor, constant: -4),
                trailingView.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
                trailingView.centerYAnchor.constraint(equalTo: centerYAnchor)
            ])
        } else {
            titleLabel.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -8).isActive = true
        }
        titleLabel.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 9).isActive = true
        titleLabel.centerYAnchor.constraint(equalTo: centerYAnchor).isActive = true
        heightAnchor.constraint(equalToConstant: 28).isActive = true
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel(title)
        setAccessibilityChildren(trailingView.map { [titleLabel, $0] } ?? [titleLabel])
    }

    required init?(coder: NSCoder) { nil }
}

@MainActor
final class AgentSidebarList: FlippedStackView {
    private enum FocusedElement {
        case row(AgentSidebarIdentity, layoutIndex: Int)
        case header(AgentSidebarSection, layoutIndex: Int)
        case other
    }

    private var presentations: [AgentSidebarPresentation]?
    private var statusAvailable = true
    private var collapsedSections: Set<AgentSidebarSection> = []
    private var rowViewsByIdentity: [AgentSidebarIdentity: AgentSidebarRowView] = [:]
    private var headerViewsBySection: [AgentSidebarSection: AgentSidebarGroupHeaderView] = [:]
    private var widthConstraints: [ObjectIdentifier: NSLayoutConstraint] = [:]
    private var rowViews: [AgentSidebarRowView] = []
    var onSelect: (@MainActor (AgentLifecycleRecord) -> Void)?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        orientation = .vertical
        alignment = .trailing
        spacing = 2
        edgeInsets = NSEdgeInsets(top: 4, left: 8, bottom: 10, right: 8)
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel("Live agents")
        setAccessibilityHelp("Use Up and Down Arrow to move between agents. Press Return or Space to open the selected agent's existing terminal.")
        let loadingState = makeEmptyState(title: "Loading agents…", detail: "Waiting for live agent status.")
        addArrangedSubview(loadingState)
        widthConstraints[ObjectIdentifier(loadingState)]?.isActive = true
        setAccessibilityChildren(arrangedSubviews)
        setAccessibilityValue("Loading")
    }

    required init?(coder: NSCoder) { nil }

    func update(_ newPresentations: [AgentSidebarPresentation]?, statusAvailable: Bool) {
        guard presentations != newPresentations || self.statusAvailable != statusAvailable else { return }
        presentations = newPresentations
        self.statusAvailable = statusAvailable
        reconcile()
    }

    private func reconcile() {
        let focusedElement = captureFocusedElement()
        var desiredViews: [NSView] = []
        rowViews.removeAll(keepingCapacity: true)

        guard statusAvailable, let presentations else {
            desiredViews = [makeEmptyState(title: "Agent status unavailable",
                                           detail: "This desktop has not published live agent status.")]
            setAccessibilityValue("Agent status unavailable")
            reconcileViews(desiredViews)
            pruneViews(keeping: [], sections: [])
            window?.recalculateKeyViewLoop()
            restoreFocus(focusedElement)
            return
        }
        guard !presentations.isEmpty else {
            desiredViews = [makeEmptyState(title: "No live agents",
                                           detail: "Enable a supported Claude Code lifecycle adapter to show agent sessions here.")]
            setAccessibilityValue("No live agents")
            reconcileViews(desiredViews)
            pruneViews(keeping: presentations, sections: [])
            window?.recalculateKeyViewLoop()
            restoreFocus(focusedElement)
            return
        }

        for section in AgentSidebarSection.displayOrder {
            let sectionRows = presentations.filter { $0.section == section }
            guard !sectionRows.isEmpty else { continue }
            let header = headerViewsBySection[section] ?? makeHeader(for: section)
            header.update(count: sectionRows.count, collapsed: collapsedSections.contains(section))
            desiredViews.append(header)
            guard !collapsedSections.contains(section) else { continue }

            for presentation in sectionRows {
                let row = rowViewsByIdentity[presentation.identity] ?? makeRow(for: presentation)
                row.update(presentation)
                rowViews.append(row)
                desiredViews.append(row)
            }
        }

        reconcileViews(desiredViews)
        pruneViews(keeping: presentations, sections: Set(presentations.map(\.section)))
        setAccessibilityChildren(arrangedSubviews)
        setAccessibilityValue("\(presentations.count) live agents")
        window?.recalculateKeyViewLoop()
        restoreFocus(focusedElement)
    }

    private func makeEmptyState(title: String, detail: String) -> NSView {
        let container = NSStackView()
        container.orientation = .vertical
        container.alignment = .leading
        container.spacing = 3
        container.edgeInsets = NSEdgeInsets(top: 7, left: 1, bottom: 5, right: 1)
        let titleLabel = NSTextField(labelWithString: title)
        titleLabel.font = .systemFont(ofSize: 11.5, weight: .medium)
        titleLabel.textColor = ZmuxPalette.subdued
        let detailLabel = NSTextField(wrappingLabelWithString: detail)
        detailLabel.font = .systemFont(ofSize: 10.5)
        detailLabel.textColor = ZmuxPalette.inactive
        container.addArrangedSubview(titleLabel)
        container.addArrangedSubview(detailLabel)
        widthConstraints[ObjectIdentifier(container)] = container.widthAnchor.constraint(equalTo: widthAnchor, constant: -24)
        return container
    }

    private func makeHeader(for section: AgentSidebarSection) -> AgentSidebarGroupHeaderView {
        let header = AgentSidebarGroupHeaderView(section: section, count: 0, collapsed: false)
        header.onToggle = { [weak self] section in self?.toggle(section: section) }
        headerViewsBySection[section] = header
        widthConstraints[ObjectIdentifier(header)] = header.widthAnchor.constraint(equalTo: widthAnchor, constant: -16)
        return header
    }

    private func makeRow(for presentation: AgentSidebarPresentation) -> AgentSidebarRowView {
        let row = AgentSidebarRowView(presentation: presentation)
        row.onSelect = { [weak self] record in self?.onSelect?(record) }
        row.onMoveFocus = { [weak self, weak row] offset in
            guard let self, let row else { return }
            self.focus(row: row, offset: offset)
        }
        rowViewsByIdentity[presentation.identity] = row
        widthConstraints[ObjectIdentifier(row)] = row.widthAnchor.constraint(equalTo: widthAnchor, constant: -16)
        return row
    }

    private func toggle(section: AgentSidebarSection) {
        if collapsedSections.contains(section) {
            collapsedSections.remove(section)
        } else {
            collapsedSections.insert(section)
        }
        reconcile()
    }

    private func reconcileViews(_ desiredViews: [NSView]) {
        let desiredIDs = Set(desiredViews.map(ObjectIdentifier.init))
        for view in arrangedSubviews where !desiredIDs.contains(ObjectIdentifier(view)) {
            removeArrangedSubview(view)
            widthConstraints[ObjectIdentifier(view)]?.isActive = false
            view.removeFromSuperview()
            if view is NSStackView {
                removeWidthConstraint(for: view)
            }
        }

        for (index, view) in desiredViews.enumerated() {
            if index < arrangedSubviews.count, arrangedSubviews[index] === view { continue }
            if arrangedSubviews.contains(where: { $0 === view }) {
                removeArrangedSubview(view)
            }
            insertArrangedSubview(view, at: min(index, arrangedSubviews.count))
            if let constraint = widthConstraints[ObjectIdentifier(view)], !constraint.isActive {
                constraint.isActive = true
            }
        }

        setAccessibilityChildren(arrangedSubviews)
    }

    private func pruneViews(keeping presentations: [AgentSidebarPresentation], sections: Set<AgentSidebarSection>) {
        let currentIdentities = Set(presentations.map(\.identity))
        for identity in Array(rowViewsByIdentity.keys) where !currentIdentities.contains(identity) {
            if let row = rowViewsByIdentity.removeValue(forKey: identity) {
                removeWidthConstraint(for: row)
            }
        }
        for section in Array(headerViewsBySection.keys) where !sections.contains(section) {
            if let header = headerViewsBySection.removeValue(forKey: section) {
                removeWidthConstraint(for: header)
            }
        }
    }

    private func removeWidthConstraint(for view: NSView) {
        if let constraint = widthConstraints.removeValue(forKey: ObjectIdentifier(view)) {
            constraint.isActive = false
        }
    }

    private func captureFocusedElement() -> FocusedElement {
        guard let focusedView = window?.firstResponder as? NSView,
              let layoutIndex = arrangedSubviews.firstIndex(where: { $0 === focusedView }) else { return .other }
        if let row = focusedView as? AgentSidebarRowView {
            return .row(row.identity, layoutIndex: layoutIndex)
        }
        if let header = focusedView as? AgentSidebarGroupHeaderView {
            return .header(header.section, layoutIndex: layoutIndex)
        }
        return .other
    }

    private func restoreFocus(_ focusedElement: FocusedElement) {
        switch focusedElement {
        case .other:
            return
        case .header(let section, let layoutIndex):
            if let header = headerViewsBySection[section], arrangedSubviews.contains(where: { $0 === header }) {
                focus(header)
            } else {
                focusNearest(to: layoutIndex)
            }
        case .row(let identity, let layoutIndex):
            if let presentation = presentations?.first(where: { $0.identity == identity }),
               collapsedSections.contains(presentation.section),
               let header = headerViewsBySection[presentation.section] {
                focus(header)
            } else if let row = rowViewsByIdentity[identity], arrangedSubviews.contains(where: { $0 === row }) {
                focus(row)
            } else {
                focusNearest(to: layoutIndex)
            }
        }
    }

    private func focus(_ view: NSView) {
        guard window?.makeFirstResponder(view) == true else { return }
        view.scrollToVisible(view.bounds)
    }

    private func focusNearest(to layoutIndex: Int) {
        let focusableViews = arrangedSubviews.filter { $0 is AgentSidebarRowView || $0 is AgentSidebarGroupHeaderView }
        guard let target = focusableViews.min(by: { lhs, rhs in
            let lhsIndex = arrangedSubviews.firstIndex(where: { $0 === lhs }) ?? 0
            let rhsIndex = arrangedSubviews.firstIndex(where: { $0 === rhs }) ?? 0
            return abs(lhsIndex - layoutIndex) < abs(rhsIndex - layoutIndex)
        }) else {
            window?.makeFirstResponder(nil)
            return
        }
        focus(target)
    }

    private func focus(row: AgentSidebarRowView, offset: Int) {
        guard let index = rowViews.firstIndex(of: row), !rowViews.isEmpty else { return }
        let next = min(rowViews.count - 1, max(0, index + offset))
        focus(rowViews[next])
    }
}

@MainActor
private final class AgentSidebarGroupHeaderView: NSView {
    fileprivate let section: AgentSidebarSection
    private var isCollapsed: Bool
    private let titleLabel = NSTextField(labelWithString: "")
    private let countLabel = NSTextField(labelWithString: "")
    private let disclosure = NSImageView()
    var onToggle: (@MainActor (AgentSidebarSection) -> Void)?

    init(section: AgentSidebarSection, count: Int, collapsed: Bool) {
        self.section = section
        isCollapsed = collapsed
        super.init(frame: .zero)
        wantsLayer = true
        titleLabel.stringValue = section.title
        titleLabel.font = .systemFont(ofSize: 10.5, weight: .semibold)
        titleLabel.textColor = ZmuxPalette.subdued
        countLabel.stringValue = String(count)
        countLabel.font = .systemFont(ofSize: 10, weight: .regular)
        countLabel.textColor = ZmuxPalette.inactive
        disclosure.contentTintColor = ZmuxPalette.inactive
        disclosure.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 8, weight: .semibold)
        for view in [disclosure, titleLabel, countLabel] { view.translatesAutoresizingMaskIntoConstraints = false; addSubview(view) }
        NSLayoutConstraint.activate([
            disclosure.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 3),
            disclosure.centerYAnchor.constraint(equalTo: centerYAnchor),
            disclosure.widthAnchor.constraint(equalToConstant: 9),
            disclosure.heightAnchor.constraint(equalToConstant: 12),
            titleLabel.leadingAnchor.constraint(equalTo: disclosure.trailingAnchor, constant: 5),
            titleLabel.centerYAnchor.constraint(equalTo: centerYAnchor),
            countLabel.leadingAnchor.constraint(greaterThanOrEqualTo: titleLabel.trailingAnchor, constant: 5),
            countLabel.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -5),
            countLabel.centerYAnchor.constraint(equalTo: centerYAnchor)
        ])
        heightAnchor.constraint(equalToConstant: 24).isActive = true
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        updateAccessibility(count: count)
        setAccessibilityHelp("Press Return or Space to collapse or expand this status group.")
    }

    func update(count: Int, collapsed: Bool) {
        isCollapsed = collapsed
        countLabel.stringValue = String(count)
        disclosure.image = NSImage(systemSymbolName: collapsed ? "chevron.right" : "chevron.down", accessibilityDescription: nil)
        updateAccessibility(count: count)
    }

    required init?(coder: NSCoder) { nil }
    override var acceptsFirstResponder: Bool { true }
    override var canBecomeKeyView: Bool { window != nil && !isHiddenOrHasHiddenAncestor }
    override func becomeFirstResponder() -> Bool { needsDisplay = true; return true }
    override func resignFirstResponder() -> Bool { needsDisplay = true; return true }
    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        toggle()
    }
    override func keyDown(with event: NSEvent) {
        if event.keyCode == 36 || event.keyCode == 49 { toggle() }
        else { super.keyDown(with: event) }
    }
    override func accessibilityPerformPress() -> Bool { toggle(); return true }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard window?.firstResponder === self else { return }
        NSGraphicsContext.saveGraphicsState()
        NSFocusRingPlacement.only.set()
        bounds.insetBy(dx: 1, dy: 1).fill()
        NSGraphicsContext.restoreGraphicsState()
    }

    private func toggle() {
        onToggle?(section)
    }

    private func updateAccessibility(count: Int) {
        setAccessibilityLabel("\(section.title), \(count) agents")
        setAccessibilityValue(isCollapsed ? "Collapsed" : "Expanded")
    }
}

@MainActor
private final class AgentSidebarRowView: BackgroundView {
    private var presentation: AgentSidebarPresentation
    private let providerIcon = NSImageView()
    private let titleLabel = NSTextField(labelWithString: "")
    private let subtitleLabel = NSTextField(labelWithString: "")
    private let statusIcon = NSImageView()
    var onSelect: (@MainActor (AgentLifecycleRecord) -> Void)?
    var onMoveFocus: (@MainActor (Int) -> Void)?

    init(presentation: AgentSidebarPresentation) {
        self.presentation = presentation
        super.init(color: presentation.isSelected ? ZmuxPalette.selection : .clear)
        providerIcon.image = NSImage(systemSymbolName: "sparkles", accessibilityDescription: nil)
        providerIcon.contentTintColor = ZmuxPalette.subdued
        providerIcon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 12, weight: .medium)
        titleLabel.textColor = .labelColor
        titleLabel.lineBreakMode = .byTruncatingTail
        subtitleLabel.font = .systemFont(ofSize: 10, weight: .regular)
        subtitleLabel.textColor = ZmuxPalette.inactive
        subtitleLabel.lineBreakMode = .byTruncatingTail
        statusIcon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 10, weight: .medium)
        for view in [providerIcon, titleLabel, subtitleLabel, statusIcon] { view.translatesAutoresizingMaskIntoConstraints = false; addSubview(view) }
        NSLayoutConstraint.activate([
            providerIcon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            providerIcon.centerYAnchor.constraint(equalTo: centerYAnchor),
            providerIcon.widthAnchor.constraint(equalToConstant: 15),
            providerIcon.heightAnchor.constraint(equalToConstant: 15),
            statusIcon.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -9),
            statusIcon.centerYAnchor.constraint(equalTo: centerYAnchor),
            statusIcon.widthAnchor.constraint(equalToConstant: 12),
            statusIcon.heightAnchor.constraint(equalToConstant: 12),
            titleLabel.leadingAnchor.constraint(equalTo: providerIcon.trailingAnchor, constant: 7),
            titleLabel.trailingAnchor.constraint(lessThanOrEqualTo: statusIcon.leadingAnchor, constant: -5),
            titleLabel.topAnchor.constraint(equalTo: topAnchor, constant: 8),
            subtitleLabel.leadingAnchor.constraint(equalTo: titleLabel.leadingAnchor),
            subtitleLabel.trailingAnchor.constraint(lessThanOrEqualTo: statusIcon.leadingAnchor, constant: -5),
            subtitleLabel.topAnchor.constraint(equalTo: titleLabel.bottomAnchor, constant: 2),
            subtitleLabel.bottomAnchor.constraint(lessThanOrEqualTo: bottomAnchor, constant: -7)
        ])
        heightAnchor.constraint(equalToConstant: 48).isActive = true
        layer?.cornerRadius = 6
        update(presentation)
    }

    var identity: AgentSidebarIdentity { presentation.identity }

    func update(_ newPresentation: AgentSidebarPresentation) {
        presentation = newPresentation
        layer?.backgroundColor = newPresentation.isSelected ? ZmuxPalette.selection.cgColor : NSColor.clear.cgColor
        layer?.borderWidth = newPresentation.isSelected ? 1 : 0
        layer?.borderColor = newPresentation.isSelected ? ZmuxPalette.selectionBorder.cgColor : NSColor.clear.cgColor
        titleLabel.stringValue = newPresentation.title
        titleLabel.font = .systemFont(ofSize: 11.5, weight: newPresentation.isSelected ? .medium : .regular)
        subtitleLabel.stringValue = newPresentation.subtitle
        statusIcon.image = NSImage(systemSymbolName: newPresentation.section.symbol, accessibilityDescription: nil)
        statusIcon.contentTintColor = newPresentation.section.color
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityLabel("\(newPresentation.title), \(newPresentation.subtitle), \(newPresentation.status)")
        setAccessibilityValue(newPresentation.status)
        setAccessibilitySelected(newPresentation.isSelected)
        toolTip = "\(newPresentation.providerName) · \(newPresentation.workspaceName) · \(newPresentation.surfaceContext) · \(newPresentation.status)"
    }

    required init?(coder: NSCoder) { nil }
    override var acceptsFirstResponder: Bool { true }
    override var canBecomeKeyView: Bool { window != nil && !isHiddenOrHasHiddenAncestor }
    override func becomeFirstResponder() -> Bool { needsDisplay = true; return true }
    override func resignFirstResponder() -> Bool { needsDisplay = true; return true }
    override func mouseDown(with event: NSEvent) { onSelect?(presentation.record) }
    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 36, 49: onSelect?(presentation.record)
        case 125: onMoveFocus?(1)
        case 126: onMoveFocus?(-1)
        default: super.keyDown(with: event)
        }
    }
    override func accessibilityPerformPress() -> Bool { onSelect?(presentation.record); return true }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard window?.firstResponder === self else { return }
        NSGraphicsContext.saveGraphicsState()
        NSFocusRingPlacement.only.set()
        bounds.insetBy(dx: 1, dy: 1).fill()
        NSGraphicsContext.restoreGraphicsState()
    }
}

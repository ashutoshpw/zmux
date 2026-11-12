import AppKit

struct ProgramSidebarRowIdentity: Hashable, Sendable {
    let owner: ProgramStatusTerminalIdentity
    let sessionEpoch: UInt64
    let commandEpoch: UInt64
    let id: String
}

struct ProgramSidebarPresentation: Hashable, Sendable {
    let target: ProgramSelectionTarget
    let source: ProgramStatusSource
    let availability: ProgramStatusAvailability
    let state: ProgramStatusState
    let kind: ProgramStatusKind?
    let progress: UInt8?
    let app: String?
    let title: String?
    let message: String?
    let workspaceName: String
    let surfaceTitle: String
    let surfaceContext: String
    let isSelected: Bool

    var identity: ProgramSidebarRowIdentity {
        ProgramSidebarRowIdentity(owner: terminalIdentity, sessionEpoch: target.sessionEpoch,
                                  commandEpoch: target.commandEpoch, id: target.id)
    }

    var terminalIdentity: ProgramStatusTerminalIdentity {
        ProgramStatusTerminalIdentity(identityNamespace: target.identityNamespace,
            windowId: target.windowId, windowUuid: target.windowUuid,
            workspaceId: target.workspaceId, workspaceUuid: target.workspaceUuid,
            paneId: target.paneId, paneUuid: target.paneUuid,
            surfaceId: target.surfaceId, surfaceUuid: target.surfaceUuid,
            sessionId: target.sessionId)
    }

    var displayID: String { target.id.isEmpty ? "Root program" : ProgramSidebarText.safe(target.id, limit: 64) }
    var displayTitle: String {
        for candidate in [title, app, target.id.isEmpty ? "Root program" : target.id] {
            if let candidate {
                let trimmed = candidate.trimmingCharacters(in: .whitespacesAndNewlines)
                if !trimmed.isEmpty { return ProgramSidebarText.safe(trimmed, limit: 80) }
            }
        }
        return "Program"
    }

    var status: String {
        let state: String
        switch self.state {
        case .idle: state = "Idle"
        case .working: state = "Working"
        case .done: state = "Done"
        case .blocked: state = "Blocked"
        case .error: state = "Error"
        }
        let progressText = progress.map { " · \($0)%" } ?? ""
        if availability == .available { return state + progressText }
        let availabilityText = "Unavailable"
        return "\(availabilityText) · last reported \(state.lowercased())\(progressText)"
    }

    var detail: String {
        let reason = (state == .blocked || state == .error) ? message : nil
        let kindText = state == .blocked ? kind?.rawValue : nil
        let appText = app.map { "App \(ProgramSidebarText.safe($0, limit: 32))" }
        let values = [displayID, appText, kindText.map { ProgramSidebarText.safe($0, limit: 40) },
                      reason.map { ProgramSidebarText.safe($0, limit: 96) }].compactMap { $0 }
        return values.joined(separator: " · ")
    }

    var provenance: String {
        "\(ProgramSidebarText.safe(workspaceName, limit: 48)) · \(ProgramSidebarText.safe(surfaceContext, limit: 48)) · Self-reported OSC 7501"
    }

    var accessibilityLabel: String {
        "\(displayTitle), \(displayID), \(status), \(provenance)"
    }

    var priority: Int {
        switch state {
        case .blocked: 0
        case .error: 1
        case .working: 2
        case .done: 3
        case .idle: 4
        }
    }
}

struct ProgramSidebarTerminalPresentation: Hashable, Sendable {
    let identity: ProgramStatusTerminalIdentity
    let availability: ProgramStatusAvailability
    let recordCount: UInt16
    let recordsComplete: Bool
    let omittedRecords: UInt16
    let workspaceName: String
    let surfaceTitle: String
    let surfaceContext: String
    let records: [ProgramSidebarPresentation]

    var summary: ProgramSidebarPresentation? {
        records.min {
            if $0.priority != $1.priority { return $0.priority < $1.priority }
            return $0.target.id.localizedStandardCompare($1.target.id) == .orderedAscending
        }
    }

    var orderedRecords: [ProgramSidebarPresentation] {
        records.sorted {
            let leftDepth = $0.target.id.isEmpty ? 0 : $0.target.id.split(separator: "/").count
            let rightDepth = $1.target.id.isEmpty ? 0 : $1.target.id.split(separator: "/").count
            if leftDepth != rightDepth { return leftDepth < rightDepth }
            return $0.target.id.localizedStandardCompare($1.target.id) == .orderedAscending
        }
    }

    var groupLabel: String {
        "\(ProgramSidebarText.safe(workspaceName, limit: 48)) · \(ProgramSidebarText.safe(surfaceTitle, limit: 48))"
    }
}

enum ProgramSidebarText {
    /// Render formatting/control scalars visibly before using terminal-provided text in AppKit labels.
    static func safe(_ value: String, limit: Int) -> String {
        var output = ""
        var consumed = 0
        var truncated = false
        for scalar in value.unicodeScalars {
            guard consumed < limit else { truncated = true; break }
            consumed += 1
            if mustEscape(scalar.value) {
                let digits = String(scalar.value, radix: 16, uppercase: true)
                let padded = String(repeating: "0", count: max(0, 4 - digits.count)) + digits
                output += "⟦U+\(padded)⟧"
            } else {
                output.unicodeScalars.append(scalar)
            }
        }
        if truncated { output += "…" }
        return output
    }

    private static func mustEscape(_ value: UInt32) -> Bool {
        value <= 0x1F || (0x7F...0x9F).contains(value)
            || [0x00AD, 0x034F, 0x061C, 0x180E, 0xFEFF].contains(value)
            || (0x200B...0x200F).contains(value)
            || (0x202A...0x202E).contains(value)
            || (0x2060...0x206F).contains(value)
            || (0xFFF9...0xFFFB).contains(value)
            || (0xE0000...0xE007F).contains(value)
    }
}

@MainActor
final class ProgramSidebarList: FlippedStackView {
    private enum FocusedElement {
        case summary(ProgramStatusTerminalIdentity)
        case header(ProgramStatusTerminalIdentity)
        case record(ProgramSidebarRowIdentity)
        case other
    }

    private var terminals: [ProgramSidebarTerminalPresentation]?
    private var statusAvailable = true
    private var collapsedTerminals: Set<ProgramStatusTerminalIdentity> = []
    private var summaryRows: [ProgramStatusTerminalIdentity: ProgramSidebarRowView] = [:]
    private var headers: [ProgramStatusTerminalIdentity: ProgramSidebarTerminalHeaderView] = [:]
    private var recordRows: [ProgramSidebarRowIdentity: ProgramSidebarRowView] = [:]
    private var widthConstraints: [ObjectIdentifier: NSLayoutConstraint] = [:]
    private var focusableViews: [NSView] = []
    var onSelect: (@MainActor (ProgramSelectionTarget) -> Void)?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        orientation = .vertical
        alignment = .trailing
        spacing = 2
        edgeInsets = NSEdgeInsets(top: 4, left: 8, bottom: 10, right: 8)
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel("Programs")
        setAccessibilityHelp("Program status is self-reported by terminal applications using OSC 7501. Select a row to focus its existing terminal.")
        let loading = makeEmptyState(title: "Loading program status…", detail: "Waiting for terminal reports.")
        addArrangedSubview(loading)
        widthConstraints[ObjectIdentifier(loading)]?.isActive = true
        setAccessibilityChildren(arrangedSubviews)
        setAccessibilityValue("Loading")
    }

    required init?(coder: NSCoder) { nil }

    func update(_ newTerminals: [ProgramSidebarTerminalPresentation]?, statusAvailable: Bool) {
        guard terminals != newTerminals || self.statusAvailable != statusAvailable else { return }
        let previousIdentities = Set(terminals?.map(\.identity) ?? [])
        terminals = newTerminals
        self.statusAvailable = statusAvailable
        for terminal in newTerminals ?? [] where !previousIdentities.contains(terminal.identity) {
            collapsedTerminals.insert(terminal.identity)
        }
        reconcile()
    }

    private func reconcile() {
        let focused = captureFocusedElement()
        var desired: [NSView] = []
        focusableViews.removeAll(keepingCapacity: true)

        guard statusAvailable, let terminals else {
            setAccessibilityValue("Program status unavailable")
            reconcileViews([makeEmptyState(title: "Program status unavailable",
                detail: "This desktop has not published OSC 7501 program status.")])
            prune(keeping: [], expanded: [])
            restoreFocus(focused)
            return
        }
        guard !terminals.isEmpty else {
            setAccessibilityValue("No program reports")
            reconcileViews([makeEmptyState(title: "No program reports",
                detail: "Programs appear here when a terminal reports OSC 7501 status.")])
            prune(keeping: [], expanded: [])
            restoreFocus(focused)
            return
        }

        let ordered = terminals.sorted {
            if $0.workspaceName != $1.workspaceName {
                return $0.workspaceName.localizedStandardCompare($1.workspaceName) == .orderedAscending
            }
            if $0.surfaceTitle != $1.surfaceTitle {
                return $0.surfaceTitle.localizedStandardCompare($1.surfaceTitle) == .orderedAscending
            }
            return $0.identity.surfaceId < $1.identity.surfaceId
        }
        var expanded: Set<ProgramStatusTerminalIdentity> = []
        for terminal in ordered {
            guard let summary = terminal.summary else { continue }
            if collapsedTerminals.contains(terminal.identity) {
                let row = summaryRows[terminal.identity] ?? makeSummaryRow(for: summary)
                row.update(summary, expanded: false, recordCount: terminal.recordCount,
                           canExpand: terminal.recordCount > 1)
                desired.append(row)
                focusableViews.append(row)
            } else {
                expanded.insert(terminal.identity)
                let header = headers[terminal.identity] ?? makeHeader(for: terminal)
                header.update(terminal, expanded: true)
                desired.append(header)
                focusableViews.append(header)
                for record in terminal.orderedRecords {
                    let row = recordRows[record.identity] ?? makeRecordRow(for: record)
                    row.update(record, expanded: false, recordCount: terminal.recordCount, canExpand: false)
                    desired.append(row)
                    focusableViews.append(row)
                }
            }
        }
        reconcileViews(desired)
        prune(keeping: ordered, expanded: expanded)
        setAccessibilityChildren(arrangedSubviews)
        let recordCount = terminals.reduce(0) { $0 + $1.records.count }
        setAccessibilityValue("\(terminals.count) terminals, \(recordCount) program reports")
        window?.recalculateKeyViewLoop()
        restoreFocus(focused)
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

    private func makeSummaryRow(for presentation: ProgramSidebarPresentation) -> ProgramSidebarRowView {
        let row = ProgramSidebarRowView(presentation: presentation, expandable: true)
        row.onSelect = { [weak self] target in self?.onSelect?(target) }
        row.onToggle = { [weak self] identity in self?.toggle(identity) }
        row.onMoveFocus = { [weak self, weak row] offset in
            guard let self, let row else { return }
            self.moveFocus(from: row, offset: offset)
        }
        summaryRows[presentation.terminalIdentity] = row
        widthConstraints[ObjectIdentifier(row)] = row.widthAnchor.constraint(equalTo: widthAnchor, constant: -16)
        return row
    }

    private func makeRecordRow(for presentation: ProgramSidebarPresentation) -> ProgramSidebarRowView {
        let row = ProgramSidebarRowView(presentation: presentation, expandable: false)
        row.onSelect = { [weak self] target in self?.onSelect?(target) }
        row.onMoveFocus = { [weak self, weak row] offset in
            guard let self, let row else { return }
            self.moveFocus(from: row, offset: offset)
        }
        recordRows[presentation.identity] = row
        widthConstraints[ObjectIdentifier(row)] = row.widthAnchor.constraint(equalTo: widthAnchor, constant: -16)
        return row
    }

    private func makeHeader(for terminal: ProgramSidebarTerminalPresentation) -> ProgramSidebarTerminalHeaderView {
        let header = ProgramSidebarTerminalHeaderView(identity: terminal.identity, terminal: terminal)
        header.onToggle = { [weak self] identity in self?.toggle(identity) }
        headers[terminal.identity] = header
        widthConstraints[ObjectIdentifier(header)] = header.widthAnchor.constraint(equalTo: widthAnchor, constant: -16)
        return header
    }

    private func toggle(_ identity: ProgramStatusTerminalIdentity) {
        if collapsedTerminals.contains(identity) { collapsedTerminals.remove(identity) }
        else { collapsedTerminals.insert(identity) }
        reconcile()
    }

    private func moveFocus(from view: NSView, offset: Int) {
        guard let index = focusableViews.firstIndex(where: { $0 === view }), !focusableViews.isEmpty else { return }
        let next = min(focusableViews.count - 1, max(0, index + offset))
        focus(focusableViews[next])
    }

    private func reconcileViews(_ desired: [NSView]) {
        let desiredIDs = Set(desired.map(ObjectIdentifier.init))
        for view in arrangedSubviews where !desiredIDs.contains(ObjectIdentifier(view)) {
            removeArrangedSubview(view)
            widthConstraints[ObjectIdentifier(view)]?.isActive = false
            view.removeFromSuperview()
        }
        for (index, view) in desired.enumerated() {
            if index < arrangedSubviews.count, arrangedSubviews[index] === view { continue }
            if arrangedSubviews.contains(where: { $0 === view }) { removeArrangedSubview(view) }
            insertArrangedSubview(view, at: min(index, arrangedSubviews.count))
            if let constraint = widthConstraints[ObjectIdentifier(view)], !constraint.isActive { constraint.isActive = true }
        }
        setAccessibilityChildren(arrangedSubviews)
    }

    private func prune(keeping terminals: [ProgramSidebarTerminalPresentation], expanded: Set<ProgramStatusTerminalIdentity>) {
        let identities = Set(terminals.map(\.identity))
        let records = Set(terminals.flatMap(\.records).map(\.identity))
        for identity in Array(summaryRows.keys) where !identities.contains(identity) {
            if let row = summaryRows.removeValue(forKey: identity) { removeWidthConstraint(for: row) }
        }
        for identity in Array(headers.keys) where !identities.contains(identity) {
            if let header = headers.removeValue(forKey: identity) { removeWidthConstraint(for: header) }
        }
        for identity in Array(recordRows.keys) where !records.contains(identity) {
            if let row = recordRows.removeValue(forKey: identity) { removeWidthConstraint(for: row) }
        }
        collapsedTerminals.formIntersection(identities)
        for identity in expanded where collapsedTerminals.contains(identity) { collapsedTerminals.remove(identity) }
    }

    private func removeWidthConstraint(for view: NSView) {
        if let constraint = widthConstraints.removeValue(forKey: ObjectIdentifier(view)) { constraint.isActive = false }
    }

    private func captureFocusedElement() -> FocusedElement {
        guard let focused = window?.firstResponder as? NSView,
              arrangedSubviews.contains(where: { $0 === focused }) else { return .other }
        if let header = focused as? ProgramSidebarTerminalHeaderView { return .header(header.identity) }
        if let row = focused as? ProgramSidebarRowView {
            return row.isSummary ? .summary(row.identity.owner) : .record(row.identity)
        }
        return .other
    }

    private func restoreFocus(_ focused: FocusedElement) {
        switch focused {
        case .other: return
        case .header(let identity):
            if let header = headers[identity], arrangedSubviews.contains(where: { $0 === header }) { focus(header) }
            else { focusNearest() }
        case .summary(let identity):
            if let row = summaryRows[identity], arrangedSubviews.contains(where: { $0 === row }) { focus(row) }
            else { focusNearest() }
        case .record(let identity):
            if let row = recordRows[identity], arrangedSubviews.contains(where: { $0 === row }) { focus(row) }
            else if let row = summaryRows[identity.owner], arrangedSubviews.contains(where: { $0 === row }) { focus(row) }
            else { focusNearest() }
        }
    }

    private func focusNearest() {
        guard let first = focusableViews.first else { window?.makeFirstResponder(nil); return }
        focus(first)
    }

    private func focus(_ view: NSView) {
        guard window?.makeFirstResponder(view) == true else { return }
        view.scrollToVisible(view.bounds)
    }
}

@MainActor
private final class ProgramSidebarTerminalHeaderView: NSView {
    fileprivate let identity: ProgramStatusTerminalIdentity
    private let title = NSTextField(labelWithString: "")
    private let subtitle = NSTextField(labelWithString: "")
    private let disclosure = NSImageView()
    private var expanded = true
    var onToggle: (@MainActor (ProgramStatusTerminalIdentity) -> Void)?

    init(identity: ProgramStatusTerminalIdentity, terminal: ProgramSidebarTerminalPresentation) {
        self.identity = identity
        super.init(frame: .zero)
        title.font = .systemFont(ofSize: 10.5, weight: .semibold)
        title.textColor = ZmuxPalette.subdued
        title.lineBreakMode = .byTruncatingTail
        subtitle.font = .systemFont(ofSize: 9.5)
        subtitle.textColor = ZmuxPalette.inactive
        subtitle.lineBreakMode = .byTruncatingTail
        disclosure.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 8, weight: .semibold)
        for view in [disclosure, title, subtitle] { view.translatesAutoresizingMaskIntoConstraints = false; addSubview(view) }
        NSLayoutConstraint.activate([
            disclosure.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 3),
            disclosure.centerYAnchor.constraint(equalTo: centerYAnchor),
            disclosure.widthAnchor.constraint(equalToConstant: 9), disclosure.heightAnchor.constraint(equalToConstant: 12),
            title.leadingAnchor.constraint(equalTo: disclosure.trailingAnchor, constant: 5),
            title.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -5),
            title.topAnchor.constraint(equalTo: topAnchor, constant: 5),
            subtitle.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            subtitle.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -5),
            subtitle.topAnchor.constraint(equalTo: title.bottomAnchor, constant: 2),
            subtitle.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -5)
        ])
        heightAnchor.constraint(equalToConstant: 38).isActive = true
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityHelp("Press Return or Space to collapse or expand this terminal's program records.")
        update(terminal, expanded: true)
    }

    required init?(coder: NSCoder) { nil }
    override var acceptsFirstResponder: Bool { true }
    override var canBecomeKeyView: Bool { window != nil && !isHiddenOrHasHiddenAncestor }
    override func mouseDown(with event: NSEvent) { window?.makeFirstResponder(self); toggle() }
    override func keyDown(with event: NSEvent) {
        if event.keyCode == 36 || event.keyCode == 49 { toggle() } else { super.keyDown(with: event) }
    }
    override func accessibilityPerformPress() -> Bool { toggle(); return true }

    func update(_ terminal: ProgramSidebarTerminalPresentation, expanded: Bool) {
        self.expanded = expanded
        title.stringValue = ProgramSidebarText.safe(terminal.groupLabel, limit: 96)
        let availability = terminal.availability == .available ? "Available" : "Unavailable"
        let details = terminal.recordsComplete
            ? "\(terminal.recordCount) programs"
            : "\(terminal.records.count) of \(terminal.recordCount) shown · focus to load details"
        subtitle.stringValue = "\(details) · \(availability) · Self-reported OSC 7501"
        disclosure.image = NSImage(systemSymbolName: expanded ? "chevron.down" : "chevron.right", accessibilityDescription: nil)
        setAccessibilityLabel("Programs for \(title.stringValue), Self-reported OSC 7501")
        setAccessibilityValue("\(expanded ? "Expanded" : "Collapsed"), \(details), \(availability)")
    }

    private func toggle() { onToggle?(identity) }
}

@MainActor
private final class ProgramSidebarRowView: BackgroundView {
    private var presentation: ProgramSidebarPresentation
    private let programIcon = NSImageView()
    private let title = NSTextField(labelWithString: "")
    private let subtitle = NSTextField(labelWithString: "")
    private let detail = NSTextField(labelWithString: "")
    private let statusIcon = NSImageView()
    private let disclosureButton = NSButton()
    private let expandable: Bool
    private var canExpand = false
    private var expanded = false
    private var recordCount = 1
    var onSelect: (@MainActor (ProgramSelectionTarget) -> Void)?
    var onToggle: (@MainActor (ProgramStatusTerminalIdentity) -> Void)?
    var onMoveFocus: (@MainActor (Int) -> Void)?

    init(presentation: ProgramSidebarPresentation, expandable: Bool) {
        self.presentation = presentation
        self.expandable = expandable
        super.init(color: presentation.isSelected ? ZmuxPalette.selection : .clear)
        programIcon.image = NSImage(systemSymbolName: "terminal", accessibilityDescription: nil)
        programIcon.contentTintColor = ZmuxPalette.subdued
        programIcon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 12, weight: .medium)
        title.textColor = .labelColor
        title.lineBreakMode = .byTruncatingTail
        subtitle.font = .systemFont(ofSize: 9.5)
        subtitle.textColor = ZmuxPalette.inactive
        subtitle.lineBreakMode = .byTruncatingTail
        detail.font = .systemFont(ofSize: 9.5)
        detail.textColor = ZmuxPalette.subdued
        detail.lineBreakMode = .byTruncatingTail
        statusIcon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 10, weight: .medium)
        disclosureButton.isBordered = false
        disclosureButton.imagePosition = .imageOnly
        disclosureButton.target = self
        disclosureButton.action = #selector(toggleExpanded)
        for view in [programIcon, title, subtitle, detail, statusIcon, disclosureButton] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        NSLayoutConstraint.activate([
            programIcon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            programIcon.topAnchor.constraint(equalTo: topAnchor, constant: 9),
            programIcon.widthAnchor.constraint(equalToConstant: 15), programIcon.heightAnchor.constraint(equalToConstant: 15),
            disclosureButton.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -5),
            disclosureButton.topAnchor.constraint(equalTo: topAnchor, constant: 5),
            disclosureButton.widthAnchor.constraint(equalToConstant: 18), disclosureButton.heightAnchor.constraint(equalToConstant: 18),
            statusIcon.trailingAnchor.constraint(equalTo: (expandable ? disclosureButton.leadingAnchor : trailingAnchor), constant: -7),
            statusIcon.topAnchor.constraint(equalTo: topAnchor, constant: 8),
            statusIcon.widthAnchor.constraint(equalToConstant: 12), statusIcon.heightAnchor.constraint(equalToConstant: 12),
            title.leadingAnchor.constraint(equalTo: programIcon.trailingAnchor, constant: 7),
            title.trailingAnchor.constraint(lessThanOrEqualTo: statusIcon.leadingAnchor, constant: -5),
            title.topAnchor.constraint(equalTo: topAnchor, constant: 5),
            subtitle.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            subtitle.trailingAnchor.constraint(lessThanOrEqualTo: title.trailingAnchor),
            subtitle.topAnchor.constraint(equalTo: title.bottomAnchor, constant: 2),
            detail.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            detail.trailingAnchor.constraint(lessThanOrEqualTo: title.trailingAnchor),
            detail.topAnchor.constraint(equalTo: subtitle.bottomAnchor, constant: 2),
            detail.bottomAnchor.constraint(lessThanOrEqualTo: bottomAnchor, constant: -5)
        ])
        heightAnchor.constraint(equalToConstant: 58).isActive = true
        layer?.cornerRadius = 6
        disclosureButton.isHidden = !expandable
        disclosureButton.setAccessibilityRole(.button)
        disclosureButton.setAccessibilityLabel("Show program records")
        disclosureButton.setAccessibilityHelp("Expand or collapse details for this terminal.")
        update(presentation, expanded: false, recordCount: 1, canExpand: expandable)
    }

    required init?(coder: NSCoder) { nil }
    var identity: ProgramSidebarRowIdentity { presentation.identity }
    var isSummary: Bool { expandable }
    override var acceptsFirstResponder: Bool { true }
    override var canBecomeKeyView: Bool { window != nil && !isHiddenOrHasHiddenAncestor }
    override func becomeFirstResponder() -> Bool { needsDisplay = true; return true }
    override func resignFirstResponder() -> Bool { needsDisplay = true; return true }
    override func mouseDown(with event: NSEvent) { onSelect?(presentation.target) }
    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 36, 49: onSelect?(presentation.target)
        case 125: onMoveFocus?(1)
        case 126: onMoveFocus?(-1)
        default: super.keyDown(with: event)
        }
    }
    override func accessibilityPerformPress() -> Bool { onSelect?(presentation.target); return true }

    func update(_ newPresentation: ProgramSidebarPresentation, expanded: Bool, recordCount: UInt16,
                canExpand: Bool) {
        presentation = newPresentation
        self.expanded = expanded
        self.recordCount = Int(recordCount)
        self.canExpand = canExpand
        layer?.backgroundColor = newPresentation.isSelected ? ZmuxPalette.selection.cgColor : NSColor.clear.cgColor
        layer?.borderWidth = newPresentation.isSelected ? 1 : 0
        layer?.borderColor = newPresentation.isSelected ? ZmuxPalette.selectionBorder.cgColor : NSColor.clear.cgColor
        title.stringValue = newPresentation.displayTitle
        title.font = .systemFont(ofSize: 11, weight: newPresentation.isSelected ? .medium : .regular)
        subtitle.stringValue = newPresentation.provenance
        detail.stringValue = newPresentation.detail
        let (symbol, color) = Self.statusStyle(newPresentation.state, availability: newPresentation.availability)
        statusIcon.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
        statusIcon.contentTintColor = color
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityLabel(newPresentation.accessibilityLabel)
        setAccessibilityValue(newPresentation.status)
        setAccessibilitySelected(newPresentation.isSelected)
        let showsDisclosure = expandable && canExpand
        disclosureButton.isHidden = !showsDisclosure
        setAccessibilityChildren(showsDisclosure ? [programIcon, title, subtitle, detail, statusIcon, disclosureButton] : [programIcon, title, subtitle, detail, statusIcon])
        disclosureButton.image = NSImage(systemSymbolName: expanded ? "chevron.down" : "chevron.right", accessibilityDescription: nil)
        disclosureButton.setAccessibilityLabel("\(expanded ? "Collapse" : "Expand") program records for \(ProgramSidebarText.safe(newPresentation.workspaceName, limit: 48))")
        disclosureButton.setAccessibilityValue("\(recordCount) programs")
        toolTip = newPresentation.accessibilityLabel
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard window?.firstResponder === self else { return }
        NSGraphicsContext.saveGraphicsState()
        NSFocusRingPlacement.only.set()
        bounds.insetBy(dx: 1, dy: 1).fill()
        NSGraphicsContext.restoreGraphicsState()
    }

    @objc private func toggleExpanded() { onToggle?(presentation.terminalIdentity) }

    private static func statusStyle(_ state: ProgramStatusState, availability: ProgramStatusAvailability) -> (String, NSColor) {
        guard availability == .available else { return ("clock", ZmuxPalette.inactive) }
        return switch state {
        case .idle: ("circle", ZmuxPalette.inactive)
        case .working: ("circle.dashed", ZmuxPalette.success)
        case .done: ("checkmark.circle.fill", ZmuxPalette.success)
        case .blocked: ("exclamationmark.circle.fill", .systemOrange)
        case .error: ("xmark.octagon.fill", .systemRed)
        }
    }
}

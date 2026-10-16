import AppKit
import PDFKit

/// The narrow application-level callbacks a file panel needs. The coordinator owns the Rust
/// Desktop; a panel never touches the bridge or another window directly.
@MainActor
protocol FilePanelOwner: AnyObject {
    /// Runs one trusted file-channel request off the main actor and returns its validated reply.
    func fileCall(_ request: FileHostRequest, body: Data) async throws -> FileHostReply
    func filePanelFocused(_ surface: UInt64)
    /// Dirty state, banner or title changed; window chrome (tab marker, header) should refresh.
    func filePanelStateChanged(_ surface: UInt64)
    func filePanelReport(_ message: String)
    func filePanelDetach(_ owner: FileOwnerToken)
}

enum FileConflictChoice: Sendable { case reload, overwrite, cancel }

/// Presents `alert` as a sheet of `window` (or modally without one) and returns the response.
@MainActor
func presentSheet(_ alert: NSAlert, in window: NSWindow?) async -> NSApplication.ModalResponse {
    guard let window else { return alert.runModal() }
    return await withCheckedContinuation { continuation in
        alert.beginSheetModal(for: window) { response in continuation.resume(returning: response) }
    }
}

@MainActor
private final class FileEditorTextView: NSTextView {
    var onFocus: (() -> Void)?
    var onEdit: (() -> Void)?
    override func becomeFirstResponder() -> Bool {
        let accepted = super.becomeFirstResponder()
        if accepted { onFocus?() }
        return accepted
    }
    /// Called for every user edit, including undo and redo; programmatic `string` writes do not.
    override func didChangeText() {
        super.didChangeText()
        onEdit?()
    }
}

/// One file tab: an editable monospaced text view, a rendered Markdown view, an image view or a
/// PDF view, plus explicit loading, error, empty, too-large and binary states. The view is
/// retained by the coordinator per surface and re-parented between windows, never recreated by
/// model changes. Rust owns the file service: this view holds the unsaved text (the source of
/// truth for dirtiness) and reconciles a trusted dirty mirror back to the model.
@MainActor
final class FilePanelView: BackgroundView, NSTextViewDelegate {
    private enum Phase { case idle, attaching, ready, failed }
    typealias Banner = FileBanner

    let surfaceID: UInt64
    /// Names this view instance to Rust; a different host cannot take over a bound surface.
    let hostID = UUID()
    let path: String
    let mode: FilePanelMode?
    private let rawMode: String
    private(set) var title: String
    private weak var owner: (any FilePanelOwner)?
    var onFocus: (() -> Void)?
    /// Replaces the conflict sheet; used by the in-app smoke so no human has to click it.
    var conflictChooser: (@MainActor (FileConflict) async -> FileConflictChoice)?
    /// Replaces the "discard unsaved changes and reload" sheet; used by the in-app smoke only.
    var discardConfirmation: (@MainActor () async -> Bool)?

    // Rust-owned binding and the last baseline this view loaded or saved.
    private var phase = Phase.idle
    private var ownerToken: FileOwnerToken?
    private var attachedEpoch: UUID?
    private var latestEpoch: UUID?
    private var writable = false
    private var busy = false
    private var closed = false
    private var staleOwner = false
    private(set) var isLoaded = false
    private(set) var failureCode: String?

    // Editing. Every data-loss decision (what is unsaved, what a save writes, when typing is
    // blocked, what a banner button does) is made by `FileEditorModel`, which the macOS fixture
    // runs; this view only applies the answers. While a reload or attach is in flight the model
    // says the editor is read-only: its reply replaces the whole buffer, so text typed meanwhile
    // would be overwritten with no sign.
    private var model = FileEditorModel()
    var banner: Banner { model.banner }
    var isDirty: Bool { model.isDirty }
    private var suppressEdits = false
    private var lastModelDirty = false
    /// Held for a panel whose Don't Save is being applied; see `DirtyMirrorGate`.
    private var dirtyGate = DirtyMirrorGate()
    private var observeTask: Task<Void, Never>?
    private var observeLoopID = 0
    private var observeGaveUp = false

    // External-change bookkeeping.
    private var workerStalledMs: UInt64?

    // Presentation.
    private static let wrapKey = "filePanel.wordWrap"
    private(set) var wordWrap = UserDefaults.standard.bool(forKey: FilePanelView.wrapKey)
    private(set) var markdownShowsSource = false
    private var markdownRendered = false
    private var markdownGeneration: UInt64 = 0
    private var markdownTask: Task<Void, Never>?
    private var decodeGeneration: UInt64 = 0
    private var decodeTask: Task<Void, Never>?
    private var imageWidth = 0
    private var imageHeight = 0
    private var pdfPageCount = 0

    private let editor = FileEditorTextView(frame: .zero)
    private let editorScroll = NSScrollView()
    private let markdownView = NSTextView(frame: .zero)
    private let markdownScroll = NSScrollView()
    private let imageView = NSImageView()
    private let imageContainer = BackgroundView(color: ZmuxPalette.terminal)
    private let pdfView = PDFView()
    private let stateContainer = BackgroundView(color: ZmuxPalette.terminal)
    private let stateIcon = NSImageView()
    private let stateTitle = NSTextField(labelWithString: "")
    private let stateDetail = NSTextField(wrappingLabelWithString: "")
    private lazy var retryButton = NSButton(title: "Retry", target: self, action: #selector(retryTapped))
    private let header = BackgroundView(color: ZmuxPalette.chrome)
    private let nameLabel = NSTextField(labelWithString: "")
    private let statusLabel = NSTextField(labelWithString: "")
    private lazy var modeControl = NSSegmentedControl(labels: ["Preview", "Source"], trackingMode: .selectOne,
                                                      target: self, action: #selector(modeChanged(_:)))
    private lazy var wrapButton = IconButton(symbol: "text.alignleft", label: "Toggle word wrap (⌥⌘L)", target: self, action: #selector(wrapTapped))
    private let bannerView = BackgroundView(color: ZmuxPalette.chrome)
    private let bannerLabel = NSTextField(labelWithString: "")
    private lazy var bannerPrimary = NSButton(title: "", target: self, action: #selector(primaryTapped))
    private lazy var bannerSecondary = NSButton(title: "", target: self, action: #selector(secondaryTapped))
    private let content = NSView()
    private var bannerHeight: NSLayoutConstraint?
    private var presented: NSView?

    init(surface: Surface, file: FileSurface, owner: any FilePanelOwner) {
        surfaceID = surface.id
        path = file.path
        rawMode = file.mode
        mode = FilePanelMode(rawValue: file.mode)
        title = surface.title
        self.owner = owner
        super.init(color: ZmuxPalette.terminal)
        configureControls()
        buildLayout()
        refreshHeader()
        showLoading("Opening " + title + "…")
    }
    required init?(coder: NSCoder) { nil }

    // MARK: - Queries used by menus, window chrome, close flows and the smoke

    var displayName: String { title }
    var currentText: String { editor.string }
    var hasUnsavedChanges: Bool { model.hasUnsavedChanges(editable: mode?.isEditableText ?? false) }
    var isMarkdown: Bool { mode == .markdown }
    var supportsWordWrap: Bool { (mode?.isEditableText ?? false) && presented === editorScroll }
    var canSave: Bool {
        model.canSave(editable: mode?.isEditableText ?? false, writable: writable, attached: ownerToken != nil,
                      busy: busy, staleOwner: staleOwner)
    }
    var canReload: Bool { !busy && !closed && (ownerToken != nil || staleOwner || phase == .failed) }
    var editorTextView: NSTextView { editor }
    var markdownTextView: NSTextView { markdownView }
    /// The responder that should own the keyboard when this tab is selected.
    var preferredResponder: NSResponder {
        if presented === editorScroll { return editor }
        if presented === markdownScroll { return markdownView }
        if presented === pdfView { return pdfView }
        return self
    }
    override var acceptsFirstResponder: Bool { true }
    /// True when `candidate`'s first responder is this panel or one of its subviews, including
    /// the text views and PDF view; used to restore focus after the pane is rebuilt.
    func ownsFocus(in candidate: NSWindow) -> Bool {
        guard window === candidate, let responder = candidate.firstResponder as? NSView else { return false }
        return responder === self || responder.isDescendant(of: self)
    }
    /// A short stable description of what is on screen, for tests and diagnostics.
    var presentationSummary: String {
        if let failureCode { return "error:" + failureCode }
        guard isLoaded else { return "loading" }
        switch mode {
        case .text?: return "text"
        case .markdown?: return markdownShowsSource ? "markdown:source" : (markdownRendered ? "markdown:rendered" : "markdown:pending")
        case .image?: return "image:\(imageWidth)x\(imageHeight)"
        case .pdf?: return "pdf:\(pdfPageCount)"
        case nil: return "unsupported"
        }
    }

    // MARK: - Construction

    private func configureControls() {
        editor.isRichText = false
        editor.importsGraphics = false
        editor.allowsUndo = true
        editor.usesFontPanel = false
        // Typing must never alter bytes: no smart quotes, dashes, replacements or link detection.
        editor.isAutomaticQuoteSubstitutionEnabled = false
        editor.isAutomaticDashSubstitutionEnabled = false
        editor.isAutomaticTextReplacementEnabled = false
        editor.isAutomaticSpellingCorrectionEnabled = false
        editor.isAutomaticLinkDetectionEnabled = false
        editor.isAutomaticDataDetectionEnabled = false
        editor.isContinuousSpellCheckingEnabled = false
        editor.isGrammarCheckingEnabled = false
        editor.smartInsertDeleteEnabled = false
        editor.font = .monospacedSystemFont(ofSize: 13, weight: .regular)
        editor.textColor = .labelColor
        editor.insertionPointColor = .labelColor
        editor.drawsBackground = false
        editor.textContainerInset = NSSize(width: 12, height: 10)
        editor.isEditable = false
        editor.isSelectable = true
        editor.isVerticallyResizable = true
        editor.minSize = .zero
        editor.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        editor.setAccessibilityLabel("File editor: " + title)
        editor.onFocus = { [weak self] in self?.onFocus?() }
        editor.onEdit = { [weak self] in self?.editorDidChange() }
        editorScroll.hasVerticalScroller = true
        editorScroll.drawsBackground = false
        editorScroll.borderType = .noBorder
        editorScroll.documentView = editor
        editorScroll.setAccessibilityLabel("File editor scroll area")

        markdownView.isEditable = false
        markdownView.isSelectable = true
        markdownView.isRichText = true
        markdownView.drawsBackground = false
        markdownView.textContainerInset = NSSize(width: 24, height: 18)
        markdownView.isVerticallyResizable = true
        markdownView.isHorizontallyResizable = false
        markdownView.autoresizingMask = [.width]
        markdownView.minSize = .zero
        markdownView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        markdownView.textContainer?.widthTracksTextView = true
        markdownView.delegate = self
        markdownView.setAccessibilityLabel("Rendered Markdown: " + title)
        markdownScroll.hasVerticalScroller = true
        markdownScroll.drawsBackground = false
        markdownScroll.borderType = .noBorder
        markdownScroll.documentView = markdownView

        imageView.imageScaling = .scaleProportionallyDown
        imageView.imageAlignment = .alignCenter
        imageView.animates = false
        imageView.isEditable = false
        imageView.setAccessibilityLabel("Image preview: " + title)
        imageView.translatesAutoresizingMaskIntoConstraints = false
        imageContainer.addSubview(imageView)
        NSLayoutConstraint.activate([
            imageView.leadingAnchor.constraint(equalTo: imageContainer.leadingAnchor, constant: 12),
            imageView.trailingAnchor.constraint(equalTo: imageContainer.trailingAnchor, constant: -12),
            imageView.topAnchor.constraint(equalTo: imageContainer.topAnchor, constant: 12),
            imageView.bottomAnchor.constraint(equalTo: imageContainer.bottomAnchor, constant: -12)
        ])

        pdfView.autoScales = true
        pdfView.displayMode = .singlePageContinuous
        pdfView.backgroundColor = ZmuxPalette.terminal
        pdfView.setAccessibilityLabel("PDF preview: " + title)

        stateIcon.image = NSImage(systemSymbolName: "exclamationmark.triangle", accessibilityDescription: nil)
        stateIcon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 26, weight: .light)
        stateIcon.contentTintColor = ZmuxPalette.subdued
        stateTitle.font = .systemFont(ofSize: 15, weight: .medium)
        stateTitle.textColor = .labelColor
        stateTitle.alignment = .center
        stateDetail.font = .systemFont(ofSize: 12)
        stateDetail.textColor = .secondaryLabelColor
        stateDetail.alignment = .center
        stateDetail.preferredMaxLayoutWidth = 420
        retryButton.bezelStyle = .rounded
        retryButton.setAccessibilityLabel("Retry opening the file")
        let stack = NSStackView(views: [stateIcon, stateTitle, stateDetail, retryButton])
        stack.orientation = .vertical
        stack.alignment = .centerX
        stack.spacing = 8
        stack.translatesAutoresizingMaskIntoConstraints = false
        stateContainer.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.centerXAnchor.constraint(equalTo: stateContainer.centerXAnchor),
            stack.centerYAnchor.constraint(equalTo: stateContainer.centerYAnchor),
            stack.widthAnchor.constraint(lessThanOrEqualToConstant: 460)
        ])
        stateContainer.setAccessibilityLabel("File status")

        nameLabel.font = .systemFont(ofSize: 11.5, weight: .medium)
        nameLabel.textColor = .labelColor
        nameLabel.lineBreakMode = .byTruncatingMiddle
        statusLabel.font = .systemFont(ofSize: 10.5)
        statusLabel.textColor = ZmuxPalette.subdued
        statusLabel.lineBreakMode = .byTruncatingTail
        modeControl.selectedSegment = 0
        modeControl.controlSize = .small
        modeControl.setAccessibilityLabel("Markdown view mode")
        modeControl.isHidden = mode != .markdown
        wrapButton.isHidden = !(mode?.isEditableText ?? false)
        bannerLabel.font = .systemFont(ofSize: 11.5)
        bannerLabel.textColor = .labelColor
        bannerLabel.lineBreakMode = .byTruncatingTail
        for button in [bannerPrimary, bannerSecondary] {
            button.bezelStyle = .rounded
            button.controlSize = .small
            button.isHidden = true
        }
        bannerView.isHidden = true
        applyWrapSettings()
    }

    private func buildLayout() {
        for view in [header, bannerView, content] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        for view in [nameLabel, statusLabel, modeControl, wrapButton] {
            view.translatesAutoresizingMaskIntoConstraints = false
            header.addSubview(view)
        }
        for view in [bannerLabel, bannerPrimary, bannerSecondary] {
            view.translatesAutoresizingMaskIntoConstraints = false
            bannerView.addSubview(view)
        }
        nameLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        bannerLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let height = bannerView.heightAnchor.constraint(equalToConstant: 0)
        bannerHeight = height
        NSLayoutConstraint.activate([
            header.topAnchor.constraint(equalTo: topAnchor),
            header.leadingAnchor.constraint(equalTo: leadingAnchor),
            header.trailingAnchor.constraint(equalTo: trailingAnchor),
            header.heightAnchor.constraint(equalToConstant: 26),
            nameLabel.leadingAnchor.constraint(equalTo: header.leadingAnchor, constant: 10),
            nameLabel.centerYAnchor.constraint(equalTo: header.centerYAnchor),
            nameLabel.trailingAnchor.constraint(lessThanOrEqualTo: statusLabel.leadingAnchor, constant: -8),
            statusLabel.centerYAnchor.constraint(equalTo: header.centerYAnchor),
            statusLabel.trailingAnchor.constraint(equalTo: modeControl.leadingAnchor, constant: -8),
            modeControl.centerYAnchor.constraint(equalTo: header.centerYAnchor),
            modeControl.trailingAnchor.constraint(equalTo: wrapButton.leadingAnchor, constant: -6),
            wrapButton.trailingAnchor.constraint(equalTo: header.trailingAnchor, constant: -6),
            wrapButton.centerYAnchor.constraint(equalTo: header.centerYAnchor),
            wrapButton.widthAnchor.constraint(equalToConstant: 22),
            wrapButton.heightAnchor.constraint(equalToConstant: 22),
            bannerView.topAnchor.constraint(equalTo: header.bottomAnchor),
            bannerView.leadingAnchor.constraint(equalTo: leadingAnchor),
            bannerView.trailingAnchor.constraint(equalTo: trailingAnchor),
            height,
            bannerLabel.leadingAnchor.constraint(equalTo: bannerView.leadingAnchor, constant: 10),
            bannerLabel.centerYAnchor.constraint(equalTo: bannerView.centerYAnchor),
            bannerLabel.trailingAnchor.constraint(lessThanOrEqualTo: bannerPrimary.leadingAnchor, constant: -8),
            bannerPrimary.centerYAnchor.constraint(equalTo: bannerView.centerYAnchor),
            bannerPrimary.trailingAnchor.constraint(equalTo: bannerSecondary.leadingAnchor, constant: -6),
            bannerSecondary.centerYAnchor.constraint(equalTo: bannerView.centerYAnchor),
            bannerSecondary.trailingAnchor.constraint(equalTo: bannerView.trailingAnchor, constant: -8),
            content.topAnchor.constraint(equalTo: bannerView.bottomAnchor),
            content.leadingAnchor.constraint(equalTo: leadingAnchor),
            content.trailingAnchor.constraint(equalTo: trailingAnchor),
            content.bottomAnchor.constraint(equalTo: bottomAnchor)
        ])
        let click = NSClickGestureRecognizer(target: self, action: #selector(focusSurface))
        click.delaysPrimaryMouseButtonEvents = false
        addGestureRecognizer(click)
        setAccessibilityRole(.group)
        setAccessibilityLabel("File panel: " + title)
    }

    override func layout() {
        super.layout()
        syncEditorSize()
    }

    /// The text view must at least fill the visible area so a click below the last line, or in an
    /// empty file, still lands in the editor.
    private func syncEditorSize() {
        let visible = editorScroll.contentSize
        editor.minSize = NSSize(width: wordWrap ? 0 : visible.width, height: visible.height)
    }

    @objc private func focusSurface() { onFocus?() }

    private func present(_ view: NSView) {
        guard presented !== view else { return }
        // Decide before the outgoing view leaves the hierarchy: removing the first responder
        // makes the window the responder. Focus that was on this panel (the loading or error
        // state, or the view being replaced) follows the content that now takes its place; focus
        // elsewhere is never taken.
        let hadFocus = window.map { ownsFocus(in: $0) } ?? false
        presented?.removeFromSuperview()
        view.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(view)
        NSLayoutConstraint.activate([
            view.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            view.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            view.topAnchor.constraint(equalTo: content.topAnchor),
            view.bottomAnchor.constraint(equalTo: content.bottomAnchor)
        ])
        presented = view
        wrapButton.isHidden = !supportsWordWrap
        if view === editorScroll {
            content.layoutSubtreeIfNeeded()
            syncEditorSize()
        }
        if hadFocus { window?.makeFirstResponder(preferredResponder) }
    }

    private func showLoading(_ message: String) {
        stateIcon.image = NSImage(systemSymbolName: "hourglass", accessibilityDescription: nil)
        stateTitle.stringValue = message
        stateDetail.stringValue = ""
        retryButton.isHidden = true
        present(stateContainer)
    }

    private func showState(title: String, detail: String, code: String?, retry: Bool) {
        stateIcon.image = NSImage(systemSymbolName: code == nil ? "doc" : "exclamationmark.triangle", accessibilityDescription: nil)
        stateTitle.stringValue = title
        stateDetail.stringValue = detail + (code.map { "\n(" + $0 + ")" } ?? "")
        retryButton.isHidden = !retry
        present(stateContainer)
    }

    private static func heading(for code: String) -> String {
        switch code {
        case "not_found": return "File not found"
        case "permission_denied": return "Permission denied"
        case "too_large": return "File is too large"
        case "binary_file": return "Not a text file"
        case "invalid_utf8": return "Not valid UTF-8"
        case "is_directory": return "This is a folder"
        case "not_regular_file": return "Not a regular file"
        case "path_changed": return "The file moved"
        case "content_mismatch": return "File type mismatch"
        case "file_worker_unavailable", "busy", "timeout": return "File service is busy"
        case "image_too_large": return "Image is too large"
        case "image_undecodable", "pdf_undecodable", "pdf_empty": return "Cannot show this file"
        case "pdf_locked": return "PDF is locked"
        default: return "Cannot open this file"
        }
    }

    // MARK: - Snapshot reconciliation

    func update(surface: Surface, files: FilesSnapshot) {
        guard !closed else { return }
        if surface.title != title {
            title = surface.title
            editor.setAccessibilityLabel("File editor: " + title)
            markdownView.setAccessibilityLabel("Rendered Markdown: " + title)
            setAccessibilityLabel("File panel: " + title)
        }
        lastModelDirty = surface.file?.dirty ?? false
        latestEpoch = files.epoch
        workerStalledMs = files.worker.state == "ok" ? nil : files.worker.stalledMs
        guard mode != nil else {
            failureCode = "unsupported_mode"
            showState(title: "Unsupported file type",
                      detail: "This version of Zmux cannot show “" + rawMode + "” files.", code: "unsupported_mode", retry: false)
            refreshHeader()
            return
        }
        let entry = files.entries.first { $0.surfaceId == surfaceID }
        if let attachedEpoch, attachedEpoch != files.epoch { epochChanged() }
        switch phase {
        case .idle: beginAttach()
        case .ready:
            reconcileExternal(entry)
            reconcileOutcome(entry)
        case .attaching, .failed: break
        }
        reconcileDirty()
        refreshHeader()
    }

    /// The file service restarted (session restore): every old owner is stale. Clean views simply
    /// re-attach; a view with unsaved text keeps it and says so, never silently discarding it.
    private func epochChanged() {
        ownerToken = nil
        attachedEpoch = nil
        observeTask?.cancel()
        observeTask = nil
        if isLoaded && hasUnsavedChanges {
            staleOwner = true
            setBanner(.ownerLost("The file service restarted. Your unsaved text is still here but cannot be saved. Reload from disk to discard it, or copy it out first."))
        } else {
            phase = .idle
        }
    }

    private func reconcileExternal(_ entry: FileEntrySnapshot?) {
        guard let entry else { return }
        let unsaved = hasUnsavedChanges
        switch model.externalStateReported(state: entry.state, diskGeneration: entry.diskGeneration, errorCode: entry.errorCode,
                                           attached: ownerToken != nil, busy: busy, hasUnsavedChanges: unsaved) {
        case .none: break
        case .reloadAutomatically: startReload(discard: true, automatic: true)
        case .show(let value): setBanner(value)
        case .clearBanner: setBanner(.none)
        }
    }

    /// A save whose acknowledgement was lost reports its outcome through a later snapshot.
    private func reconcileOutcome(_ entry: FileEntrySnapshot?) {
        guard let last = entry?.lastOp,
              let resolved = model.saveOutcomeBanner(lastOperation: last.opId, state: last.state, code: last.code) else { return }
        setBanner(resolved)
    }

    // MARK: - Loading

    private func send(_ request: FileHostRequest, body: Data = Data()) async -> Result<FileHostReply, FileHostFailure> {
        guard let owner else { return .failure(FileHostFailure(code: "closed", message: "The application is shutting down")) }
        do { return .success(try await owner.fileCall(request, body: body)) }
        catch let failure as FileHostFailure { return .failure(failure) }
        catch { return .failure(FileHostFailure(code: "internal", message: error.localizedDescription, outcomeUnknown: true)) }
    }

    /// Marks the panel busy and builds the attach request, or returns nil when an attach cannot
    /// start (another operation is running, the tab is closing, or no snapshot has arrived yet).
    private func prepareAttach() -> FileHostRequest? {
        guard !busy, !closed, let epoch = latestEpoch else { return nil }
        busy = true
        suspendEditing()
        phase = .attaching
        if !isLoaded { showLoading("Opening " + title + "…") }
        return FileHostRequest.attach(epoch: epoch, surface: surfaceID, host: hostID)
    }

    private func finishAttach(_ request: FileHostRequest) async {
        let result = await send(request)
        busy = false
        resumeEditing()
        if closed {
            // The tab closed while the attach was in flight; release the binding it created.
            if case .success(let reply) = result, let token = reply.header.owner { owner?.filePanelDetach(token) }
            return
        }
        switch result {
        case .success(let reply): apply(reply)
        case .failure(let failure): loadFailed(failure)
        }
        refreshHeader()
    }

    private func beginAttach() {
        guard let request = prepareAttach() else { return }
        Task { [weak self] in await self?.finishAttach(request) }
    }

    /// Re-attaches a loaded panel whose binding was lost and waits for the outcome. The caller has
    /// already confirmed that unsaved text may be replaced; a failure leaves the text, the
    /// `ownerLost` banner and `File > Reload from Disk` in place so the person can try again.
    private func reattach() async {
        guard let request = prepareAttach() else {
            if !busy && !closed { owner?.filePanelReport("The file service state is not available yet; try again in a moment.") }
            return
        }
        await finishAttach(request)
    }

    /// Retry from the failed-state button and the load-failed banner. A panel with a lost binding
    /// recovers through `reloadFromDisk`, which also asks before replacing unsaved text.
    func retry() {
        guard !busy, !closed else { return }
        phase = .idle
        if case .loadFailed = banner { setBanner(.none) }
        beginAttach()
    }
    @objc private func retryTapped() { retry() }

    private func apply(_ reply: FileHostReply) {
        let header = reply.header
        guard let mode, let token = header.owner, let replyMode = header.mode, let newGeneration = header.generation,
              replyMode == mode.rawValue else {
            loadFailed(FileHostFailure(code: "invalid_reply", message: "The file service described a different file than this tab opened."))
            return
        }
        ownerToken = token
        attachedEpoch = token.epoch
        model.attached(generation: newGeneration)
        writable = header.writable ?? false
        staleOwner = false
        observeGaveUp = false
        phase = .ready
        failureCode = nil
        switch mode {
        case .text, .markdown: applyText(reply.body)
        case .image: applyImage(reply.body)
        case .pdf: applyPDF(reply.body)
        }
        if banner.isObsoleteAfterLoad { setBanner(.none) }
    }

    private func applyText(_ body: Data) {
        guard let text = String(data: body, encoding: .utf8) else {
            showFailure(FilePanelFailure(code: "invalid_utf8", message: "The file is not valid UTF-8 text and cannot be shown."))
            return
        }
        suppressEdits = true
        let selection = editor.selectedRange()
        editor.string = text
        editor.setSelectedRange(NSRange(location: min(selection.location, text.utf16.count), length: 0))
        editor.undoManager?.removeAllActions()
        suppressEdits = false
        dirtyChanged(model.adopt(text: text))
        isLoaded = true
        refreshEditable()
        applyWrapSettings()
        if mode == .markdown { applyMarkdownMode() } else { present(editorScroll) }
        refreshHeader()
    }

    private func applyImage(_ data: Data) {
        decodeGeneration &+= 1
        let generation = decodeGeneration
        decodeTask?.cancel()
        if !isLoaded { showLoading("Decoding image…") }
        decodeTask = Task { [weak self] in
            let result = await Task.detached(priority: .userInitiated) { FileImageDecoder.decode(data) }.value
            guard let self, !Task.isCancelled, !self.closed, generation == self.decodeGeneration else { return }
            switch result {
            case .success(let decoded):
                self.imageView.image = NSImage(cgImage: decoded.image, size: NSSize(width: decoded.width, height: decoded.height))
                self.imageWidth = decoded.width
                self.imageHeight = decoded.height
                self.isLoaded = true
                self.present(self.imageContainer)
                self.refreshHeader()
            case .failure(let failure):
                self.showFailure(failure)
            }
        }
    }

    private func applyPDF(_ data: Data) {
        guard let document = PDFDocument(data: data) else {
            showFailure(FilePanelFailure(code: "pdf_undecodable", message: "This PDF could not be read. The file may be corrupt."))
            return
        }
        guard !document.isLocked else {
            showFailure(FilePanelFailure(code: "pdf_locked", message: "This PDF is password protected and cannot be previewed."))
            return
        }
        guard document.pageCount > 0 else {
            showFailure(FilePanelFailure(code: "pdf_empty", message: "This PDF has no pages."))
            return
        }
        pdfView.document = document
        pdfPageCount = document.pageCount
        isLoaded = true
        present(pdfView)
        refreshHeader()
    }

    private func loadFailed(_ failure: FileHostFailure) {
        phase = .failed
        // A loaded panel without a working binding, or one holding unsaved text, never turns a
        // failed re-attach into a dead end: it stays stale, keeps its text and keeps offering
        // Reload from Disk (retried, and confirmed before text is replaced) and Copy Text.
        if isLoaded && (failure.isOwnerLoss || staleOwner || ownerToken == nil || hasUnsavedChanges) {
            if failure.isOwnerLoss { ownerToken = nil }
            staleOwner = true
            if failure.isOwnerLoss {
                setBanner(.ownerLost("This view is no longer attached to the file service (" + failure.code + "). Reload from disk to continue; unsaved text is kept until then."))
            } else {
                setBanner(.ownerLost("Could not reconnect to the file service: " + failure.message + " (" + failure.code + "). Your text is kept. Reload from disk to try again, or copy it out first."))
            }
            return
        }
        showFailure(FilePanelFailure(code: failure.code, message: failure.message))
    }

    private func showFailure(_ failure: FilePanelFailure) {
        if isLoaded {
            setBanner(.loadFailed(failure.message + " (" + failure.code + ")"))
        } else {
            phase = .failed
            failureCode = failure.code
            showState(title: Self.heading(for: failure.code), detail: failure.message, code: failure.code, retry: true)
        }
        refreshHeader()
    }

    // MARK: - Reload

    /// `File > Reload from Disk`, and every banner button that reloads. This is the one manual
    /// route that replaces the buffer, so it is the one place that asks before discarding
    /// unsaved text.
    func reloadFromDisk() {
        if phase == .failed && !isLoaded { retry(); return }
        guard canReload else {
            if busy && !closed { owner?.filePanelReport("Another file operation is still running; try again in a moment.") }
            return
        }
        Task { [weak self] in
            guard let self else { return }
            if self.hasUnsavedChanges { guard await self.confirmDiscard() else { return } }
            await self.performReload(discard: true, automatic: false)
        }
    }

    private func confirmDiscard() async -> Bool {
        if let confirm = discardConfirmation { return await confirm() }
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = "Reload “" + title + "” from disk?"
        alert.informativeText = "Your unsaved changes will be discarded."
        alert.addButton(withTitle: "Cancel")
        alert.addButton(withTitle: "Discard and Reload")
        let response = await presentSheet(alert, in: window)
        return response == .alertSecondButtonReturn
    }

    private func startReload(discard: Bool, automatic: Bool) {
        Task { [weak self] in
            guard let self else { return }
            await self.performReload(discard: discard, automatic: automatic)
        }
    }

    /// `discard` is always true: a clean view has nothing to lose and a dirty view only reloads
    /// after the person chose to discard (or the reload is deferred, see below).
    private func performReload(discard: Bool, automatic: Bool) async {
        guard !busy, !closed else {
            if busy && !closed && !automatic { owner?.filePanelReport("Another file operation is still running; try again in a moment.") }
            return
        }
        guard let token = ownerToken else {
            // The binding is gone (restart, lost owner): attach again. A bound host with a dirty
            // model is refused by Rust, which is why a kept binding reloads instead (below).
            await reattach()
            return
        }
        // One automatic attempt per observed disk generation; a failed attempt shows a banner and
        // waits for the next external change or an explicit Reload instead of looping.
        if automatic { model.automaticReloadStarted() }
        busy = true
        suspendEditing()
        refreshHeader()
        let result = await send(.reload(token, discard: discard))
        busy = false
        resumeEditing()
        guard !closed else { return }
        switch result {
        case .success(let reply):
            // An automatic reload that finds text typed after it was scheduled but before it began
            // (editing is read-only once it runs, see `suspendEditing`) keeps that
            // text; every other reply replaces the buffer.
            let unsaved = hasUnsavedChanges
            switch model.reloadReplied(automatic: automatic, hasUnsavedChanges: unsaved, generation: reply.header.generation) {
            case .replaceBuffer: apply(reply)
            case .keepText(let notice): setBanner(notice)
            }
        case .failure(let failure):
            if failure.isOwnerLoss {
                ownerToken = nil
                staleOwner = true
                setBanner(.ownerLost("This view is no longer attached to the file service (" + failure.code + "). Reload from disk to continue; unsaved text is kept until then."))
            } else {
                setBanner(.loadFailed("Reload failed: " + failure.message + " (" + failure.code + ")"))
            }
        }
        refreshHeader()
    }

    // MARK: - Save

    /// `File > Save`.
    func save() {
        Task { [weak self] in
            guard let self else { return }
            _ = await self.performSave(force: false)
        }
    }

    private var saveUnavailableReason: String {
        if !(mode?.isEditableText ?? false) { return "“" + title + "” is a preview and cannot be saved." }
        if !writable { return "“" + title + "” is read-only; it cannot be saved." }
        if busy { return "Another file operation is still running; try again in a moment." }
        if ownerToken == nil { return "“" + title + "” is not attached to the file service; reload it first." }
        return "There are no changes to save."
    }

    /// Saves the buffer. Returns true only when the file service confirmed the write.
    @discardableResult
    func performSave(force: Bool) async -> Bool {
        guard canSave else {
            owner?.filePanelReport(saveUnavailableReason)
            return false
        }
        if model.saveNeedsConflictPrompt(force: force) {
            switch await chooseConflict(FileConflict(kind: "modified", diskSha256: nil, diskSize: nil)) {
            case .overwrite: return await performSave(force: true)
            case .reload:
                await performReload(discard: true, automatic: false)
                return false
            case .cancel: return false
            }
        }
        guard let token = ownerToken else { return false }
        let text = editor.string
        let body = model.savePayload(for: text)
        busy = true
        refreshHeader()
        let result = await send(.save(token, baseGeneration: model.generation, force: force), body: body)
        busy = false
        guard !closed else { return false }
        switch result {
        case .success(let reply):
            dirtyChanged(model.saved(written: text, current: editor.string, generation: reply.header.generation))
            if banner.isObsoleteAfterSave { setBanner(.none) }
            refreshHeader()
            return true
        case .failure(let failure):
            return await saveFailed(failure)
        }
    }

    private func saveFailed(_ failure: FileHostFailure) async -> Bool {
        defer { refreshHeader() }
        if failure.code == "conflict" {
            switch await chooseConflict(failure.conflict) {
            case .overwrite: return await performSave(force: true)
            case .reload:
                await performReload(discard: true, automatic: false)
                return false
            case .cancel: return false
            }
        }
        if failure.outcomeUnknown {
            setBanner(.saveUnknown(failure.opId ?? 0))
            return false
        }
        if failure.isOwnerLoss || failure.code == "stale_view" {
            ownerToken = failure.code == "stale_view" ? ownerToken : nil
            staleOwner = true
            setBanner(.ownerLost("The file service no longer matches this editor (" + failure.code + "). Your text is kept. Reload from disk to discard it, or copy it out first."))
            return false
        }
        setBanner(.message("Could not save: " + failure.message + " (" + failure.code + ")"))
        return false
    }

    private func chooseConflict(_ conflict: FileConflict?) async -> FileConflictChoice {
        let value = conflict ?? FileConflict(kind: "modified", diskSha256: nil, diskSize: nil)
        if let chooser = conflictChooser { return await chooser(value) }
        let alert = NSAlert()
        alert.alertStyle = .warning
        if value.kind == "deleted" {
            alert.messageText = "“" + title + "” was deleted on disk."
            alert.informativeText = "Recreate the file with your text, or cancel and keep editing."
            alert.addButton(withTitle: "Cancel")
            alert.addButton(withTitle: "Recreate File")
            let answer = await presentSheet(alert, in: window)
            return answer == .alertSecondButtonReturn ? .overwrite : .cancel
        }
        alert.messageText = "“" + title + "” changed on disk."
        alert.informativeText = "Overwrite replaces the version on disk with yours. Reload discards your edits and loads the version on disk."
        alert.addButton(withTitle: "Cancel")
        alert.addButton(withTitle: "Reload from Disk")
        alert.addButton(withTitle: "Overwrite")
        let response = await presentSheet(alert, in: window)
        switch response {
        case .alertSecondButtonReturn: return .reload
        case .alertThirdButtonReturn: return .overwrite
        default: return .cancel
        }
    }

    // MARK: - Editing gate

    private func suspendEditing() {
        model.suspendEditing()
        refreshEditable()
    }

    private func resumeEditing() {
        model.resumeEditing()
        refreshEditable()
    }

    private func refreshEditable() {
        editor.isEditable = model.isEditable(loaded: isLoaded, writable: writable)
    }

    // MARK: - Dirty tracking

    private func editorDidChange() {
        guard !suppressEdits else { return }
        dirtyChanged(model.textEdited(editor.string))
    }

    /// The model reported that the dirty bit changed: refresh the chrome and mirror it to Rust.
    private func dirtyChanged(_ changed: Bool) {
        guard changed else { return }
        refreshHeader()
        owner?.filePanelStateChanged(surfaceID)
        reconcileDirty()
    }

    private var observeNeeded: Bool {
        dirtyGate.observationNeeded(attached: ownerToken != nil, editable: mode?.isEditableText ?? false, closed: closed,
                                    gaveUp: observeGaveUp, isDirty: isDirty, modelDirty: lastModelDirty)
    }

    private func reconcileDirty() {
        guard observeNeeded, observeTask == nil else { return }
        observeLoopID += 1
        let loopID = observeLoopID
        observeTask = Task { [weak self] in
            guard let self else { return }
            await self.runObserveLoop(id: loopID)
        }
    }

    /// The model's dirty bit is a mirror of this view's text. `observe` is an idempotent
    /// absolute-state write, so it is retried (bounded rate, no count limit) until a snapshot
    /// shows the model agrees; a refused owner stops the loop and says so.
    private func runObserveLoop(id: Int) async {
        var delay = 250
        var confirmDelay = 150
        var unconfirmed = 0
        loop: while !Task.isCancelled && observeNeeded {
            guard let token = ownerToken else { break loop }
            let wanted = isDirty
            let result = await send(.observe(token, dirty: wanted))
            switch result {
            case .success:
                delay = 250
                do { try await Task.sleep(for: .milliseconds(confirmDelay)) } catch { break loop }
                if observeNeeded && isDirty == wanted {
                    // Acknowledged but not yet visible in a snapshot. A lost observation is a
                    // data-loss hazard, so this never gives up: it only slows to a bounded rate
                    // and says so once it has been waiting for a while.
                    unconfirmed += 1
                    confirmDelay = min(confirmDelay * 2, 2_000)
                    if unconfirmed >= 8 && banner == Banner.none { setBanner(.dirtyUnconfirmed) }
                } else {
                    unconfirmed = 0
                    confirmDelay = 150
                }
            case .failure(let failure):
                if failure.isRetryableObservation {
                    do { try await Task.sleep(for: .milliseconds(delay)) } catch { break loop }
                    delay = min(delay * 2, 2_000)
                } else {
                    observeGaveUp = true
                    if failure.isOwnerLoss {
                        ownerToken = nil
                        staleOwner = true
                        setBanner(.ownerLost("This view is no longer attached to the file service (" + failure.code + "). Unsaved text is kept; reload from disk to continue."))
                    } else {
                        setBanner(.message("Could not record unsaved changes: " + failure.message + " (" + failure.code + ")"))
                    }
                }
            }
        }
        if !observeNeeded && banner == .dirtyUnconfirmed { setBanner(.none) }
        if observeLoopID == id {
            observeTask = nil
            if observeNeeded { reconcileDirty() }
        }
    }

    // MARK: - Close support

    /// Treats the buffer as saved/abandoned so the tab can close without a second prompt.
    func discardLocalChanges() {
        dirtyChanged(model.adopt(text: editor.string))
    }

    /// Don't Save was chosen for this panel and is about to be settled. From here until
    /// `releaseDiscard` or `applyDiscard` (both wait for the core's answer to the close) no
    /// snapshot may start a dirty-mirror observation: the model is told the text is clean while
    /// the panel still holds it. Synchronous, so it can run before the first awaited step of a
    /// multi-panel close.
    func holdDiscard() {
        dirtyGate.hold()
        observeTask?.cancel()
    }

    /// True from Don't Save being settled until the core has closed the tab or refused to.
    var isDiscardPending: Bool { dirtyGate.isDiscarding }

    /// The close did not happen (the core refused it, or it never reached the core): nothing is
    /// discarded, so the mirror resumes and re-marks the model dirty.
    func releaseDiscard() {
        dirtyGate.release()
        reconcileDirty()
    }

    /// The core has closed the tab. The text is dropped from dirty tracking and the hold is
    /// lifted in the same synchronous step, so no snapshot sees one without the other. A panel
    /// that was already retired with its tab only gives its hold back.
    func applyDiscard() {
        guard !closed else {
            dirtyGate.release()
            return
        }
        discardLocalChanges()
        dirtyGate.release()
        reconcileDirty()
    }

    /// Makes the model's dirty bit equal this view's before an app-initiated close, so the core
    /// close guard cannot race a pending observation. False means the close must not proceed.
    /// `assumingDiscarded` is for a panel whose Don't Save has been chosen but not yet applied:
    /// the model is told the text is clean while the panel still holds the text, so a later
    /// abort of the close loses nothing (see `resumeDirtyMirror`).
    func settleDirtyMirror(assumingDiscarded: Bool = false) async -> Bool {
        guard mode?.isEditableText ?? false, let token = ownerToken else { return true }
        // A running observation may have a write in flight. Wait for it to finish so it cannot
        // land after (and undo) the write below; the snapshot cannot be trusted to show it.
        let running = observeTask
        running?.cancel()
        observeTask = nil
        if let running { await running.value }
        let wanted = assumingDiscarded ? false : isDirty
        // A held panel's loop was cancelled by the hold, possibly long before this call, so it may
        // already have finished with its write landed but not yet visible in `lastModelDirty`.
        guard DirtyMirrorGate.closeWriteNeeded(wanted: wanted, modelDirty: lastModelDirty,
                                               loopMayHaveWritten: running != nil || assumingDiscarded) else { return true }
        switch await send(.observe(token, dirty: wanted)) {
        case .success:
            lastModelDirty = wanted
            return true
        case .failure(let failure):
            owner?.filePanelReport("Could not record unsaved changes: " + failure.message + " (" + failure.code + "); the tab was not closed.")
            return false
        }
    }

    /// Restarts the dirty-mirror observation after a close was aborted part way through
    /// `settleDirtyMirror`, which cancels it and may have recorded a state this panel no longer
    /// has. A no-op when the model already agrees with the panel.
    func resumeDirtyMirror() { reconcileDirty() }

    /// Releases tasks and decoded content without talking to the file service (shutdown).
    func shutdown() {
        guard !closed else { return }
        closed = true
        observeTask?.cancel(); observeTask = nil
        decodeTask?.cancel(); decodeTask = nil
        markdownTask?.cancel(); markdownTask = nil
        pdfView.document = nil
        imageView.image = nil
    }

    /// The tab is gone (or this view was replaced): release the Rust binding and the content.
    func close() {
        let token = ownerToken
        shutdown()
        ownerToken = nil
        if let token { owner?.filePanelDetach(token) }
        removeFromSuperview()
    }

    // MARK: - Markdown, wrap and header

    func toggleMarkdownSource() {
        guard mode == .markdown, isLoaded else { return }
        markdownShowsSource.toggle()
        applyMarkdownMode()
        if markdownShowsSource { window?.makeFirstResponder(editor) }
    }

    @objc private func modeChanged(_ sender: NSSegmentedControl) {
        let wantsSource = sender.selectedSegment == 1
        guard wantsSource != markdownShowsSource, isLoaded else {
            modeControl.selectedSegment = markdownShowsSource ? 1 : 0
            return
        }
        markdownShowsSource = wantsSource
        applyMarkdownMode()
    }

    private func applyMarkdownMode() {
        modeControl.selectedSegment = markdownShowsSource ? 1 : 0
        if markdownShowsSource {
            markdownTask?.cancel()
            markdownGeneration &+= 1
            present(editorScroll)
        } else {
            renderMarkdown()
        }
    }

    private func renderMarkdown() {
        let source = editor.string
        markdownGeneration &+= 1
        let generation = markdownGeneration
        markdownTask?.cancel()
        if source.isEmpty {
            markdownRendered = false
            markdownView.string = ""
            showState(title: "Empty Markdown file", detail: "There is nothing to preview. Switch to Source to start writing.", code: nil, retry: false)
            return
        }
        if markdownView.string.isEmpty { showLoading("Rendering Markdown…") } else { present(markdownScroll) }
        markdownTask = Task { [weak self] in
            let rendered = await Task.detached(priority: .userInitiated) { FileMarkdownRenderer.render(source) }.value
            guard let self, !Task.isCancelled, !self.closed, generation == self.markdownGeneration,
                  self.mode == .markdown, !self.markdownShowsSource else { return }
            self.markdownView.textStorage?.setAttributedString(rendered.text)
            self.markdownRendered = true
            self.present(self.markdownScroll)
            if let failure = rendered.failure { self.setBanner(.message(failure)) }
            self.refreshHeader()
        }
    }

    func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
        let url = (link as? URL) ?? (link as? String).flatMap { URL(string: $0) }
        if let url, FileMarkdownRenderer.isAllowedLink(url) { _ = NSWorkspace.shared.open(url) }
        return true
    }

    func toggleWordWrap() {
        wordWrap.toggle()
        UserDefaults.standard.set(wordWrap, forKey: Self.wrapKey)
        applyWrapSettings()
    }
    @objc private func wrapTapped() { toggleWordWrap() }

    private func applyWrapSettings() {
        guard let container = editor.textContainer else { return }
        editor.isVerticallyResizable = true
        editor.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        if wordWrap {
            editor.isHorizontallyResizable = false
            editor.autoresizingMask = [.width]
            container.widthTracksTextView = true
            container.containerSize = NSSize(width: max(editorScroll.contentSize.width, 1), height: CGFloat.greatestFiniteMagnitude)
            editorScroll.hasHorizontalScroller = false
        } else {
            editor.isHorizontallyResizable = true
            editor.autoresizingMask = []
            container.widthTracksTextView = false
            container.containerSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
            editorScroll.hasHorizontalScroller = true
        }
        wrapButton.contentTintColor = wordWrap ? .controlAccentColor : .secondaryLabelColor
        wrapButton.setAccessibilityValue(wordWrap ? "On" : "Off")
        editor.needsLayout = true
    }

    private func setText(_ label: NSTextField, _ value: String) {
        if label.stringValue != value { label.stringValue = value }
    }

    private func refreshHeader() {
        setText(nameLabel, title + (hasUnsavedChanges ? " — Edited" : ""))
        nameLabel.toolTip = path
        var parts: [String] = []
        if !isLoaded {
            parts.append(failureCode == nil ? "Opening…" : "Unavailable")
        } else {
            switch mode {
            case .text?: parts.append(editor.string.isEmpty ? "Empty file" : "Text")
            case .markdown?: parts.append(editor.string.isEmpty ? "Empty Markdown file" : "Markdown")
            case .image?: parts.append("Image \(imageWidth) × \(imageHeight)")
            case .pdf?: parts.append("PDF · \(pdfPageCount) page" + (pdfPageCount == 1 ? "" : "s"))
            case nil: parts.append("Unsupported")
            }
            if (mode?.isEditableText ?? false) && !writable { parts.append("Read-only") }
        }
        if busy { parts.append("Working…") }
        if let stalled = workerStalledMs { parts.append("File service busy for \(stalled / 1000)s") }
        setText(statusLabel, parts.joined(separator: " · "))
    }

    // MARK: - Banner

    private struct BannerStyle {
        let text: String
        let warning: Bool
        let primary: String?
        let secondary: String?
    }

    private func style(for banner: Banner) -> BannerStyle? {
        switch banner {
        case .none:
            return nil
        case .changedOnDisk:
            return BannerStyle(text: "This file changed on disk. Reload loads the new version and discards your edits; Keep Mine continues editing.",
                               warning: true, primary: "Reload", secondary: "Keep Mine")
        case .deletedOnDisk:
            let kind = mode?.isEditableText == true ? "Saving recreates it." : "The preview shows the last loaded version."
            return BannerStyle(text: "This file was deleted on disk. " + kind, warning: true, primary: "Dismiss", secondary: nil)
        case .unreadable(let code):
            return BannerStyle(text: "This file can no longer be read (" + code + ").", warning: false, primary: "Reload", secondary: nil)
        case .ownerLost(let message):
            return BannerStyle(text: message, warning: false, primary: "Reload from Disk", secondary: "Copy Text")
        case .saveUnknown(let operation):
            if operation == 0 {
                return BannerStyle(text: "The save outcome is unknown. Reload from disk to see what was written.",
                                   warning: true, primary: "Reload", secondary: "Dismiss")
            }
            return BannerStyle(text: "The save did not confirm in time (operation \(operation)). Waiting for the file service to report its outcome; do not save again yet.",
                               warning: true, primary: nil, secondary: nil)
        case .saveResolved:
            return BannerStyle(text: "The save finished after a timeout. Reload from disk to continue.", warning: true, primary: "Reload", secondary: "Dismiss")
        case .loadFailed(let message):
            return BannerStyle(text: message, warning: false, primary: hasUnsavedChanges ? "Dismiss" : "Retry", secondary: nil)
        case .message(let text):
            return BannerStyle(text: text, warning: false, primary: "Dismiss", secondary: nil)
        case .dirtyUnconfirmed:
            return BannerStyle(text: "Still recording your unsaved changes with the file service. Save or copy your text before closing.",
                               warning: true, primary: nil, secondary: nil)
        }
    }

    private func setBanner(_ value: Banner) {
        guard model.setBanner(value) else { return }
        if let style = style(for: value) {
            bannerLabel.stringValue = style.text
            bannerLabel.toolTip = style.text
            bannerLabel.setAccessibilityLabel(style.text)
            bannerView.layer?.backgroundColor = (style.warning ? NSColor.systemOrange : NSColor.systemRed).withAlphaComponent(0.2).cgColor
            bannerPrimary.title = style.primary ?? ""
            bannerPrimary.isHidden = style.primary == nil
            bannerSecondary.title = style.secondary ?? ""
            bannerSecondary.isHidden = style.secondary == nil
            bannerView.isHidden = false
            bannerHeight?.constant = 32
        } else {
            bannerView.isHidden = true
            bannerHeight?.constant = 0
        }
        owner?.filePanelStateChanged(surfaceID)
    }

    @objc private func primaryTapped() { bannerAction(primary: true) }
    @objc private func secondaryTapped() { bannerAction(primary: false) }

    private func bannerAction(primary: Bool) {
        let unsaved = hasUnsavedChanges
        switch model.bannerAction(primary: primary, hasUnsavedChanges: unsaved) {
        case .reloadFromDisk:
            // Replaces the buffer, so it asks first when there is unsaved text.
            reloadFromDisk()
        case .retry: retry()
        case .copyText: copyTextToPasteboard()
        case .dismiss: setBanner(.none)
        case .nothing: break
        }
    }

    /// Presses a banner button through the control itself (the in-app smoke); false when that
    /// button is not currently shown.
    @discardableResult
    func clickBannerButton(primary: Bool) -> Bool {
        let button = primary ? bannerPrimary : bannerSecondary
        guard !bannerView.isHidden, !button.isHidden else { return false }
        button.performClick(nil)
        return true
    }

    func copyTextToPasteboard() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(editor.string, forType: .string)
    }
}

extension FilePanelView: FileCloseParticipant {}

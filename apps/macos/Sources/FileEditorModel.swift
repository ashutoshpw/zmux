import Foundation

/// What a file panel's banner says. Lives here, away from AppKit, so the model below and the macOS
/// fixture (`scripts/check-file-editor-model`) can name it.
enum FileBanner: Equatable, Sendable {
    case none
    case changedOnDisk(UInt64)
    case deletedOnDisk
    case unreadable(String)
    case ownerLost(String)
    case saveUnknown(UInt64)
    case saveResolved
    case loadFailed(String)
    case message(String)
    /// Informational: the dirty bit has not been confirmed yet; the observation keeps retrying.
    case dirtyUnconfirmed

    /// The banners that report the file's on-disk state and go away once it is in sync again.
    var isExternal: Bool {
        switch self {
        case .changedOnDisk, .deletedOnDisk, .unreadable: return true
        default: return false
        }
    }

    /// Left over from an attach or reload whose reply replaced the buffer, so it no longer applies.
    var isObsoleteAfterLoad: Bool {
        switch self {
        case .loadFailed, .ownerLost, .saveResolved: return true
        default: return isExternal
        }
    }

    /// Left over from a save the file service confirmed.
    var isObsoleteAfterSave: Bool {
        if case .saveResolved = self { return true }
        return isExternal
    }
}

/// What the panel does about a snapshot's report of the file's state on disk.
enum FileExternalReaction: Equatable, Sendable {
    case none
    /// The buffer is clean: load the new disk version without asking.
    case reloadAutomatically
    case show(FileBanner)
    case clearBanner
}

/// What happens to the buffer when a reload request is answered.
enum FileReloadDisposition: Equatable, Sendable {
    /// Replace the buffer with the reply.
    case replaceBuffer
    /// Keep the text that was typed meanwhile and show this banner.
    case keepText(FileBanner)
}

/// What pressing a banner button does. Buttons that replace the buffer never do it themselves:
/// `reloadFromDisk` is the one route and it asks before discarding unsaved text.
enum FileBannerAction: Equatable, Sendable {
    case reloadFromDisk
    case retry
    case copyText
    case dismiss
    case nothing
}

/// Every data-loss-relevant decision of a file editor, with no AppKit in it: what counts as
/// unsaved, which bytes a save writes, what a save or reload reply does to the dirty state, when the
/// editor is read-only and which banner button does what. `FilePanelView` holds one and applies
/// the answers; the macOS fixture drives this same type (`scripts/check-file-editor-model`).
///
/// Nothing here waits, sleeps or touches the file service. Callers pass in the facts that live in
/// the view (is the file an editable text, is the view attached, is a request running).
struct FileEditorModel: Sendable, Equatable {
    /// The disk generation the view last loaded or saved; the base of the next save.
    private(set) var generation: UInt64 = 0
    /// The text the view last loaded or saved.
    private(set) var baseline = ""
    private(set) var isDirty = false
    /// True while a reload or an attach is in flight. Its reply replaces the whole buffer, so the
    /// editor is read-only for that time.
    private(set) var editingSuspended = false
    /// True when the disk changed while the view had unsaved edits and the edits were kept.
    private(set) var externalPending = false
    private(set) var banner = FileBanner.none

    private(set) var autoReloadedDiskGeneration: UInt64 = 0
    private(set) var dismissedDiskGeneration: UInt64 = 0
    private(set) var latestDiskGeneration: UInt64 = 0
    private(set) var dismissedDeleted = false

    // MARK: Dirty tracking

    /// Exact UTF-8 equality. Swift `==` on `String` treats canonically equivalent text (a
    /// precomposed "é" and "e" plus a combining accent) as equal, which would hide an edit that
    /// only changes the normalization form even though it changes the bytes Save writes.
    static func sameBytes(_ lhs: String, _ rhs: String) -> Bool {
        lhs.utf8.elementsEqual(rhs.utf8)
    }

    /// True when `isDirty` changed.
    @discardableResult
    mutating func setDirty(_ value: Bool) -> Bool {
        guard isDirty != value else { return false }
        isDirty = value
        return true
    }

    /// The person edited the buffer (or undid or redid an edit). True when `isDirty` changed.
    @discardableResult
    mutating func textEdited(_ text: String) -> Bool {
        setDirty(!Self.sameBytes(text, baseline))
    }

    /// The buffer now equals what is on disk: a load, or text the person chose to abandon.
    @discardableResult
    mutating func adopt(text: String) -> Bool {
        baseline = text
        return setDirty(false)
    }

    /// Whether closing, quitting or reloading must ask first. Previews have nothing to lose.
    func hasUnsavedChanges(editable: Bool) -> Bool { isDirty && editable }

    // MARK: Save

    func canSave(editable: Bool, writable: Bool, attached: Bool, busy: Bool, staleOwner: Bool) -> Bool {
        editable && writable && attached && !busy && isDirty && !staleOwner
    }

    /// The exact bytes a save sends: the buffer, complete, as UTF-8.
    func savePayload(for text: String) -> Data { Data(text.utf8) }

    /// A save the person did not force must ask first when the disk moved on under their edits.
    func saveNeedsConflictPrompt(force: Bool) -> Bool { externalPending && !force }

    /// The file service confirmed the write of `written`. `current` is the buffer now: the person
    /// may have typed while the save was in flight, and that text stays dirty. True when
    /// `isDirty` changed.
    @discardableResult
    mutating func saved(written: String, current: String, generation reply: UInt64?) -> Bool {
        if let reply { generation = reply }
        baseline = written
        externalPending = false
        return setDirty(!Self.sameBytes(current, written))
    }

    // MARK: Reload and attach

    /// A manual reload replaces the buffer, so it asks first whenever there is unsaved text.
    static func reloadNeedsConfirmation(hasUnsavedChanges: Bool) -> Bool { hasUnsavedChanges }

    /// An attach or reload reply was accepted: its generation is the new base and any kept-edits
    /// state is over.
    mutating func attached(generation value: UInt64) {
        generation = value
        externalPending = false
    }

    mutating func suspendEditing() { editingSuspended = true }
    mutating func resumeEditing() { editingSuspended = false }

    /// Whether the text view accepts typing.
    func isEditable(loaded: Bool, writable: Bool) -> Bool { loaded && writable && !editingSuspended }

    /// One automatic reload per observed disk generation.
    mutating func automaticReloadStarted() { autoReloadedDiskGeneration = latestDiskGeneration }

    /// A reload request succeeded. An automatic reload that finds unsaved text (typed after it was
    /// scheduled) must not replace it: the Rust baseline moved on, so the buffer is remembered as
    /// no longer matching it and Save asks before overwriting what changed on disk.
    mutating func reloadReplied(automatic: Bool, hasUnsavedChanges: Bool, generation reply: UInt64?) -> FileReloadDisposition {
        guard automatic && hasUnsavedChanges else { return .replaceBuffer }
        if let reply { generation = reply }
        externalPending = true
        return .keepText(.changedOnDisk(latestDiskGeneration))
    }

    // MARK: Banner

    /// True when the banner changed.
    @discardableResult
    mutating func setBanner(_ value: FileBanner) -> Bool {
        guard banner != value else { return false }
        banner = value
        return true
    }

    /// A snapshot reported the file's state on disk. `attached` is whether the view holds a
    /// binding and `busy` whether a request is running; neither lets a banner or reload start.
    mutating func externalStateReported(state: String, diskGeneration: UInt64, errorCode: String?,
                                        attached: Bool, busy: Bool, hasUnsavedChanges: Bool) -> FileExternalReaction {
        latestDiskGeneration = diskGeneration
        guard attached, !busy else { return .none }
        switch state {
        case "changed":
            dismissedDeleted = false
            if !hasUnsavedChanges {
                return diskGeneration != autoReloadedDiskGeneration ? .reloadAutomatically : .none
            }
            return diskGeneration != dismissedDiskGeneration ? .show(.changedOnDisk(diskGeneration)) : .none
        case "deleted":
            return dismissedDeleted ? .none : .show(.deletedOnDisk)
        case "unreadable":
            return .show(.unreadable(errorCode ?? "unknown"))
        case "in_sync":
            dismissedDeleted = false
            return banner.isExternal && !externalPending ? .clearBanner : .none
        default:
            return .none
        }
    }

    /// A save whose acknowledgement was lost reports its outcome through a later snapshot.
    func saveOutcomeBanner(lastOperation: UInt64, state: String, code: String?) -> FileBanner? {
        guard case .saveUnknown(let operation) = banner, operation != 0, lastOperation == operation else { return nil }
        switch state {
        case "completed": return .saveResolved
        case "failed": return .message("The save that did not confirm in time failed (" + (code ?? "unknown") + "). Your text is unchanged.")
        default: return nil
        }
    }

    /// What a banner button does. Dismissing remembers what was dismissed so the same disk state
    /// is not announced again.
    mutating func bannerAction(primary: Bool, hasUnsavedChanges: Bool) -> FileBannerAction {
        switch (banner, primary) {
        case (.changedOnDisk, true), (.unreadable, true), (.saveResolved, true), (.ownerLost, true):
            return .reloadFromDisk
        case (.changedOnDisk(let diskGeneration), false):
            dismissedDiskGeneration = diskGeneration
            return .dismiss
        case (.deletedOnDisk, true):
            dismissedDeleted = true
            return .dismiss
        case (.ownerLost, false):
            return .copyText
        case (.saveUnknown(let operation), true) where operation == 0:
            return .reloadFromDisk
        case (.saveUnknown(let operation), false) where operation == 0:
            return .dismiss
        case (.saveResolved, false):
            return .dismiss
        case (.loadFailed, true):
            return hasUnsavedChanges ? .dismiss : .retry
        case (.message, true):
            return .dismiss
        default:
            return .nothing
        }
    }
}

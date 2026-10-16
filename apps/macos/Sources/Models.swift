import Foundation

struct GridSize: Decodable, Sendable { let rows: Int; let cols: Int }
struct TerminalColor: Decodable, Sendable {
    let kind: String
    let index: Int?
    let rgb: [Int]?
    enum CodingKeys: String, CodingKey { case kind, value }
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        kind = try c.decode(String.self, forKey: .kind)
        if kind == "indexed" { index = try c.decode(Int.self, forKey: .value); rgb = nil }
        else if kind == "rgb" { rgb = try c.decode([Int].self, forKey: .value); index = nil }
        else { index = nil; rgb = nil }
    }
}
struct TerminalStyle: Decodable, Sendable {
    let foreground: TerminalColor; let background: TerminalColor
    let bold: Bool; let italic: Bool; let underline: Bool; let inverse: Bool
}
struct TerminalRun: Decodable, Sendable {
    let column: Int; let width: Int; let text: String; let style: TerminalStyle
}
struct TerminalRowCopy: Decodable, Sendable {
    let wrapsToNext: Bool
    let writtenEnd: Int
}
enum TerminalPointerTracking: String, Decodable, Sendable {
    case none, press
    case pressRelease = "press_release"
    case buttonMotion = "button_motion"
    case anyMotion = "any_motion"
}
struct TerminalPointerEvent: Sendable {
    enum Action: String, Sendable { case press, release, motion }
    enum Button: String, Sendable, Hashable { case left, middle, right, none }
    let action: Action
    let button: Button
    let row: Int
    let col: Int
    let modifiers: Int
}
struct Viewport: Decodable, Sendable {
    let size: GridSize; let contents: String; let cursorRow: Int; let cursorCol: Int
    let lines: [[TerminalRun]]; let applicationCursor: Bool; let bracketedPaste: Bool; let hideCursor: Bool
    let title: String?; let workingDirectory: String?
    let scrollbackOffset: Int; let scrollbackLines: Int; let alternateScreen: Bool
    let rowCopy: [TerminalRowCopy]?
    let pointerTracking: TerminalPointerTracking?
    enum CodingKeys: String, CodingKey {
        case size, contents, cursorRow, cursorCol, lines, applicationCursor, bracketedPaste, hideCursor
        case title, workingDirectory, scrollbackOffset, scrollbackLines, alternateScreen, rowCopy, pointerTracking
    }
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        size = try c.decode(GridSize.self, forKey: .size)
        contents = try c.decode(String.self, forKey: .contents)
        cursorRow = try c.decode(Int.self, forKey: .cursorRow)
        cursorCol = try c.decode(Int.self, forKey: .cursorCol)
        lines = try c.decode([[TerminalRun]].self, forKey: .lines)
        applicationCursor = try c.decode(Bool.self, forKey: .applicationCursor)
        bracketedPaste = try c.decode(Bool.self, forKey: .bracketedPaste)
        hideCursor = try c.decode(Bool.self, forKey: .hideCursor)
        title = try c.decodeIfPresent(String.self, forKey: .title)
        workingDirectory = try c.decodeIfPresent(String.self, forKey: .workingDirectory)
        scrollbackOffset = try c.decode(Int.self, forKey: .scrollbackOffset)
        scrollbackLines = try c.decode(Int.self, forKey: .scrollbackLines)
        alternateScreen = try c.decode(Bool.self, forKey: .alternateScreen)
        rowCopy = try c.decodeIfPresent([TerminalRowCopy].self, forKey: .rowCopy)
        pointerTracking = try c.decodeIfPresent(TerminalPointerTracking.self, forKey: .pointerTracking)
        if let rowCopy {
            guard (1...120).contains(size.rows), (1...240).contains(size.cols),
                  lines.count == size.rows, rowCopy.count == size.rows, rowCopy.allSatisfy({ (0...size.cols).contains($0.writtenEnd) }) else {
                throw DecodingError.dataCorruptedError(forKey: .rowCopy, in: c,
                    debugDescription: "Copy metadata must match the bounded viewport rows and written columns")
            }
        }
    }
}
struct TerminalSnapshot: Decodable, Sendable {
    let state: String; let viewport: Viewport; let bytesRead: UInt64
    let exitCode: Int?; let failure: String?
}
struct Frame: Decodable, Sendable {
    let sessionId: UInt64; let processId: UInt32?; let starting: Bool
    let snapshot: TerminalSnapshot?; let failure: String?
}
struct RemoteEndpoint: Decodable, Sendable { let destination: String }
struct UploadProgress: Decodable, Sendable {
    let id: UInt64; let surfaceId: UInt64; let sessionId: UInt64
    let sourceReleased: Bool
    let state: String; let bytesSent: UInt64; let totalBytes: UInt64
    let remotePaths: [String]; let remoteResultUncertain: Bool; let error: String?; let delivery: String; let deliveryError: String?
}
struct CompletedLocalZellijHistory: Decodable, Sendable {
    struct Receipt: Decodable, Sendable {
        struct Exit: Decodable, Sendable {
            enum Kind: String, Decodable, Sendable { case exited, signaled }
            let kind: Kind
            let value: Int
            var label: String {
                switch kind {
                case .exited: return "Process exited with status \(value)"
                case .signaled: return "Process ended by signal \(value)"
                }
            }
        }
        enum ExecutionObservation: Decodable, Sendable {
            case notObserved
            case shellObserved
            case execFailed(errno: Int)
            private enum CodingKeys: String, CodingKey { case kind, stage, errno }
            init(from decoder: any Decoder) throws {
                let c = try decoder.container(keyedBy: CodingKeys.self)
                let kind = try c.decode(String.self, forKey: .kind)
                switch kind {
                case "not_observed", "shell_observed":
                    guard !c.contains(.stage), !c.contains(.errno) else {
                        throw DecodingError.dataCorruptedError(forKey: .kind, in: c,
                            debugDescription: "Observed execution variant has unexpected failure fields")
                    }
                    self = kind == "not_observed" ? .notObserved : .shellObserved
                case "exec_failed":
                    let number = try c.decode(Int.self, forKey: .errno)
                    guard try c.decode(String.self, forKey: .stage) == "configured_shell_exec",
                          number > 0, number <= Int(Int32.max) else {
                        throw DecodingError.dataCorruptedError(forKey: .stage, in: c,
                            debugDescription: "Invalid configured shell launch failure")
                    }
                    self = .execFailed(errno: number)
                default:
                    throw DecodingError.dataCorruptedError(forKey: .kind, in: c,
                        debugDescription: "Unknown execution observation")
                }
            }
        }
        private struct Body: Decodable {
            let version: Int
            let domain: String
            let bindingDigest: String
            let workloadExit: Exit
            let executionObservation: ExecutionObservation?
        }
        private enum CodingKeys: String, CodingKey {
            case executionContract, receipt, version, domain, bindingDigest, workloadExit, executionObservation
        }
        let bindingDigest: String
        let workloadExit: Exit
        let executionObservation: ExecutionObservation?
        let contractVersion: Int

        // This is a presentation projection. Rust owns receipt authentication and
        // persistence validation; decoding a saved value does not authenticate it.
        init(from decoder: any Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            let body: Body
            if c.contains(.executionContract) {
                guard try c.decode(String.self, forKey: .executionContract) == "admitted_lifetime_v2",
                      Set(c.allKeys) == Set([.executionContract, .receipt]) else {
                    throw DecodingError.dataCorruptedError(forKey: .executionContract, in: c,
                        debugDescription: "Unknown or ambiguous execution contract")
                }
                body = try c.decode(Body.self, forKey: .receipt)
                guard body.version == 2, body.domain == "zmux.local-zellij.terminal.v2",
                      body.executionObservation != nil else {
                    throw DecodingError.dataCorruptedError(forKey: .receipt, in: c,
                        debugDescription: "V2 execution receipt requires its version, domain and observation")
                }
                contractVersion = 2
            } else {
                body = try Body(from: decoder)
                guard !c.contains(.receipt), body.version == 1, body.domain == "zmux.local-zellij.terminal.v1",
                      body.executionObservation == nil else {
                    throw DecodingError.dataCorruptedError(forKey: .version, in: c,
                        debugDescription: "Legacy history requires a raw V1 receipt")
                }
                contractVersion = 1
            }
            let bound = body.workloadExit.kind == .exited ? 0...255 : 1...127
            guard bound.contains(body.workloadExit.value), body.bindingDigest.utf8.count == 64,
                  body.bindingDigest.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }) else {
                throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath,
                    debugDescription: "Invalid saved history exit or binding digest"))
            }
            bindingDigest = body.bindingDigest
            workloadExit = body.workloadExit
            executionObservation = body.executionObservation
        }
        var statusLabel: String {
            if case .execFailed(let number)? = executionObservation { return "Shell launch failed (error \(number))" }
            return workloadExit.label
        }
        var executionDetail: String? {
            switch executionObservation {
            case .notObserved?: return "Shell startup was not observed."
            case .execFailed?: return workloadExit.label
            case .shellObserved?, nil: return nil
            }
        }
    }
    struct CapturedViewport: Decodable, Sendable {
        let contents: String
        let truncated: Bool
    }
    let receipt: Receipt
    let viewport: CapturedViewport?
}
struct Surface: Decodable, Sendable { let id: UInt64; let kind: String; let sessionId: UInt64?; let title: String; let cwd: String?; let url: String?; let navigation: UInt64; let browserProfileId: UUID?; let remote: RemoteEndpoint?; let localZellijCompletion: CompletedLocalZellijHistory? }
struct Pane: Decodable, Sendable {
    let id: UInt64; let selectedSurface: UInt64; let surfaces: [Surface]
    var selected: Surface { surfaces.first { $0.id == selectedSurface }! }
    var sessionId: UInt64? { selected.sessionId }
}
indirect enum PaneTree: Decodable, Sendable {
    case leaf(Pane)
    case split(String, PaneTree, PaneTree)
    enum CodingKeys: String, CodingKey { case kind, pane, axis, first, second }
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        if try c.decode(String.self, forKey: .kind) == "leaf" {
            self = .leaf(try c.decode(Pane.self, forKey: .pane))
        } else {
            self = .split(try c.decode(String.self, forKey: .axis), try c.decode(PaneTree.self, forKey: .first), try c.decode(PaneTree.self, forKey: .second))
        }
    }
    var key: String {
        switch self {
        case .leaf(let pane):
            let surfaces = pane.surfaces.map { surface in
                let session = surface.sessionId.map(String.init) ?? "none"
                let completion = surface.localZellijCompletion?.receipt.bindingDigest ?? "live"
                return "\(surface.id):\(session):\(completion)"
            }.joined(separator: ",")
            return "p\(pane.id):\(pane.selectedSurface):" + surfaces
        case .split(let axis, let a, let b): return "\(axis)(\(a.key),\(b.key))"
        }
    }
    var geometryKey: String {
        switch self { case .leaf(let pane): return "p\(pane.id)"; case .split(let axis, let a, let b): return "\(axis)(\(a.geometryKey),\(b.geometryKey))" }
    }
    var splitKeys: [String] {
        switch self { case .leaf: return []; case .split(_, let a, let b): return [geometryKey] + a.splitKeys + b.splitKeys }
    }
    var panes: [Pane] {
        switch self { case .leaf(let pane): return [pane]; case .split(_, let a, let b): return a.panes + b.panes }
    }
}
struct Workspace: Decodable, Sendable { let id: UInt64; let name: String; let focusedPane: UInt64; let root: PaneTree }
struct ClosedItem: Decodable, Sendable { let windowId: UInt64; let id: UInt64; let title: String; let workspaceName: String; let restoring: Bool }
struct DesktopWindow: Decodable, Sendable {
    let id: UInt64
    let workspaceIds: [UInt64]
    let selectedWorkspace: UInt64?
}
struct WorkspaceGroup: Decodable, Sendable {
    let id: UInt64; let windowId: UInt64; let name: String
    let workspaceIds: [UInt64]; let collapsed: Bool; let externalId: String?
}
struct Model: Decodable, Sendable {
    let identityNamespace: UUID; let modelVersion: UInt32; let closedItems: [ClosedItem]
    let revision: UInt64; let focusedWorkspace: UInt64?; let workspaces: [Workspace]
    let windows: [DesktopWindow]; let focusedWindow: UInt64?
    let workspaceGroups: [WorkspaceGroup]
}
struct AttentionNotification: Decodable, Sendable {
    let id: UInt64; let surfaceId: UInt64?; let workspaceId: UInt64; let identityNamespace: UUID
    let title: String; let subtitle: String; let body: String; let source: String
    let timestampMs: UInt64; let read: Bool
    let target: NotificationTarget?
}
struct SessionPersistence: Decodable, Sendable {
    let enabled: Bool
    let startupPending: Bool
    let restoring: Bool
    let autosaveEnabled: Bool
    let lastSavedRevision: UInt64?
    let error: String?
}
struct WorkspaceStatus: Decodable, Sendable, Equatable {
    let key: String
    let value: String
    let icon: String?
    let color: String?
    let priority: Int32
}
struct WorkspaceProgress: Decodable, Sendable, Equatable {
    let value: Double
    let label: String?
}
struct WorkspacePullRequest: Decodable, Sendable, Equatable {
    let number: UInt64
    let url: String
}
struct WorkspaceLog: Decodable, Sendable, Equatable {
    enum Level: String, Decodable, Sendable, Equatable { case info, progress, success, warning, error }
    let sequence: UInt64
    let message: String
    let level: Level
    let source: String?
}
struct WorkspaceTodo: Decodable, Sendable, Equatable {
    enum State: String, Decodable, Sendable, Equatable { case pending, inProgress = "in-progress", completed }
    enum Origin: String, Decodable, Sendable, Equatable { case user, agent }
    let id: UUID
    let text: String
    let state: State
    let origin: Origin
}
struct WorkspaceMetadata: Decodable, Sendable, Equatable {
    let workspaceId: UInt64
    let pullRequest: WorkspacePullRequest?
    let todos: [WorkspaceTodo]
    let statuses: [WorkspaceStatus]
    let progress: WorkspaceProgress?
    let logs: [WorkspaceLog]
    let logsDropped: UInt64

    static func empty(_ workspaceId: UInt64) -> Self {
        Self(workspaceId: workspaceId, pullRequest: nil, todos: [], statuses: [], progress: nil, logs: [], logsDropped: 0)
    }
    var isEmpty: Bool { pullRequest == nil && todos.isEmpty && statuses.isEmpty && progress == nil && logs.isEmpty && logsDropped == 0 }
}
struct BrowserProfile: Decodable, Sendable, Equatable { let id: UUID; let name: String }
struct BrowserProfileReceipt: Decodable, Sendable {
    let intent: UUID?
    let epoch: UUID
    let operationId: UInt64
    let state: String
    let profile: BrowserProfile?
    let error: String?
}
struct BrowserProfileState: Decodable, Sendable {
    let nativePendingRequests: Int?
    let reconciliationPendingRequests: Int?
    let epoch: UUID
    let enabled: Bool
    let loading: Bool
    let profiles: [BrowserProfile]
    let operations: [BrowserProfileReceipt]
    let error: String?
}
struct WorkspaceIdentity: Decodable, Sendable { let workspaceId: UInt64; let uuid: UUID }
struct DesktopSnapshot: Decodable, Sendable {
    let workspaceIdentities: [WorkspaceIdentity]
    let windowEpoch: UUID
    let windowRequests: [NativeWindowRequest]
    let windowHosts: [WindowHostToken]
    let observedKeyWindow: UInt64?
    let downloadEpoch: UUID
    let downloads: [BrowserDownloadRecord]
    let browserProfiles: BrowserProfileState
    let nativeNotifications: NativeNotificationSnapshot
    let browserEpoch: UUID
    let browserPendingRequests: Int
    let persistence: SessionPersistence?
    let cleanupError: String?
    let uploads: [UploadProgress]
    let workspaceMetadata: [WorkspaceMetadata]?
    let notifications: [AttentionNotification]; let notificationsDropped: UInt64
    let revision: UInt64; var model: Model; let frames: [Frame]
    let lastRequestId: UInt64; let lastError: String?; let closed: Bool
    var workspace: Workspace? { model.workspaces.first { $0.id == model.focusedWorkspace } }
}

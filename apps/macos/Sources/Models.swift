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
struct Viewport: Decodable, Sendable {
    let size: GridSize; let contents: String; let cursorRow: Int; let cursorCol: Int
    let lines: [[TerminalRun]]; let applicationCursor: Bool; let bracketedPaste: Bool; let hideCursor: Bool
    let title: String?; let workingDirectory: String?
    let scrollbackOffset: Int; let scrollbackLines: Int; let alternateScreen: Bool
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
struct Surface: Decodable, Sendable { let id: UInt64; let kind: String; let sessionId: UInt64?; let title: String; let cwd: String?; let url: String?; let navigation: UInt64; let browserProfileId: UUID?; let remote: RemoteEndpoint? }
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
        switch self { case .leaf(let pane): return "p\(pane.id):\(pane.selectedSurface):" + pane.surfaces.map { "\($0.id)" }.joined(separator: ","); case .split(let axis, let a, let b): return "\(axis)(\(a.key),\(b.key))" }
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
struct ClosedItem: Decodable, Sendable { let id: UInt64; let title: String; let workspaceName: String; let restoring: Bool }
struct Model: Decodable, Sendable { let identityNamespace: UUID; let modelVersion: UInt32; let closedItems: [ClosedItem]; let revision: UInt64; let focusedWorkspace: UInt64?; let workspaces: [Workspace] }
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
struct WorkspaceLog: Decodable, Sendable, Equatable {
    enum Level: String, Decodable, Sendable, Equatable { case info, progress, success, warning, error }
    let sequence: UInt64
    let message: String
    let level: Level
    let source: String?
}
struct WorkspaceMetadata: Decodable, Sendable, Equatable {
    let workspaceId: UInt64
    let statuses: [WorkspaceStatus]
    let progress: WorkspaceProgress?
    let logs: [WorkspaceLog]
    let logsDropped: UInt64

    static func empty(_ workspaceId: UInt64) -> Self {
        Self(workspaceId: workspaceId, statuses: [], progress: nil, logs: [], logsDropped: 0)
    }
    var isEmpty: Bool { statuses.isEmpty && progress == nil && logs.isEmpty && logsDropped == 0 }
}
struct BrowserProfile: Decodable, Sendable { let id: UUID; let name: String }
struct BrowserProfileState: Decodable, Sendable { let enabled: Bool; let loading: Bool; let profiles: [BrowserProfile]; let error: String? }
struct DesktopSnapshot: Decodable, Sendable {
    let browserProfiles: BrowserProfileState
    let nativeNotifications: NativeNotificationSnapshot
    let browserEpoch: UUID
    let browserPendingRequests: Int
    let persistence: SessionPersistence?
    let cleanupError: String?
    let uploads: [UploadProgress]
    let workspaceMetadata: [WorkspaceMetadata]?
    let notifications: [AttentionNotification]; let notificationsDropped: UInt64
    let revision: UInt64; let model: Model; let frames: [Frame]
    let lastRequestId: UInt64; let lastError: String?; let closed: Bool
    var workspace: Workspace? { model.workspaces.first { $0.id == model.focusedWorkspace } }
}

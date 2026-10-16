import Foundation

struct WindowHostToken: Codable, Sendable, Equatable {
    let epoch: UUID
    let identityNamespace: UUID
    let windowId: UInt64
    let hostId: UUID
}
struct NativeWindowRequest: Decodable, Sendable {
    let epoch: UUID
    let operationId: UUID
    let identityNamespace: UUID
    let windowId: UInt64
    let windowUuid: UUID
    let hostId: UUID?
    let kind: String
    let focus: Bool
}
struct NativeWindowEvent: Encodable, Sendable {
    let event: String
    var token: WindowHostToken? = nil
    var operationId: UUID? = nil
    var epoch: UUID? = nil
    var message: String? = nil
    var key: Bool? = nil
}

/// Only for guards checked before any AppKit effect. ABI uncertainty must never
/// be converted into a claim that an operation did not happen.
struct NativeWindowPreflightFailure: Error { let message: String }

struct WorkspaceMoveTarget {
    let namespace: UUID
    let workspace: UUID
    let destination: UUID
    let focus: Bool
}

struct WorkspaceOrderTarget {
    let namespace: UUID
    let workspace: UUID
    let window: UUID
    let index: Int
    let revision: UInt64
}

/// Menu targets retain namespace so a stale menu cannot address a new desktop.
struct WorkspaceGroupTarget {
    let namespace: UUID
    let command: WorkspaceGroupCommand
}
enum WorkspaceGroupCommand: Sendable {
    case create(window: UInt64, name: String, members: [UInt64])
    case rename(UInt64, String)
    case collapse(UInt64, Bool)
    case add(group: UInt64, workspace: UInt64)
    case remove(UInt64)
    case ungroup(UInt64)

    var payload: [String: Any] {
        switch self {
        case .create(let window, let name, let members):
            return ["operation": "create", "window_id": window, "name": name, "workspace_ids": members]
        case .rename(let id, let name): return ["operation": "rename", "group_id": id, "name": name]
        case .collapse(let id, let value): return ["operation": "set_collapsed", "group_id": id, "collapsed": value]
        case .add(let group, let workspace): return ["operation": "add", "group_id": group, "workspace_id": workspace]
        case .remove(let workspace): return ["operation": "remove", "workspace_id": workspace]
        case .ungroup(let id): return ["operation": "ungroup", "group_id": id]
        }
    }
}

struct WorkspaceGroupWorkspaceTarget {
    let namespace: UUID
    let workspace: UInt64
}

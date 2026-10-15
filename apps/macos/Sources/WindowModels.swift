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

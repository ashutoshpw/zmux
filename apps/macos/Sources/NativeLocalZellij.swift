import Foundation

/// Explicit host configuration only. Bundle admission supplies these paths;
/// constructing this value neither selects a backend nor authorizes a launch.
struct NativeLocalZellijConfiguration: Encodable, Sendable {
    struct Installation: Encodable, Sendable {
        let epoch: UUID
        let cli: String
        let zellij: String
    }
    struct Profile: Encodable, Sendable {
        let id: UUID
        let root: String
    }
    let installation: Installation
    let profiles: [Profile]
}

enum NativeLocalZellijRequest: Encodable, Sendable {
    case start(profile: UUID, pane: UInt64, name: String, cwd: String, command: String?)
    case attach(profile: UUID, owner: UUID, pane: UInt64)
    case cancel(operation: UInt64)

    private enum CodingKeys: String, CodingKey {
        case operation, profileId = "profile_id", paneId = "pane_id", name, cwd, command
        case ownerUuid = "owner_uuid", operationId = "operation_id", focus
    }
    func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .start(let profile, let pane, let name, let cwd, let command):
            try c.encode("start", forKey: .operation)
            try c.encode(profile, forKey: .profileId)
            try c.encode(pane, forKey: .paneId)
            try c.encode(name, forKey: .name)
            try c.encode(cwd, forKey: .cwd)
            try c.encodeIfPresent(command, forKey: .command)
            try c.encode(true, forKey: .focus)
        case .attach(let profile, let owner, let pane):
            try c.encode("attach", forKey: .operation)
            try c.encode(profile, forKey: .profileId)
            try c.encode(owner, forKey: .ownerUuid)
            try c.encode(pane, forKey: .paneId)
            try c.encode(true, forKey: .focus)
        case .cancel(let operation):
            try c.encode("cancel", forKey: .operation)
            try c.encode(operation, forKey: .operationId)
        }
    }
}

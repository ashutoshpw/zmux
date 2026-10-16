import Foundation

struct AgentMessage: Decodable, Sendable, Identifiable {
    struct Sender: Decodable, Sendable {
        let surfaceId: UUID?
        let workspaceId: UUID?
        let workspaceName: String?
        let name: String
        let attribution: String
    }
    struct Recipient: Decodable, Sendable {
        let surfaceId: UUID
        let workspaceId: UUID
        let workspaceName: String
    }
    struct Attempt: Decodable, Sendable {
        let id: UUID
        let outcome: String
        let payloadDigest: String
        let preparedAt: UInt64
    }
    let id: UUID
    let requestId: UUID
    let namespace: UUID
    let threadId: UUID
    let replyTo: UUID?
    let sender: Sender
    let recipient: Recipient
    let body: String
    let sequence: UInt64
    let state: String
    let failure: String?
    let deliveryAttempts: [Attempt]?
    let retryAuthorized: Bool?
    var failureLabel: String? {
        switch failure {
        case "recipient_closed": return "Recipient tab closed"
        case "messaging_disabled": return "Receiving disabled"
        default: return failure
        }
    }
    var status: String {
        if state == "queued", deliveryAttempts?.last?.outcome == "delivery_unknown" {
            return "Queued · Submission unconfirmed"
        }
        return state.prefix(1).uppercased() + String(state.dropFirst())
    }
}
struct AgentMessageRequest: Encodable, Sendable {
    struct Origin: Encodable, Sendable { let namespace: UUID; let windowId: UInt64 }
    enum Action: Encodable, Sendable {
        case list(UInt64?)
        case reply(UUID, String)
        case markRead([UUID])
        case settings(String, String?, Bool?)
        case status
        private enum CodingKeys: String, CodingKey { case operation, cursor, replyTo, body, ids, scope, target, enabled }
        func encode(to encoder: any Encoder) throws {
            var values = encoder.container(keyedBy: CodingKeys.self)
            switch self {
            case .list(let cursor):
                try values.encode("list", forKey: .operation); try values.encodeIfPresent(cursor, forKey: .cursor)
            case .reply(let parent, let body):
                try values.encode("reply", forKey: .operation); try values.encode(parent, forKey: .replyTo); try values.encode(body, forKey: .body)
            case .markRead(let ids):
                try values.encode("mark_read", forKey: .operation); try values.encode(ids, forKey: .ids)
            case .settings(let scope, let target, let enabled):
                try values.encode("settings", forKey: .operation); try values.encode(scope, forKey: .scope)
                try values.encodeIfPresent(target, forKey: .target); try values.encodeIfPresent(enabled, forKey: .enabled)
            case .status: try values.encode("status", forKey: .operation)
            }
        }
    }
    let origin: Origin
    let requestId: UUID
    let action: Action
}
struct AgentMessageReceipt: Decodable, Sendable {
    struct Failure: Decodable, Sendable { let code: String; let message: String; let outcomeUnknown: Bool }
    struct Result: Decodable, Sendable {
        let messages: [AgentMessage]?
        let nextCursor: UInt64?
        let message: AgentMessage?
        let durability: String?
        let scope: String?
        let owner: UUID?
        let enabled: Bool?
        let effectiveEnabled: Bool?
        let reason: String?
    }
    let requestId: UUID
    let state: String
    let result: Result?
    let error: Failure?
}
struct AgentMessageOperationFailure: Error, LocalizedError, Sendable {
    let message: String
    let outcomeUnknown: Bool
    var errorDescription: String? { message }
}
struct AgentMessageClient: Sendable {
    let bridge: NativeBridge
    func perform(_ request: AgentMessageRequest) async throws -> AgentMessageReceipt {
        let encoder = JSONEncoder(); encoder.keyEncodingStrategy = .convertToSnakeCase
        let data = try encoder.encode(request)
        let bytes = try await bridge.nativeAgentMessages(data)
        struct Response: Decodable {
            struct Failure: Decodable { let message: String }
            let ok: Bool
            let requestId: UUID
            let receipt: AgentMessageReceipt?
            let error: Failure?
            let outcomeUnknown: Bool?
        }
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        let response: Response
        do { response = try decoder.decode(Response.self, from: bytes) }
        catch { throw AgentMessageTransportFailure(message: "The message receipt could not be read. Check the original operation status.", outcomeUnknown: true) }
        guard response.requestId == request.requestId else {
            throw AgentMessageTransportFailure(message: "The message receipt identity differs. Check the original operation status.", outcomeUnknown: true)
        }
        guard response.ok, let receipt = response.receipt else {
            throw AgentMessageTransportFailure(message: response.error?.message ?? "Message request refused.", outcomeUnknown: response.outcomeUnknown ?? true)
        }
        guard receipt.requestId == request.requestId else {
            throw AgentMessageTransportFailure(message: "The original message operation could not be identified.", outcomeUnknown: true)
        }
        return receipt
    }
}

import Foundation

struct BrowserDownloadToken: Codable, Sendable, Equatable {
    let epoch: UUID
    let downloadId: UUID
    let hostId: UUID
}
struct BrowserDownloadOrigin: Decodable, Sendable, Equatable {
    let browser: BrowserOwner
    let surfaceUuid: UUID
    let profileId: UUID?
}
struct BrowserDownloadRecord: Decodable, Sendable, Equatable {
    let epoch: UUID
    let downloadId: UUID
    let hostId: UUID
    let origin: BrowserDownloadOrigin
    let filename: String
    let status: String
    let path: String?
    let bytes: UInt64
    let totalBytes: UInt64?
    let error: String?
    let cleanupComplete: Bool
    var token: BrowserDownloadToken { BrowserDownloadToken(epoch: epoch, downloadId: downloadId, hostId: hostId) }
    var terminal: Bool { ["saved", "failed", "cancelled"].contains(status) }
}
struct NativeDownloadCommand: Encodable, Sendable {
    let operation: String
    var owner: BrowserOwner? = nil
    var token: BrowserDownloadToken? = nil
    var filename: String? = nil
    var path: String? = nil
    var bytes: UInt64? = nil
    var totalBytes: UInt64? = nil
    var message: String? = nil
    var cleanupComplete: Bool? = nil
}
struct NativeDownloadFailure: Error, LocalizedError, Sendable {
    let message: String
    let outcomeUnknown: Bool
    var errorDescription: String? { message }
}

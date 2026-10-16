import Foundation

/// Wire limits of the trusted native file channel. They mirror `crates/native-bridge/include/zmux.h`
/// (`ZMUX_FILE_MAX_HEADER`, `ZMUX_FILE_MAX_SAVE_BODY`, `ZMUX_FILE_MAX_SAVE_FRAME`) and the file
/// service caps; Rust remains the authority and rejects anything larger.
enum FileHostLimits {
    static let maxHeader = 16_384
    /// A reply header is bounded only by the whole reply buffer (`MAX_RESPONSE_BYTES` of the native
    /// bridge), not by the request cap: an open reply echoes every path and a surface receipt per item.
    static let maxReplyHeader = 8_388_608
    static let maxFrame = 16_384
    static let maxSaveBody = 2_097_152
    static let maxSaveFrame = 16_384 + 2_097_152
    static let maxTextBytes = 2_097_152
    static let maxPreviewBytes = 7_340_032
    static let maxOpenItems = 16
}

/// Identifies one native view bound to one surface. Created by Rust, echoed back verbatim.
struct FileOwnerToken: Decodable, Sendable, Equatable {
    let epoch: UUID
    let identityNamespace: UUID
    let surfaceId: UInt64
    let hostId: UUID

    var json: [String: Any] {
        [
            "epoch": epoch.uuidString.lowercased(),
            "identity_namespace": identityNamespace.uuidString.lowercased(),
            "surface_id": surfaceId,
            "host_id": hostId.uuidString.lowercased()
        ]
    }
}

struct FileConflict: Decodable, Sendable, Equatable {
    /// `modified` or `deleted`.
    let kind: String
    let diskSha256: String?
    let diskSize: UInt64?
}

/// A refused or failed file operation. `code` is the stable Rust vocabulary (or `transport` for a
/// failure of the C channel itself). A failure with `outcomeUnknown` must never be replayed.
struct FileHostFailure: Error, LocalizedError, Sendable {
    let code: String
    let message: String
    var opId: UInt64?
    var outcomeUnknown = false
    var conflict: FileConflict?

    var errorDescription: String? { message + " (" + code + ")" }
    /// Only an `observe` (an idempotent absolute-state write) is retried automatically.
    var isRetryableObservation: Bool { ["busy", "timeout", "transport"].contains(code) }
    /// The owner token no longer names a live binding; the view must re-attach.
    var isOwnerLoss: Bool { ["stale_owner", "unknown_surface", "host_conflict", "closed"].contains(code) }
}

enum FileHostRequest: Sendable {
    case attach(epoch: UUID, surface: UInt64, host: UUID)
    case reload(FileOwnerToken, discard: Bool)
    case save(FileOwnerToken, baseGeneration: UInt64, force: Bool)
    case observe(FileOwnerToken, dirty: Bool)
    case detach(FileOwnerToken)
    case open(paths: [String], focus: Bool, forceMarkdown: Bool)

    private var header: [String: Any] {
        switch self {
        case .attach(let epoch, let surface, let host):
            return ["op": "attach", "epoch": epoch.uuidString.lowercased(), "surface_id": surface,
                    "host_id": host.uuidString.lowercased()]
        case .reload(let owner, let discard):
            return ["op": "reload", "owner": owner.json, "discard": discard]
        case .save(let owner, let baseGeneration, let force):
            return ["op": "save", "owner": owner.json, "base_generation": baseGeneration, "force": force]
        case .observe(let owner, let dirty):
            return ["op": "observe", "owner": owner.json, "dirty": dirty]
        case .detach(let owner):
            return ["op": "detach", "owner": owner.json]
        case .open(let paths, let focus, let forceMarkdown):
            var value: [String: Any] = [
                "op": "open", "focus": focus,
                "items": paths.map { ["kind": "path", "value": $0] }
            ]
            if forceMarkdown { value["force_mode"] = "markdown" }
            return value
        }
    }

    /// `u32 big-endian header_len | header JSON | raw body`. Only `save` carries a body.
    func frame(body: Data = Data()) throws -> Data {
        var isSave = false
        if case .save = self { isSave = true }
        guard isSave || body.isEmpty else {
            throw FileHostFailure(code: "invalid_request", message: "Only a save carries a body")
        }
        let encoded = try JSONSerialization.data(withJSONObject: header)
        guard encoded.count <= FileHostLimits.maxHeader else {
            throw FileHostFailure(code: "invalid_request", message: "The file request is too large to send")
        }
        if isSave {
            guard body.count <= FileHostLimits.maxSaveBody else {
                throw FileHostFailure(code: "too_large",
                    message: "The text is larger than the 2 MiB editing limit and cannot be saved")
            }
        } else {
            guard 4 + encoded.count <= FileHostLimits.maxFrame else {
                throw FileHostFailure(code: "invalid_request",
                    message: "Too many or too long paths for one request; open fewer files at once")
            }
        }
        var frame = Data(capacity: 4 + encoded.count + body.count)
        var length = UInt32(encoded.count).bigEndian
        withUnsafeBytes(of: &length) { frame.append(contentsOf: $0) }
        frame.append(encoded)
        frame.append(body)
        return frame
    }
}

struct FileOpenItem: Decodable, Sendable {
    struct Failure: Decodable, Sendable { let code: String; let message: String }
    let index: Int
    let kind: String
    let ok: Bool
    let path: String?
    let url: String?
    let mode: String?
    let deduplicated: Bool?
    let error: Failure?
}

struct FileOpenResult: Decodable, Sendable {
    let ok: Bool
    let items: [FileOpenItem]
}

struct FileHostReplyHeader: Decodable, Sendable {
    struct ErrorBody: Decodable, Sendable { let code: String; let message: String }
    let ok: Bool
    let opId: UInt64?
    let owner: FileOwnerToken?
    let mode: String?
    let size: UInt64?
    let generation: UInt64?
    let sha256: String?
    let mtimeNs: Int64?
    let writable: Bool?
    let bom: Bool?
    let revision: UInt64?
    let result: FileOpenResult?
    let outcomeUnknown: Bool?
    let error: ErrorBody?
    let conflict: FileConflict?
}

struct FileHostReply: Sendable {
    let header: FileHostReplyHeader
    /// File text (BOM removed) or preview bytes; empty for every other reply.
    let body: Data

    /// Splits and validates one reply frame. A successful reply is returned; an `ok:false`
    /// header becomes a `FileHostFailure` carrying the stable code, never a success.
    static func decode(_ frame: Data) throws -> FileHostReply {
        let bytes = [UInt8](frame)
        guard bytes.count >= 4 else {
            throw FileHostFailure(code: "invalid_reply", message: "The file channel reply was truncated", outcomeUnknown: true)
        }
        let headerLength = (Int(bytes[0]) << 24) | (Int(bytes[1]) << 16) | (Int(bytes[2]) << 8) | Int(bytes[3])
        guard headerLength <= FileHostLimits.maxReplyHeader, 4 + headerLength <= bytes.count else {
            throw FileHostFailure(code: "invalid_reply", message: "The file channel reply header is malformed", outcomeUnknown: true)
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let header: FileHostReplyHeader
        do { header = try decoder.decode(FileHostReplyHeader.self, from: Data(bytes[4..<(4 + headerLength)])) }
        catch {
            throw FileHostFailure(code: "invalid_reply", message: "The file channel reply could not be read: " + error.localizedDescription, outcomeUnknown: true)
        }
        guard header.ok else {
            let body = header.error
            throw FileHostFailure(code: body?.code ?? "invalid_reply",
                message: body?.message ?? "The file channel refused the request without a reason",
                opId: header.opId, outcomeUnknown: header.outcomeUnknown ?? false, conflict: header.conflict)
        }
        return FileHostReply(header: header, body: Data(bytes[(4 + headerLength)...]))
    }
}

extension NativeBridge {
    /// Detach a view binding. A binding that is already gone (its surface closed first) is the
    /// expected outcome and is not an error; every other refusal is reported.
    func fileDetach(_ owner: FileOwnerToken) async throws {
        do { _ = try await file(.detach(owner)) }
        catch let failure as FileHostFailure where failure.isOwnerLoss { return }
    }

    /// `File > Open...` and test entry point: the same pipeline as `zmux open` for paths.
    func fileOpen(paths: [String], focus: Bool, forceMarkdown: Bool = false) async throws -> FileOpenResult {
        let reply = try await file(.open(paths: paths, focus: focus, forceMarkdown: forceMarkdown))
        guard let result = reply.header.result else {
            throw FileHostFailure(code: "invalid_reply", message: "The open reply carried no result", outcomeUnknown: true)
        }
        return result
    }
}

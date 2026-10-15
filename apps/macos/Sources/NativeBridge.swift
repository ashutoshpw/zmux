import Foundation

struct BridgeFailure: Error, LocalizedError, Sendable {
    let message: String
    var errorDescription: String? { message }
}

struct ImageTransferAdmission: Sendable {
    let revision: UInt64
    let uploadId: UInt64?
}

struct ImageTransferFailure: Error, LocalizedError, Sendable {
    let message: String
    let outcomeUnknown: Bool
    var errorDescription: String? { message }
}

private struct ImageTransferResponse: Decodable {
    struct Failure: Decodable { let message: String }
    let ok: Bool
    let revision: UInt64?
    let uploadId: UInt64?
    let outcomeUnknown: Bool?
    let error: Failure?
    enum CodingKeys: String, CodingKey {
        case ok, revision, error
        case uploadId = "upload_id"
        case outcomeUnknown = "outcome_unknown"
    }
}

enum NativeDesktopStorage: Sendable {
    case transient
    case applicationSupport
    case persistentDirectory(URL)
}

/// Actor isolation keeps Rust operations, JSON decoding and shutdown off AppKit's main actor.
actor NativeBridge {
    private var handle: UInt64 = 0
    private var requestId: UInt64 = 0
    private var revision: UInt64 = 0

    func start(storage: NativeDesktopStorage = .transient) throws {
        guard zmux_abi_version() == 1 else { throw BridgeFailure(message: "Unsupported native bridge version") }
        let result: ZmuxResult
        switch storage {
        case .transient: result = zmux_desktop_create()
        case .applicationSupport, .persistentDirectory:
            let directory: URL
            switch storage {
            case .persistentDirectory(let explicit): directory = explicit.resolvingSymlinksInPath()
            default:
                // Directory lookup remains off MainActor for ordinary startup.
                let support = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true).resolvingSymlinksInPath()
                directory = support.appendingPathComponent("app.w3dev.zmux.dev", isDirectory: true)
            }
            let path = Data(directory.path.utf8)
            result = path.withUnsafeBytes { storage in
                zmux_desktop_create_persistent(storage.bindMemory(to: UInt8.self).baseAddress, storage.count)
            }
        }
        guard result.status == 0 else { throw Self.failure(result.status) }
        handle = result.value
    }
    func poll() throws -> DesktopSnapshot? {
        guard handle != 0 else { return nil }
        let result = zmux_desktop_snapshot(handle, revision)
        if result.status == -4 { return nil }
        guard result.status == 0 else { throw Self.failure(result.status) }
        guard result.value != 0 else { return nil }
        let data = try Self.consume(result.value)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let snapshot = try decoder.decode(DesktopSnapshot.self, from: data)
        guard snapshot.model.modelVersion == 6 else { throw BridgeFailure(message: "Unsupported workspace model version") }
        revision = snapshot.revision
        return snapshot
    }
    func moveWorkspace(_ workspace: UUID, destination: UUID, focus: Bool) async throws {
        try await send(["command":"targeted", "request":[
            "scope":["workspace":workspace.uuidString.lowercased()],
            "operation":["operation":"action", "action":["action":"move_workspace_to_window", "destination":destination.uuidString.lowercased(), "focus":focus]]
        ]])
    }
    func action(_ name: String, pane: UInt64? = nil, axis: String? = nil, title: String? = nil, workspace: UInt64? = nil, surface: UInt64? = nil, index: Int? = nil, closed: UInt64? = nil, direction: String? = nil, focus: Bool? = nil, window: UInt64? = nil) async throws {
        var action: [String: Any] = ["action": name]
        if let window { action["window_id"] = window }
        if let pane { action["pane_id"] = pane }
        if let axis { action["axis"] = axis }
        if let title { action["name"] = title }
        if let workspace { action["workspace_id"] = workspace }
        if let surface { action["surface_id"] = surface }
        if let index { action["index"] = index }
        if let closed { action["closed_id"] = closed }
        if let direction { action["direction"] = direction }
        if let focus { action["focus"] = focus }
        try await send(["command": "action", "action": action])
    }
    func browser(_ name: String, surface: UInt64?, pane: UInt64?, url: String, title: String?, profile: UUID? = nil) async throws {
        var action: [String: Any] = ["action": name, "url": url]
        if let surface { action["surface_id"] = surface }
        if let pane { action["pane_id"] = pane }
        if let profile { action["profile_id"] = profile.uuidString.lowercased() }
        if let title { action["title"] = title }
        try await send(["command": "action", "action": action])
    }
    func attention(_ operation: String, id: UInt64? = nil, through: UInt64? = nil, active: Bool? = nil) async throws {
        var action: [String: Any] = ["operation": operation]
        if let id { action["id"] = id }
        if let through { action["through_id"] = through }
        if let active { action["active"] = active }
        try await send(["command": "attention", "action": action])
    }
    /// Submit once and return attributable admission, before the disk worker completes.
    /// A lost acknowledgement is never replayed as another profile creation.
    func createBrowserProfile(name: String) async throws -> BrowserProfileReceipt {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        guard !name.isEmpty, name.utf8.count <= 128 else {
            throw BridgeFailure(message: "Profile names must contain 1 to 128 UTF-8 bytes.")
        }
        let desktop = handle
        let data = Data(name.utf8)
        let admission = Task<BrowserProfileReceipt, Error>.detached(priority: .userInitiated) {
            let result = data.withUnsafeBytes { bytes in
                zmux_desktop_profile_add(desktop, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
            }
            guard result.status == 0 else {
                throw BridgeFailure(message: "Profile creation could not be acknowledged (\(result.status)). Check the profile list before trying again.")
            }
            struct Response: Decodable {
                struct Failure: Decodable { let message: String }
                let ok: Bool
                let receipt: BrowserProfileReceipt?
                let error: Failure?
                let outcomeUnknown: Bool?
            }
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            let response: Response
            do { response = try decoder.decode(Response.self, from: Self.consume(result.value)) }
            catch {
                throw BridgeFailure(message: "The profile receipt could not be read. Check the profile list before trying again; the outcome is unknown.")
            }
            guard response.ok, let receipt = response.receipt, receipt.operationId > 0 else {
                let detail = response.error?.message ?? "Profile admission receipt is missing"
                let uncertain = response.outcomeUnknown == true ? ". Check the profile list before trying again; the outcome is unknown." : ""
                throw BridgeFailure(message: detail + uncertain)
            }
            return receipt
        }
        return try await admission.value
    }
    func cancelBrowserDownload(_ record: BrowserDownloadRecord) async throws {
        try await send(["command": "downloads", "request": ["scope": ["surface": record.origin.surfaceUuid.uuidString.lowercased()], "action": ["operation": "cancel", "download_id": record.downloadId.uuidString.lowercased()]]])
    }
    func window(_ operation: String, window: UInt64? = nil, focus: Bool? = nil) async throws {
        var scope: [String: Any] = [:]
        if let window { scope["window"] = "window:" + String(window) }
        var action: [String: Any] = ["operation": operation]
        if let focus { action["focus"] = focus }
        try await send(["command": "windows", "request": ["scope": scope, "action": action]])
    }
    func windowEvent(_ event: NativeWindowEvent) async throws {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        let encoder = JSONEncoder(); encoder.keyEncodingStrategy = .convertToSnakeCase
        let data = try encoder.encode(event)
        guard data.count <= 16_384 else { throw BridgeFailure(message: "Native window event exceeds its transport bound") }
        let desktop = handle
        let work = Task<Void, Error>.detached {
            let result = data.withUnsafeBytes { bytes in zmux_desktop_window_event(desktop, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count) }
            guard result.status == 0 else { throw BridgeFailure(message: "Native window acknowledgement is unavailable; the event was not replayed") }
            struct Response: Decodable {
                struct Failure: Decodable { let message: String }
                struct Accepted: Decodable { let accepted: Bool }
                let ok: Bool
                let error: Failure?
                let result: Accepted?
            }
            let response = try JSONDecoder().decode(Response.self, from: Self.consume(result.value))
            guard response.ok, response.result?.accepted == true else { throw BridgeFailure(message: response.error?.message ?? "Native window event was rejected") }
        }
        try await work.value
    }
    func pasteFiles(surface: UInt64, paths: [String]) async throws {
        try await send(["command": "transfer", "action": ["operation": "paste_files", "surface_id": surface, "paths": paths]])
    }
    /// Native-only download acknowledgements. Tokens are chosen before admission,
    /// so an uncertain response can be correlated with authoritative snapshots.
    func browserDownload(_ command: NativeDownloadCommand) async throws {
        guard handle != 0 else { throw NativeDownloadFailure(message: "Desktop is closed", outcomeUnknown: false) }
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        let data = try encoder.encode(command)
        guard data.count <= 16_384 else { throw NativeDownloadFailure(message: "Download request exceeds its native transport bound", outcomeUnknown: false) }
        let desktop = handle
        let acknowledgement = Task<Void, Error>.detached(priority: .userInitiated) {
            let result = data.withUnsafeBytes { bytes in
                zmux_desktop_download(desktop, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
            }
            guard result.status == 0 else {
                throw NativeDownloadFailure(message: "Download acknowledgement is unavailable; its token was retained for reconciliation", outcomeUnknown: true)
            }
            struct Response: Decodable {
                struct Failure: Decodable { let message: String }
                let ok: Bool
                let outcomeUnknown: Bool?
                let error: Failure?
                let result: DownloadResult?
            }
            struct DownloadResult: Decodable {
                let epoch: UUID?
                let downloadId: UUID?
                let hostId: UUID?
                let accepted: Bool?
            }
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            let response: Response
            do { response = try decoder.decode(Response.self, from: Self.consume(result.value)) }
            catch { throw NativeDownloadFailure(message: "Download acknowledgement could not be read; its token was retained", outcomeUnknown: true) }
            guard response.ok else {
                throw NativeDownloadFailure(message: response.error?.message ?? "Download command failed", outcomeUnknown: response.outcomeUnknown ?? true)
            }
            guard let receipt = response.result else {
                throw NativeDownloadFailure(message: "Download acknowledgement omitted its result", outcomeUnknown: true)
            }
            if command.operation == "begin" {
                guard let token = command.token, receipt.epoch == token.epoch,
                      receipt.downloadId == token.downloadId, receipt.hostId == token.hostId else {
                    throw NativeDownloadFailure(message: "Download admission returned another owner token", outcomeUnknown: true)
                }
            } else if receipt.accepted != true {
                throw NativeDownloadFailure(message: "Download lifecycle acknowledgement is invalid", outcomeUnknown: true)
            }
        }
        try await acknowledgement.value
    }
    /// Wait for the attributable executor receipt on a separate worker. Actor reentrancy
    /// keeps normal input and snapshot polling flowing while this call awaits admission.
    func pastePreparedImage(surface: UInt64, session: UInt64, path: String) async throws -> ImageTransferAdmission {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        requestId += 1
        let desktop = handle
        let data = try JSONSerialization.data(withJSONObject: [
            "version": 1, "request_id": requestId,
            "payload": ["command": "transfer", "action": [
                "operation": "paste_files", "surface_id": surface,
                "expected_session_id": session, "paths": [path]
            ]]
        ])
        return try await Task.detached {
            let result = data.withUnsafeBytes { bytes in
                zmux_desktop_transfer_files(desktop, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
            }
            // Conservatively retain the source if the acknowledgement cannot be read.
            guard result.status == 0 else {
                throw ImageTransferFailure(message: "Image transfer receipt failed (\(result.status)); the cached source was retained", outcomeUnknown: true)
            }
            let receipt: ImageTransferResponse
            do { receipt = try JSONDecoder().decode(ImageTransferResponse.self, from: Self.consume(result.value)) }
            catch { throw ImageTransferFailure(message: "Could not read the image transfer receipt; the cached source was retained", outcomeUnknown: true) }
            guard receipt.ok else {
                throw ImageTransferFailure(message: receipt.error?.message ?? "Image transfer rejected", outcomeUnknown: receipt.outcomeUnknown ?? true)
            }
            guard let revision = receipt.revision else {
                throw ImageTransferFailure(message: "Image transfer receipt has no revision; the cached source was retained", outcomeUnknown: true)
            }
            return ImageTransferAdmission(revision: revision, uploadId: receipt.uploadId)
        }.value
    }
    func cancelUpload(id: UInt64) async throws {
        try await send(["command": "transfer", "action": ["operation": "cancel", "id": id]])
    }
    func input(session: UInt64, text: String) async throws {
        guard text.utf8.count <= 16_384 else { throw BridgeFailure(message: "Development build pastes are limited to 16 KiB") }
        // Split only at Unicode scalar boundaries; each JSON request stays below the ABI limit.
        var chunk = ""
        for scalar in text.unicodeScalars {
            let piece = String(scalar)
            if chunk.utf8.count + piece.utf8.count > 1024 {
                try await send(["command": "input", "session_id": session, "text": chunk])
                chunk = ""
            }
            chunk += piece
        }
        if !chunk.isEmpty { try await send(["command": "input", "session_id": session, "text": chunk]) }
    }
    func wheel(session: UInt64, lines: Int, row: Int, col: Int, modifiers: Int, history: Bool) async throws {
        try await send(["command": "wheel", "session_id": session, "event": ["lines": lines, "row": row, "col": col, "modifiers": modifiers, "history": history]])
    }
    func scroll(session: UInt64, lines: Int) async throws {
        try await send(["command": "scroll", "session_id": session, "lines": lines])
    }
    func resize(session: UInt64, rows: Int, cols: Int) async throws {
        try await send(["command": "resize", "session_id": session, "rows": rows, "cols": cols])
    }
    func drainBrowserRequests() async throws -> BrowserDispatchBatch {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        let desktop = handle
        return try await Task.detached(priority: .userInitiated) {
            let result = zmux_desktop_browser_drain(desktop)
            guard result.status == 0 else { throw Self.failure(result.status) }
            let data = try Self.consume(result.value)
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            return try decoder.decode(BrowserDispatchBatch.self, from: data)
        }.value
    }
    func browserEvent(_ event: BrowserHostEvent) async throws {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        let data = try encoder.encode(event)
        guard data.count <= 300_000 else { throw BridgeFailure(message: "Browser event exceeds native transport limit") }
        for attempt in 0..<100 {
            let desktop = handle
            let status = await Task.detached(priority: .userInitiated) {
                data.withUnsafeBytes { bytes in
                    zmux_desktop_browser_event(desktop, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
                }
            }.value
            if status == -4 {
                guard attempt < 99 else { throw BridgeFailure(message: "Browser event queue remained busy") }
                try await Task.sleep(for: .milliseconds(10))
                continue
            }
            guard status == 0 else { throw Self.failure(status) }
            return
        }
    }
    func notificationEvent(_ event: NativeNotificationEvent) async throws {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        let data = try encoder.encode(event)
        guard data.count <= 16_384 else { throw BridgeFailure(message: "Notification event exceeds native transport limit") }
        let desktop = handle
        let status = await Task.detached(priority: .userInitiated) {
            data.withUnsafeBytes { bytes in
                zmux_desktop_notification_event(desktop, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
            }
        }.value
        guard status == 0 else { throw Self.failure(status) }
    }
    func notificationAction(_ action: NativeAttentionAction) async throws -> NativeNotificationExecution {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        let data = try encoder.encode(action)
        // A bounded 4095-byte reply can expand sixfold under JSON escaping.
        guard data.count <= 32_768 else { throw BridgeFailure(message: "Notification action exceeds native transport limit") }
        let desktop = handle
        return try await Task.detached(priority: .userInitiated) {
            let result = data.withUnsafeBytes { bytes in
                zmux_desktop_notification_action(desktop, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
            }
            guard result.status == 0 else { throw Self.failure(result.status) }
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            return try decoder.decode(NativeNotificationExecution.self, from: Self.consume(result.value))
        }.value
    }
    func browserAction(surface: UInt64, action: BrowserAction) async throws {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        let encoded = try JSONSerialization.jsonObject(with: encoder.encode(action))
        try await send(["command": "browser", "request": [
            "scope": ["surface": "surface:\(surface)"], "action": encoded
        ]])
    }
    func close() throws {
        guard handle != 0 else { return }
        let status = zmux_desktop_destroy(handle)
        if status != 0 {
            // Failed teardown retains the handle so its final cleanup error is readable.
            let snapshot = zmux_desktop_snapshot(handle, 0)
            if snapshot.status == 0, snapshot.value != 0 {
                let data = try Self.consume(snapshot.value)
                let decoder = JSONDecoder()
                decoder.keyDecodingStrategy = .convertFromSnakeCase
                let state = try decoder.decode(DesktopSnapshot.self, from: data)
                if let message = state.cleanupError ?? state.persistence?.error {
                    throw BridgeFailure(message: message)
                }
            }
            throw Self.failure(status)
        }
        handle = 0
    }
    private func send(_ payload: [String: Any]) async throws {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        requestId += 1
        let data = try JSONSerialization.data(withJSONObject: ["version": 1, "request_id": requestId, "payload": payload])
        for attempt in 0..<100 {
            let result = data.withUnsafeBytes { bytes in
                zmux_desktop_dispatch(handle, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
            }
            if result.status == -4 {
                if attempt == 99 { throw BridgeFailure(message: "Terminal is busy; input was not accepted") }
                try await Task.sleep(for: .milliseconds(10))
                continue
            }
            guard result.status == 0 else { throw Self.failure(result.status) }
            let response = try JSONSerialization.jsonObject(with: Self.consume(result.value)) as? [String: Any]
            guard response?["ok"] as? Bool == true else {
                let error = response?["error"] as? [String: Any]
                throw BridgeFailure(message: error?["message"] as? String ?? "Command rejected")
            }
            return
        }
    }
    private nonisolated static func consume(_ token: UInt64) throws -> Data {
        defer {
            let status = zmux_buffer_release(token)
            if status != 0 { FileHandle.standardError.write(Data("Zmux buffer release failed: \(status)\n".utf8)) }
        }
        let length = zmux_buffer_length(token)
        guard length.status == 0, length.value <= 8_388_608 else { throw Self.failure(length.status) }
        var data = Data(count: Int(length.value))
        let status = data.withUnsafeMutableBytes { bytes in
            zmux_buffer_copy(token, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
        }
        guard status == 0 else { throw Self.failure(status) }
        return data
    }
    private nonisolated static func failure(_ status: Int32) -> BridgeFailure { BridgeFailure(message: "Native operation failed (\(status))") }
}

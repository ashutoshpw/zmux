import Foundation

struct BridgeFailure: Error, LocalizedError, Sendable {
    let message: String
    var errorDescription: String? { message }
}

/// Actor isolation keeps Rust operations, JSON decoding and shutdown off AppKit's main actor.
actor NativeBridge {
    private var handle: UInt64 = 0
    private var requestId: UInt64 = 0
    private var revision: UInt64 = 0

    func start() throws {
        guard zmux_abi_version() == 1 else { throw BridgeFailure(message: "Unsupported native bridge version") }
        let result = zmux_desktop_create()
        guard result.status == 0 else { throw failure(result.status) }
        handle = result.value
    }
    func poll() throws -> DesktopSnapshot? {
        guard handle != 0 else { return nil }
        let result = zmux_desktop_snapshot(handle, revision)
        if result.status == -4 { return nil }
        guard result.status == 0 else { throw failure(result.status) }
        guard result.value != 0 else { return nil }
        let data = try consume(result.value)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let snapshot = try decoder.decode(DesktopSnapshot.self, from: data)
        guard snapshot.model.modelVersion == 2 else { throw BridgeFailure(message: "Unsupported workspace model version") }
        revision = snapshot.revision
        return snapshot
    }
    func action(_ name: String, pane: UInt64? = nil, axis: String? = nil, title: String? = nil, workspace: UInt64? = nil, surface: UInt64? = nil, index: Int? = nil, closed: UInt64? = nil) async throws {
        var action: [String: Any] = ["action": name]
        if let pane { action["pane_id"] = pane }
        if let axis { action["axis"] = axis }
        if let title { action["name"] = title }
        if let workspace { action["workspace_id"] = workspace }
        if let surface { action["surface_id"] = surface }
        if let index { action["index"] = index }
        if let closed { action["closed_id"] = closed }
        try await send(["command": "action", "action": action])
    }
    func attention(_ operation: String, id: UInt64? = nil, through: UInt64? = nil, active: Bool? = nil) async throws {
        var action: [String: Any] = ["operation": operation]
        if let id { action["id"] = id }
        if let through { action["through_id"] = through }
        if let active { action["active"] = active }
        try await send(["command": "attention", "action": action])
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
    func resize(session: UInt64, rows: Int, cols: Int) async throws {
        try await send(["command": "resize", "session_id": session, "rows": rows, "cols": cols])
    }
    func close() throws {
        guard handle != 0 else { return }
        let status = zmux_desktop_destroy(handle)
        guard status == 0 else { throw failure(status) }
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
            guard result.status == 0 else { throw failure(result.status) }
            let response = try JSONSerialization.jsonObject(with: consume(result.value)) as? [String: Any]
            guard response?["ok"] as? Bool == true else {
                let error = response?["error"] as? [String: Any]
                throw BridgeFailure(message: error?["message"] as? String ?? "Command rejected")
            }
            return
        }
    }
    private func consume(_ token: UInt64) throws -> Data {
        defer {
            let status = zmux_buffer_release(token)
            if status != 0 { FileHandle.standardError.write(Data("Zmux buffer release failed: \(status)\n".utf8)) }
        }
        let length = zmux_buffer_length(token)
        guard length.status == 0, length.value <= 8_388_608 else { throw failure(length.status) }
        var data = Data(count: Int(length.value))
        let status = data.withUnsafeMutableBytes { bytes in
            zmux_buffer_copy(token, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
        }
        guard status == 0 else { throw failure(status) }
        return data
    }
    private func failure(_ status: Int32) -> BridgeFailure { BridgeFailure(message: "Native operation failed (\(status))") }
}

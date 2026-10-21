import Foundation

private struct NativeFrameEnvelope: Decodable {
    let schemaVersion: Int
    let baseRevision: UInt64?
    var snapshot: DesktopSnapshot
    let framePackets: [NativeFramePacket]
}
private struct NativeFramePacket: Decodable {
    let frame: Frame
    let unchanged: Bool
    let changedRows: [Int]?
    let contentsStart: Int
    let contentsEnd: Int
}
private struct NativeTerminalSharingReceipt: Decodable {
    struct Failure: Decodable {
        let message: String
    }

    let ok: Bool?
    let grant: UUID?
    let error: Failure?
}

struct BridgeFailure: Error, LocalizedError, Sendable {
    let message: String
    var errorDescription: String? { message }
}

struct DesktopShutdownReport: Decodable, Sendable {
    struct Pending: Decodable, Sendable {
        let sessions, launches, tmux, transfers, profiles, persistence, downloads: UInt64
        let nativeDownloads, connections, control: UInt64
        let nativeCleanup: Bool
    }
    let ticket: UInt64
    let phase: String
    let deadlineExpired: Bool
    let remainingMs: UInt64
    let runtimeCleanupComplete: Bool
    let physicalCleanupComplete: Bool
    let pending: Pending
    let issues: [String]
    let issuesDropped: UInt64
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

enum BrowserEventDisposition: Sendable { case applied, rejectedObsolete }

/// Actor isolation keeps Rust operations, JSON decoding and shutdown off AppKit's main actor.
actor NativeBridge {
    private var handle: UInt64 = 0
    private var requestId: UInt64 = 0
    private var revision: UInt64 = 0
    private var frameCache: [UInt64: Frame] = [:]
    private var frameNamespace: UUID?
    private let profileLeaseEpoch = UUID()

    func start(storage: NativeDesktopStorage = .transient, terminalEngine: String? = nil) throws {
        guard zmux_abi_version() == 1 else { throw BridgeFailure(message: "Unsupported native bridge version") }
        let stateDirectory: String?
        switch storage {
        case .transient: stateDirectory = nil
        case .applicationSupport, .persistentDirectory:
            let directory: URL
            switch storage {
            case .persistentDirectory(let explicit): directory = explicit
            default:
                // Directory lookup remains off MainActor for ordinary startup.
                let support = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
                directory = support.appendingPathComponent("app.w3dev.zmux.dev", isDirectory: true)
            }
            guard directory.isFileURL else { throw NativeStoragePathFailure.invalidPath }
            stateDirectory = try NativeStoragePath.resolve(directory.path)
        }
        let result: ZmuxResult
        if terminalEngine != nil || stateDirectory != nil {
            var configuration: [String: Any] = [:]
            if let terminalEngine {
                guard terminalEngine.utf8.count <= 2048 else { throw BridgeFailure(message: "Terminal engine configuration exceeds its bound") }
                configuration["terminal_engine"] = try JSONSerialization.jsonObject(with: Data(terminalEngine.utf8))
            }
            if let stateDirectory {
                configuration["profile_controller"] = profileLeaseEpoch.uuidString.lowercased()
                configuration["state_directory"] = stateDirectory
                configuration["profile_lease_directory"] = try BrowserProfileLeaseDirectory.resolve()
            }
            let data = try JSONSerialization.data(withJSONObject: configuration)
            result = data.withUnsafeBytes { storage in
                zmux_desktop_create_configured(storage.bindMemory(to: UInt8.self).baseAddress, storage.count)
            }
        } else {
            result = zmux_desktop_create()
        }
        guard result.status == 0 else { throw Self.failure(result.status) }
        handle = result.value
        revision = 0; frameCache.removeAll(); frameNamespace = nil
    }
    func acquireBrowserProfileLease(profile: UUID, store: UUID) async throws -> BrowserProfileLeaseToken {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed; profile store acquisition was not admitted.") }
        return try await BrowserProfileLeaseClient.acquire(desktop: handle,
            owner: BrowserProfileLeaseOwner(epoch: profileLeaseEpoch, store: store), profile: profile)
    }
    func reuseBrowserProfileLease(_ token: BrowserProfileLeaseToken, construction: UUID) async throws -> BrowserProfileConstructionPermit {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop closed before profile construction admission.") }
        return try await BrowserProfileLeaseClient.reuse(desktop: handle, token: token, construction: construction)
    }
    func acknowledgeBrowserProfileConstruction(_ token: BrowserProfileLeaseToken) async throws {
        try await BrowserProfileLeaseClient.constructed(token)
    }
    func completeBrowserProfileConstruction(_ permit: BrowserProfileConstructionPermit) async throws {
        try await BrowserProfileLeaseClient.constructionDone(permit)
    }
    func profileController() -> UUID { profileLeaseEpoch }
    func releaseUnconstructedBrowserProfileLease(_ token: BrowserProfileLeaseToken) async throws {
        try await BrowserProfileLeaseClient.releaseUnconstructed(token)
    }
    func poll() throws -> DesktopSnapshot? {
        guard handle != 0 else { return nil }
        let result = zmux_desktop_poll_frames(handle, revision)
        if result.status == -4 { return nil }
        guard result.status == 0 else { throw Self.failure(result.status) }
        guard result.value != 0 else { return nil }
        let data = try Self.consume(result.value)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let envelope = try decoder.decode(NativeFrameEnvelope.self, from: data)
        let paneCount = envelope.snapshot.model.workspaces.reduce(0) { $0 + $1.root.panes.count }
        guard envelope.schemaVersion == 1,
              envelope.baseRevision == nil || (envelope.baseRevision == revision && frameNamespace == envelope.snapshot.model.identityNamespace),
              envelope.framePackets.count <= paneCount else { throw BridgeFailure(message: "Invalid native frame baseline") }
        var snapshot = envelope.snapshot
        guard snapshot.model.modelVersion == 7 else { throw BridgeFailure(message: "Unsupported workspace model version") }
        var next: [UInt64: Frame] = [:]
        for packet in envelope.framePackets {
            let frame = packet.frame
            guard next[frame.sessionId] == nil else { throw BridgeFailure(message: "Duplicate native terminal frame") }
            var terminal = frame.snapshot
            if packet.unchanged {
                guard envelope.baseRevision != nil, let old = frameCache[frame.sessionId], terminal == nil,
                      packet.changedRows == nil else { throw BridgeFailure(message: "Missing unchanged native frame baseline") }
                terminal = old.snapshot
            } else if let rows = packet.changedRows {
                guard envelope.baseRevision != nil, let old = frameCache[frame.sessionId]?.snapshot,
                      let delta = terminal, old.viewport.size == delta.viewport.size,
                      old.viewport.lines.count == delta.viewport.lines.count,
                      Set(rows).count == rows.count,
                      rows.allSatisfy({ delta.viewport.lines.indices.contains($0) }) else {
                    throw BridgeFailure(message: "Invalid native terminal row delta")
                }
                var viewport = delta.viewport
                let changed = Set(rows)
                for row in viewport.lines.indices where !changed.contains(row) { viewport.lines[row] = old.viewport.lines[row] }
                let bytes = Array(old.viewport.contents.utf8)
                let replacement = Array(viewport.contents.utf8)
                guard packet.contentsStart >= 0, packet.contentsEnd >= packet.contentsStart,
                      packet.contentsEnd <= bytes.count,
                      bytes.count - (packet.contentsEnd - packet.contentsStart) + replacement.count <= 8_388_608 else {
                    throw BridgeFailure(message: "Invalid native terminal text delta")
                }
                var merged = Data(bytes[..<packet.contentsStart])
                merged.append(contentsOf: replacement)
                merged.append(contentsOf: bytes[packet.contentsEnd...])
                guard let contents = String(data: merged, encoding: .utf8) else { throw BridgeFailure(message: "Native terminal text delta broke UTF-8") }
                viewport.contents = contents
                terminal = TerminalSnapshot(state: delta.state, viewport: viewport, bytesRead: delta.bytesRead,
                    exitCode: delta.exitCode, failure: delta.failure)
            }
            next[frame.sessionId] = Frame(sessionId: frame.sessionId, processId: frame.processId,
                starting: frame.starting, reconnectable: frame.reconnectable, snapshot: terminal, failure: frame.failure)
        }
        snapshot.frames = envelope.framePackets.compactMap { next[$0.frame.sessionId] }
        frameCache = next; frameNamespace = snapshot.model.identityNamespace
        revision = snapshot.revision
        return snapshot
    }
    /// Installs or revokes the Rust-owned gate for Cloud terminal sharing. The returned UUID is
    /// also the Cloud presence epoch; a stale relay request cannot outlive a local disable.
    func setTerminalSharing(enabled: Bool) async throws -> UUID? {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        let desktop = handle
        let result = await Task.detached(priority: .userInitiated) {
            zmux_desktop_terminal_sharing(desktop, enabled)
        }.value
        guard result.status == 0 else { throw Self.failure(result.status) }
        let receipt = try JSONDecoder().decode(NativeTerminalSharingReceipt.self, from: Self.consume(result.value))
        if let error = receipt.error {
            throw BridgeFailure(message: error.message)
        }
        guard receipt.ok != false else {
            throw BridgeFailure(message: "The native terminal sharing gate rejected the request")
        }
        if enabled {
            guard let grant = receipt.grant else { throw BridgeFailure(message: "The terminal sharing grant was not returned") }
            return grant
        }
        guard receipt.grant == nil else { throw BridgeFailure(message: "The terminal sharing gate did not close") }
        return nil
    }
    /// Dispatches one typed, already-decrypted terminal operation through the existing Rust
    /// session owner. Rust validates the grant and exact namespace/surface/session binding.
    func sharedTerminalOperation(request: Data, grant: UUID) async throws -> Data {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        guard request.count <= 131_072,
              (try? JSONSerialization.jsonObject(with: request)) is [String: Any] else {
            throw BridgeFailure(message: "Terminal sharing operation exceeds its size limit or is not an object")
        }
        // Preserve the exact request bytes so Rust's strict decoder can reject duplicate fields
        // and retain its deny-unknown-fields behavior. This owner is the grant authority.
        var envelope = Data("{\"grant\":\"\(grant.uuidString.lowercased())\",\"request\":".utf8)
        envelope.append(request)
        envelope.append(0x7D)
        guard envelope.count <= 131_072 else { throw BridgeFailure(message: "Terminal sharing envelope exceeds its size limit") }
        let envelopeData = envelope
        let desktop = handle
        return try await Task.detached(priority: .userInitiated) {
            let result = envelopeData.withUnsafeBytes { bytes in
                zmux_desktop_shared_terminal(desktop, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
            }
            guard result.status == 0 else { throw Self.failure(result.status) }
            return try Self.consume(result.value)
        }.value
    }
    func selectWorkspaceNumber(_ digit: Int, window: UUID) async throws {
        try await send(["command":"targeted", "request":[
            "scope":["window":window.uuidString.lowercased()],
            "operation":["operation":"select_workspace_number", "digit":digit]
        ]])
    }
    func reorderWorkspace(_ workspace: UUID, window: UUID, index: Int, revision: UInt64) async throws {
        try await send(["command":"workspace_order", "request":[
            "window":window.uuidString.lowercased(), "expected_model_revision":revision,
            "order":["operation":"single", "workspace":workspace.uuidString.lowercased(),
                     "placement":["placement":"index", "index":index]]
        ]])
    }
    func reorderSurface(namespace: UUID, surface: UInt64, pane: UInt64, value: Int, relative: Bool, revision: UInt64?) async throws {
        var request: [String: Any] = [
            "identity_namespace": namespace.uuidString.lowercased(),
            "scope": ["surface": "surface:\(surface)", "pane": "pane:\(pane)"],
            "focus": true,
            "placement": ["placement": relative ? "offset" : "index", relative ? "offset" : "index": value]
        ]
        if let revision { request["expected_model_revision"] = revision }
        try await send(["command": "surface_order", "request": request])
    }
    func moveWorkspace(_ workspace: UUID, destination: UUID, focus: Bool) async throws {
        try await send(["command":"targeted", "request":[
            "scope":["workspace":workspace.uuidString.lowercased()],
            "operation":["operation":"action", "action":["action":"move_workspace_to_window", "destination":destination.uuidString.lowercased(), "focus":focus]]
        ]])
    }
    func workspaceGroup(_ command: WorkspaceGroupCommand) async throws {
        try await send(["command": "action", "action": ["action": "workspace_group", "command": command.payload]])
    }
    /// `acknowledged` waits for the executor's own result instead of queue acceptance, so a
    /// refused command throws. Closes that discard unsaved text use it.
    func action(_ name: String, pane: UInt64? = nil, axis: String? = nil, title: String? = nil, workspace: UInt64? = nil, surface: UInt64? = nil, index: Int? = nil, closed: UInt64? = nil, direction: String? = nil, focus: Bool? = nil, window: UInt64? = nil, acknowledged: Bool = false) async throws {
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
        try await send(["command": "action", "action": action], acknowledged: acknowledged)
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
        try await mutateBrowserProfile(["operation": "add", "name": name])
    }
    func renameBrowserProfile(id: UUID, name: String) async throws -> BrowserProfileReceipt {
        try await mutateBrowserProfile(["operation": "rename", "profile": id.uuidString.lowercased(), "name": name])
    }
    func reconcileBrowserProfileData(profile: UUID, intent: UUID) async throws {
        let controller = profileLeaseEpoch
        var receipt = try await BrowserProfileNativeClient.call(.init(operation: "request_reconcile", controller: controller, intent: intent, profile: profile), as: BrowserProfileNativeClient.Reconciliation.self)
        let id = receipt.id
        let deadline = ContinuousClock.now + .seconds(20)
        while receipt.state == "pending", ContinuousClock.now < deadline {
            try await Task.sleep(for: .milliseconds(50))
            receipt = try await BrowserProfileNativeClient.call(.init(operation: "reconcile_status", controller: controller, id: id), as: BrowserProfileNativeClient.Reconciliation.self)
            guard receipt.id == id, receipt.profile == profile, receipt.intent == intent else { throw BridgeFailure(message: "Original profile reconciliation receipt mismatch.") }
        }
        if receipt.state == "failed", receipt.originalPhase == "failed_partial" {
            throw BrowserProfileNativeFailure(message: "Website data cleared; profile removal failed.", outcomeUnknown: false)
        }
        if receipt.state == "failed", receipt.originalPhase == "failed_before_clear" {
            throw BrowserProfileNativeFailure(message: "Operation failed before website data was cleared.", outcomeUnknown: false)
        }
        guard receipt.state == "completed" else { throw BrowserProfileNativeFailure(message: receipt.error ?? "Original profile reconciliation \(id) remains pending; no WebKit effect was replayed.", outcomeUnknown: true) }
    }
    func changeBrowserProfileData(id: UUID, delete: Bool) async throws -> BrowserProfileReceipt {
        try await mutateBrowserProfile(["operation": delete ? "delete" : "clear", "profile": id.uuidString.lowercased()])
    }
    private func mutateBrowserProfile(_ command: [String: String]) async throws -> BrowserProfileReceipt {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        if command["operation"] == "add" || command["operation"] == "rename" {
            guard let name = command["name"], !name.isEmpty, name.utf8.count <= 128 else {
                throw BridgeFailure(message: "Profile names must contain 1 to 128 UTF-8 bytes.")
            }
        }
        let desktop = handle
        let data = try JSONSerialization.data(withJSONObject: command)
        let admission = Task<BrowserProfileReceipt, Error>.detached(priority: .userInitiated) {
            let result = data.withUnsafeBytes { bytes in
                zmux_desktop_profile_command(desktop, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
            }
            guard result.status == 0 else {
                throw BridgeFailure(message: "Profile change could not be acknowledged (\(result.status)). Check the profile list before trying again.")
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
    func window(_ operation: String, window: UInt64? = nil, focus: Bool? = nil, acknowledged: Bool = false) async throws {
        var scope: [String: Any] = [:]
        if let window { scope["window"] = "window:" + String(window) }
        var action: [String: Any] = ["operation": operation]
        if let focus { action["focus"] = focus }
        try await send(["command": "windows", "request": ["scope": scope, "action": action]], acknowledged: acknowledged)
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
    func pasteFiles(surface: UInt64, expectedSession: UInt64, paths: [String]) async throws {
        try await send(["command": "transfer", "action": ["operation": "paste_files", "surface_id": surface,
            "expected_session_id": expectedSession, "paths": paths]])
    }
    func reconnectSSH(identityNamespace: UUID, surface: UInt64, expectedSession: UInt64) async throws {
        try await send(["command": "action", "action": ["action": "reconnect_ssh",
            "identity_namespace": identityNamespace.uuidString.lowercased(), "surface_id": surface,
            "expected_session_id": expectedSession]], acknowledged: true)
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
    /// Trusted native file channel: attach, reload, save, observe, detach and open. The blocking C
    /// call (up to two seconds, five for save) runs on a detached worker, never on the main actor
    /// or this actor's executor, so snapshot polling and input keep flowing. A failure whose
    /// `outcomeUnknown` is true is never replayed; only `observe` is retried by its owner view.
    func file(_ request: FileHostRequest, body: Data = Data()) async throws -> FileHostReply {
        guard handle != 0 else { throw FileHostFailure(code: "closed", message: "Desktop is closed") }
        let frame = try request.frame(body: body)
        let desktop = handle
        return try await Task.detached(priority: .userInitiated) {
            let result = frame.withUnsafeBytes { bytes in
                zmux_desktop_file(desktop, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
            }
            guard result.status == 0 else {
                // -2, -4 and -5 are refused before Rust does any work (`FileBridgeRefusal`): -4
                // and -5 are reported as the transient `busy`; any other status leaves the effect
                // unknown.
                let refusal = FileBridgeRefusal(status: result.status)
                throw FileHostFailure(code: refusal.code, message: refusal.message, outcomeUnknown: refusal.outcomeUnknown)
            }
            return try FileHostReply.decode(Self.consume(result.value))
        }.value
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
        guard text.utf8.count <= 4_096 else { throw BridgeFailure(message: "Terminal input is limited to 4096 UTF-8 bytes per operation") }
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
    func key(session: UInt64, event: TerminalKeyEvent) async throws {
        var payload: [String: Any] = ["key": event.key, "action": event.action.rawValue, "modifiers": event.modifiers]
        if let control = event.legacyControl { payload["legacy_control"] = control }
        try await send(["command": "key", "session_id": session, "event": payload])
    }
    func paste(session: UInt64, text: String) async throws {
        guard text.utf8.count <= 16_384 else { throw BridgeFailure(message: "Development build pastes are limited to 16 KiB") }
        if !text.isEmpty { try await send(["command": "paste", "session_id": session, "text": text]) }
    }
    func wheel(session: UInt64, lines: Int, row: Int, col: Int, modifiers: Int, history: Bool) async throws {
        try await send(["command": "wheel", "session_id": session, "event": ["lines": lines, "row": row, "col": col, "modifiers": modifiers, "history": history]])
    }
    func pointer(session: UInt64, event: TerminalPointerEvent) async throws {
        try await send(["command": "pointer", "session_id": session, "event": [
            "action": event.action.rawValue, "button": event.button.rawValue,
            "row": event.row, "col": event.col, "modifiers": event.modifiers]])
    }
    func openTerminalLink(_ request: TerminalLinkRequest) async throws {
        try await send(["command": "open_terminal_link", "request": [
            "identity_namespace": request.identityNamespace.uuidString.lowercased(),
            "surface_id": request.surfaceID,
            "pane_id": request.paneID,
            "session_id": request.sessionID,
            "viewport_generation": request.viewportGeneration,
            "row": request.row,
            "column": request.column,
            "expected_url": request.expectedURL
        ]])
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
    func browserEvent(_ event: BrowserHostEvent) async throws -> BrowserEventDisposition {
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
            if status == ZMUX_BROWSER_EVENT_OBSOLETE { return .rejectedObsolete }
            guard status == 0 else { throw Self.failure(status) }
            return .applied
        }
        throw BridgeFailure(message: "Browser event queue remained busy")
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
    func beginShutdown() async throws -> DesktopShutdownReport {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        let desktop = handle
        return try await Task.detached {
            try Self.decodeShutdown(zmux_desktop_shutdown_begin(desktop, 10_000, 1))
        }.value
    }
    func shutdownStatus(ticket: UInt64) async throws -> DesktopShutdownReport {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        let desktop = handle
        return try await Task.detached {
            try Self.decodeShutdown(zmux_desktop_shutdown_status(desktop, ticket))
        }.value
    }
    func nativeCleanupComplete(ticket: UInt64) async throws {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        let desktop = handle
        let status = await Task.detached { zmux_desktop_shutdown_native_complete(desktop, ticket) }.value
        guard status == 0 else { throw Self.failure(status) }
    }
    func releaseShutdown(ticket: UInt64) async throws {
        guard handle != 0 else { return }
        let desktop = handle
        let status = await Task.detached { zmux_desktop_shutdown_release(desktop, ticket) }.value
        guard status == 0 else { throw Self.failure(status) }
        handle = 0
        frameCache.removeAll(); frameNamespace = nil
    }
    private nonisolated static func decodeShutdown(_ result: ZmuxResult) throws -> DesktopShutdownReport {
        guard result.status == 0 else { throw failure(result.status) }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(DesktopShutdownReport.self, from: consume(result.value))
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
    private func send(_ payload: [String: Any], acknowledged: Bool = false) async throws {
        guard handle != 0 else { throw BridgeFailure(message: "Desktop is closed") }
        requestId += 1
        let data = try JSONSerialization.data(withJSONObject: ["version": 1, "request_id": requestId, "payload": payload])
        for attempt in 0..<100 {
            let desktop = handle
            let status: Int32
            let body: Data
            if acknowledged {
                // Blocks for up to two seconds until the executor has run the command, so it runs
                // on a detached worker: never on the main actor or this actor's executor.
                (status, body) = try await Task.detached(priority: .userInitiated) {
                    let result = data.withUnsafeBytes { bytes in
                        zmux_desktop_dispatch_acknowledged(desktop, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
                    }
                    guard result.status == 0 else { return (result.status, Data()) }
                    return (result.status, try Self.consume(result.value))
                }.value
            } else {
                let result = data.withUnsafeBytes { bytes in
                    zmux_desktop_dispatch(desktop, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
                }
                status = result.status
                body = result.status == 0 ? try Self.consume(result.value) : Data()
            }
            if status == -4 {
                // Nothing ran: the queue was full. Safe to try again for both entry points.
                if attempt == 99 { throw BridgeFailure(message: "Terminal is busy; input was not accepted") }
                try await Task.sleep(for: .milliseconds(10))
                continue
            }
            guard status == 0 else { throw Self.failure(status, operation: "send \(payload["command"] as? String ?? "unknown") request=\(requestId)") }
            let response = try JSONSerialization.jsonObject(with: body) as? [String: Any]
            guard response?["ok"] as? Bool == true else {
                let error = response?["error"] as? [String: Any]
                var message = error?["message"] as? String ?? "Command rejected"
                if acknowledged, response?["outcome_unknown"] as? Bool == true {
                    message += " (the outcome is unknown and the command was not replayed)"
                }
                throw BridgeFailure(message: message)
            }
            return
        }
    }
    nonisolated static func consume(_ token: UInt64, operation: String = #function) throws -> Data {
        defer {
            let status = zmux_buffer_release(token)
            if status != 0 { FileHandle.standardError.write(Data("Zmux buffer release failed: \(status)\n".utf8)) }
        }
        let length = zmux_buffer_length(token)
        guard length.status == 0, length.value <= 8_388_608 else { throw Self.failure(length.status, operation: "\(operation):buffer_length token=\(token)") }
        var data = Data(count: Int(length.value))
        let status = data.withUnsafeMutableBytes { bytes in
            zmux_buffer_copy(token, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
        }
        guard status == 0 else { throw Self.failure(status, operation: "\(operation):buffer_copy token=\(token)") }
        return data
    }
    private nonisolated static func failure(_ status: Int32, operation: String = #function) -> BridgeFailure { BridgeFailure(message: "Native \(operation) failed (\(status))") }
}

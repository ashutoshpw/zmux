import Foundation

/// Owns preparation tasks and remote source lifetimes; no decoding or filesystem I/O
/// runs on MainActor, and preparing an image never occupies the serial input stream.
@MainActor
final class ClipboardImagePastes {
    private struct PendingUpload {
        let prepared: PreparedClipboardImage
        let admission: ImageTransferAdmission
    }

    private let bridge: NativeBridge
    private let preparation = ClipboardImagePreparation()
    private let reportError: (String) -> Void
    private let statusChanged: () -> Void
    private var tasks: [UUID: Task<Void, Never>] = [:]
    private var cleanupTasks: [UUID: Task<Void, Never>] = [:]
    private var uploads: [UInt64: PendingUpload] = [:]
    private var latest: DesktopSnapshot?
    private var closed = false
    private let maximumPending = 4

    var preparingCount: Int { tasks.count }

    init(bridge: NativeBridge, reportError: @escaping (String) -> Void, statusChanged: @escaping () -> Void) {
        self.bridge = bridge
        self.reportError = reportError
        self.statusChanged = statusChanged
    }

    @discardableResult
    func paste(_ data: Data, surface: UInt64, session: UInt64) -> Bool {
        guard !closed else { reportError("Image paste is unavailable while closing"); return false }
        guard tasks.count + uploads.count + cleanupTasks.count < maximumPending else {
            reportError("Four images are already being prepared or uploaded; wait before pasting again")
            return false
        }
        let id = UUID()
        tasks[id] = Task {
            var failure: String?
            defer {
                tasks.removeValue(forKey: id)
                statusChanged()
                if let failure { reportError(failure) }
            }
            var prepared: PreparedClipboardImage?
            do {
                let image = try await preparation.prepare(data: data)
                prepared = image
                try Task.checkCancellation()
                let admission = try await bridge.pastePreparedImage(surface: surface, session: session, path: image.path)
                if let upload = admission.uploadId {
                    uploads[upload] = PendingUpload(prepared: image, admission: admission)
                    reconcile()
                }
                // A local insertion hands the path to the shell/user. Leave it cached
                // and protected rather than deleting it when this task completes.
            } catch {
                if !(error is CancellationError) { failure = error.localizedDescription }
                if let prepared, (error as? ImageTransferFailure)?.outcomeUnknown != true {
                    do { try await preparation.release(prepared) }
                    catch { failure = [failure, "Image paste cleanup failed: \(error.localizedDescription)"].compactMap { $0 }.joined(separator: "; ") }
                }
            }
        }
        statusChanged()
        return true
    }

    func update(_ snapshot: DesktopSnapshot) {
        guard latest == nil || snapshot.revision >= latest!.revision else { return }
        latest = snapshot
        reconcile()
    }

    private func reconcile() {
        guard let latest else { return }
        for (id, pending) in uploads {
            guard latest.revision >= pending.admission.revision else { continue }
            if let upload = latest.uploads.first(where: { $0.id == id }), !upload.sourceReleased { continue }
            // Missing records at/after the acknowledgement revision have been evicted.
            // Rust evicts only finished workers; cancellation alone is not sufficient.
            uploads.removeValue(forKey: id)
            release(pending.prepared)
        }
    }

    private func release(_ prepared: PreparedClipboardImage) {
        cleanupTasks[prepared.identifier] = Task {
            defer { cleanupTasks.removeValue(forKey: prepared.identifier) }
            do { try await preparation.release(prepared) }
            catch { reportError("Image paste cleanup failed: \(error.localizedDescription)") }
        }
    }

    func stopAccepting() async {
        closed = true
        let preparing = Array(tasks.values)
        preparing.forEach { $0.cancel() }
        for task in preparing { await task.value }
    }

    /// Call only after successful Rust shutdown has drained all transfer workers.
    func desktopClosed() async {
        for pending in uploads.values { release(pending.prepared) }
        uploads.removeAll()
        let cleaning = Array(cleanupTasks.values)
        for task in cleaning { await task.value }
    }
}

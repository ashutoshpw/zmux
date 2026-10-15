import Darwin
import Foundation

/// Executes the production Darwin file owner with real files; no replacement FS.
@main
struct DownloadFilesProbe {
    static func require(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
        if try !condition() { throw BrowserDownloadFileFailure(message: message) }
    }
    static func expectFailure(_ operation: () async throws -> Void) async throws {
        do { try await operation() }
        catch is BrowserDownloadFileFailure { return }
        throw BrowserDownloadFileFailure(message: "Expected an explicit owned-filesystem refusal")
    }
    static func main() async throws {
        let requested = FileManager.default.temporaryDirectory
            .appendingPathComponent("zmux-download-files-" + UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: requested, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        do {
            // Foundation path standardization can restore an alias such as /var.
            // Only POSIX realpath establishes the strict walk's actual root.
            let canonicalPath = try realPath(requested.path)
            let root = URL(fileURLWithPath: canonicalPath, isDirectory: true)
            try require(root.path == canonicalPath, "File URL changed the POSIX canonical fixture path")
            let foundationRoot = root.resolvingSymlinksInPath()
            FileHandle.standardError.write(Data(("Download fixture root: " + root.path + "; Foundation-resolved: " + foundationRoot.path + "\n").utf8))
            if foundationRoot.path != canonicalPath {
                try await expectFailure {
                    _ = try await BrowserDownloadFiles().prepare(UUID(), destination: foundationRoot.appendingPathComponent("foundation-alias.bin"))
                }
            }
            try await run(root)
            try FileManager.default.removeItem(at: root)
        } catch {
            // requested remains available even if POSIX realpath itself failed.
            do { try FileManager.default.removeItem(at: requested) }
            catch { FileHandle.standardError.write(Data(("download-files fixture cleanup failed: " + error.localizedDescription + "\n").utf8)) }
            throw error
        }
        print("Download files passed: absent handoff, exact bytes/attributes, no replacement, symlink refusal and owned cancellation cleanup.")
    }
    private static func realPath(_ path: String) throws -> String {
        guard let resolved = Darwin.realpath(path, nil) else {
            throw BrowserDownloadFileFailure(message: "Could not resolve the fixture root (errno " + String(errno) + ").")
        }
        defer { Darwin.free(resolved) }
        return String(cString: resolved)
    }
    static func run(_ root: URL) async throws {
        let files = BrowserDownloadFiles()
        let id = UUID(), final = root.appendingPathComponent("saved.bin")
        try require(final.path == root.path + "/saved.bin", "Appending the final leaf changed the canonical parent")
        let payload = try await files.prepare(id, destination: final)
        try require(!FileManager.default.fileExists(atPath: payload.path), "WebKit handoff must name an absent file")
        let bytes = Data([0, 1, 255, 2, 3])
        try bytes.write(to: payload)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: payload.path)
        let quarantine = Data("0081;00000000;ZmuxFixture;".utf8)
        let set = quarantine.withUnsafeBytes { storage in
            Darwin.setxattr(payload.path, "com.apple.quarantine", storage.baseAddress, storage.count, 0, 0)
        }
        try require(set == 0, "Could not set the quarantine fixture attribute")
        let saved = try await files.publish(id)
        try require(saved.path == final.path && saved.bytes == 5 && saved.cleanupComplete, "Publication receipt differs")
        try require(try Data(contentsOf: final) == bytes, "Published bytes differ")
        var attribute = Data(count: 128)
        let size = attribute.withUnsafeMutableBytes { storage in Darwin.getxattr(final.path, "com.apple.quarantine", storage.baseAddress, storage.count, 0, 0) }
        try require(size == quarantine.count && attribute.prefix(size) == quarantine, "Quarantine attribute was lost")
        try await expectFailure { _ = try await files.prepare(UUID(), destination: final) }
        try require(try Data(contentsOf: final) == bytes, "Existing file was modified")

        let conflictId = UUID(), conflict = root.appendingPathComponent("collision.bin")
        let partial = try await files.prepare(conflictId, destination: conflict)
        try Data("partial".utf8).write(to: partial)
        try Data("user file".utf8).write(to: conflict)
        try await expectFailure { _ = try await files.publish(conflictId) }
        try await files.discard(conflictId)
        try require(try Data(contentsOf: conflict) == Data("user file".utf8), "Collision replaced the user's file")
        try require(!FileManager.default.fileExists(atPath: partial.deletingLastPathComponent().path), "Owned partial directory was left behind")

        let alias = root.appendingPathComponent("alias")
        try FileManager.default.createSymbolicLink(at: alias, withDestinationURL: root)
        try await expectFailure { _ = try await files.prepare(UUID(), destination: alias.appendingPathComponent("unsafe.bin")) }
        let finalLink = root.appendingPathComponent("existing-link")
        try FileManager.default.createSymbolicLink(at: finalLink, withDestinationURL: final)
        try await expectFailure { _ = try await files.prepare(UUID(), destination: finalLink) }

        let linkId = UUID(), linkFinal = root.appendingPathComponent("linked.bin")
        let linkedPayload = try await files.prepare(linkId, destination: linkFinal)
        try FileManager.default.createSymbolicLink(at: linkedPayload, withDestinationURL: final)
        try await expectFailure { _ = try await files.publish(linkId) }
        try await expectFailure { try await files.discard(linkId) }
        try require(try Data(contentsOf: final) == bytes, "Refused symlink changed its target")
        try FileManager.default.removeItem(at: linkedPayload)
        try await files.discard(linkId)

        let cancelId = UUID(), cancelFinal = root.appendingPathComponent("cancelled.bin")
        let cancelledPayload = try await files.prepare(cancelId, destination: cancelFinal)
        try bytes.write(to: cancelledPayload)
        try await files.discard(cancelId)
        try require(!FileManager.default.fileExists(atPath: cancelFinal.path), "Cancellation published a file")
        try require(!FileManager.default.fileExists(atPath: cancelledPayload.deletingLastPathComponent().path), "Cancellation retained its private directory")
    }
}

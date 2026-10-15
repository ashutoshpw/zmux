import Darwin
import Foundation

@main
struct StoragePathsProbe {
    struct Failure: Error { let message: String }

    static func require(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
        if try !condition() { throw Failure(message: message) }
    }

    static func expectFailure(_ path: String, code: Int32? = nil) throws {
        do { _ = try NativeStoragePath.resolve(path) }
        catch NativeStoragePathFailure.invalidPath {
            try require(code == nil, "Expected a filesystem error for \(path)")
            return
        }
        catch NativeStoragePathFailure.system(_, let actual) {
            try require(code == actual, "Unexpected filesystem error \(actual) for \(path)")
            return
        }
        throw Failure(message: "Storage path unexpectedly accepted: \(path)")
    }

    /// The public kernel contract used by the Rust persistence owner, without
    /// creating directories or relaxing any symlink component.
    static func strictDirectoryWalk(_ path: String) -> Int32 {
        let flags = O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC
        var directory = Darwin.open("/", flags)
        guard directory >= 0 else { return errno }
        for component in path.split(separator: "/") {
            let next = String(component).withCString { Darwin.openat(directory, $0, flags) }
            let code = next < 0 ? errno : 0
            if Darwin.close(directory) != 0 { return errno }
            if code != 0 { return code }
            directory = next
        }
        return Darwin.close(directory) == 0 ? 0 : errno
    }

    static func rejectNonUTF8Target(_ root: String) throws {
        let files = FileManager.default
        let target = Array((root + "/invalid-").utf8).map { CChar(bitPattern: $0) } + [CChar(bitPattern: 0xff), 0]
        // A lossy decode would select this different, otherwise usable directory.
        try files.createDirectory(atPath: root + "/invalid-\u{fffd}", withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        let (created, creationCode) = target.withUnsafeBufferPointer { target in
            let result = Darwin.mkdir(target.baseAddress!, 0o700)
            return (result, result < 0 ? errno : 0)
        }
        FileHandle.standardError.write(Data("Raw storage target mkdir: returned=\(created) errno=\(creationCode)\n".utf8))
        if created != 0 {
            // Darwin run 38812458734 rejected this name with EILSEQ (92).
            // No invalid-byte target can be resolved on this fixture filesystem.
            try require(creationCode == EILSEQ, "Cannot create raw-byte storage target (errno \(creationCode))")
            FileHandle.standardError.write(Data("Raw storage target: filesystem refused illegal byte sequence; resolved-path UTF8 validation remains required.\n".utf8))
            return
        }
        do {
            let alias = root + "/non-utf8-link"
            let created = target.withUnsafeBufferPointer { target in
                alias.withCString { Darwin.symlink(target.baseAddress!, $0) }
            }
            try require(created == 0, "Cannot create raw-byte storage alias")
            try expectFailure(alias)
            try require(target.withUnsafeBufferPointer { Darwin.rmdir($0.baseAddress!) } == 0,
                        "Cannot remove raw-byte storage target")
        } catch {
            if target.withUnsafeBufferPointer({ Darwin.rmdir($0.baseAddress!) }) != 0 && errno != ENOENT {
                FileHandle.standardError.write(Data("Raw storage target cleanup failed (errno \(errno)).\n".utf8))
            }
            throw error
        }
    }

    static func main() throws {
        let root = "/private/tmp/zmux-storage-paths-" + UUID().uuidString
        let files = FileManager.default
        try files.createDirectory(atPath: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        do {
            let requested = root + "/profile/nested"
            let firstLaunch = try NativeStoragePath.resolve(requested)
            try require(firstLaunch == requested, "Missing profile lost its physical parent spelling")
            try files.createDirectory(atPath: firstLaunch, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            let reopen = try NativeStoragePath.resolve(requested)
            try require(reopen == firstLaunch, "Existing profile changed path on reopen")
            try require(strictDirectoryWalk(reopen) == 0, "Physical profile rejected by nofollow directory walk")
            let alias = "/tmp/" + String(requested.dropFirst("/private/tmp/".count))
            try require(try NativeStoragePath.resolve(alias) == reopen, "OS alias did not resolve to physical profile")
            let legacy = URL(fileURLWithPath: requested, isDirectory: true).resolvingSymlinksInPath().path
            try require(legacy != reopen && strictDirectoryWalk(legacy) == ENOTDIR,
                        "Foundation control did not reproduce the existing /private alias rejection")
            for invalid in ["relative/profile", root + "/../escape", root + "/./profile", root + "/nul\0suffix"] {
                try expectFailure(invalid)
            }
            let file = root + "/regular-file"
            try Data([1]).write(to: URL(fileURLWithPath: file))
            try expectFailure(file, code: ENOTDIR)
            try expectFailure(file + "/missing", code: ENOTDIR)
            let loop = root + "/loop"
            try files.createSymbolicLink(atPath: loop, withDestinationPath: loop)
            try expectFailure(loop + "/missing", code: ELOOP)
            try rejectNonUTF8Target(root)
            try files.removeItem(atPath: root)
        } catch {
            do { try files.removeItem(atPath: root) }
            catch { FileHandle.standardError.write(Data("Storage fixture cleanup failed: \(error)\n".utf8)) }
            throw error
        }
        print("Storage paths passed: missing launch, existing reopen, physical alias, strict nofollow control and invalid-path/raw-byte refusal.")
    }
}

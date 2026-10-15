import Darwin
import Foundation

struct BrowserDownloadFileFailure: Error, LocalizedError, Sendable {
    let message: String
    var errorDescription: String? { message }
}

struct PublishedBrowserDownload: Sendable {
    let path: String
    let bytes: UInt64
    let cleanupError: String?
    let cleanupComplete: Bool
}

/// File ownership remains behind one actor. WebKit writes only an absent payload
/// in our private staging directory. Final insertion is atomic and never replaces.
actor BrowserDownloadFiles {
    private struct Placement {
        let parent: Int32
        let staging: Int32
        let stagingName: String
        let finalName: String
        let finalURL: URL
        let payloadURL: URL
        let device: dev_t
        let inode: ino_t
    }
    private var placements: [UUID: Placement] = [:]
    private var unresolvedDirectories: Set<UUID> = []

    func prepare(_ id: UUID, destination: URL) throws -> URL {
        guard placements.count + unresolvedDirectories.count < 4, placements[id] == nil,
              destination.isFileURL, destination.path.hasPrefix("/"),
              destination.path.utf8.count <= 2048,
              !destination.path.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else {
            throw failure("The selected download destination is invalid or capacity is full.")
        }
        let name = destination.lastPathComponent
        guard try JSONEncoder().encode(destination.path).count <= 4096 else { throw failure("The selected destination path is too long.") }
        guard !name.isEmpty, ![".", ".."].contains(name), name.utf8.count <= 255,
              !name.contains("/"), !name.contains("\\") else { throw failure("Choose a valid filename.") }
        let flags = O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC
        var parent = Darwin.open("/", flags)
        guard parent >= 0 else { throw systemFailure("open destination root") }
        do {
            for component in destination.deletingLastPathComponent().pathComponents.dropFirst() {
                guard component != ".", component != ".." else { throw failure("Destination traversal is not allowed.") }
                let next = component.withCString { Darwin.openat(parent, $0, flags) }
                guard next >= 0 else { throw systemFailure("open destination directory (choose a path without symlink components)") }
                Darwin.close(parent)
                parent = next
            }
            var existing = stat()
            let inspected = name.withCString { Darwin.fstatat(parent, $0, &existing, AT_SYMLINK_NOFOLLOW) }
            guard inspected != 0, errno == ENOENT else { throw failure("The destination already exists or cannot be inspected. Choose a new filename.") }
            let stagingName = ".zmux-download-" + id.uuidString.lowercased()
            guard stagingName.withCString({ Darwin.mkdirat(parent, $0, 0o700) }) == 0 else { throw systemFailure("create private download directory") }
            unresolvedDirectories.insert(id)
            let staging = stagingName.withCString { Darwin.openat(parent, $0, flags) }
            guard staging >= 0 else {
                let primary = systemFailure("open private download directory")
                if stagingName.withCString({ Darwin.unlinkat(parent, $0, AT_REMOVEDIR) }) != 0 {
                    throw failure(primary.localizedDescription + "; private-directory cleanup failed")
                }
                unresolvedDirectories.remove(id)
                throw primary
            }
            var info = stat()
            guard Darwin.fstat(staging, &info) == 0, info.st_uid == geteuid(),
                  info.st_mode & 0o777 == 0o700 else {
                Darwin.close(staging)
                throw failure("The private download directory has unexpected ownership.")
            }
            let payload = destination.deletingLastPathComponent().appendingPathComponent(stagingName, isDirectory: true).appendingPathComponent("payload")
            placements[id] = Placement(parent: parent, staging: staging, stagingName: stagingName,
                                       finalName: name, finalURL: destination, payloadURL: payload,
                                       device: info.st_dev, inode: info.st_ino)
            unresolvedDirectories.remove(id)
            return payload
        } catch {
            Darwin.close(parent)
            throw error
        }
    }

    func publish(_ id: UUID) throws -> PublishedBrowserDownload {
        guard let placement = placements[id] else { throw failure("The owned download destination is unavailable.") }
        try validate(placement)
        let payload = Darwin.openat(placement.staging, "payload", O_RDONLY | O_NOFOLLOW | O_CLOEXEC)
        guard payload >= 0 else { throw systemFailure("open downloaded file") }
        defer { Darwin.close(payload) }
        var info = stat()
        guard Darwin.fstat(payload, &info) == 0, info.st_mode & S_IFMT == S_IFREG,
              info.st_uid == geteuid(), info.st_nlink == 1, info.st_size >= 0 else {
            throw failure("The downloaded file is not an owned regular file.")
        }
        guard Darwin.fsync(payload) == 0 else { throw systemFailure("sync downloaded file") }
        let inserted = placement.finalName.withCString {
            Darwin.renameatx_np(placement.staging, "payload", placement.parent, $0, UInt32(RENAME_EXCL))
        }
        guard inserted == 0 else { throw systemFailure("save without replacing an existing destination") }
        // Publication has won. Cleanup errors must preserve/report the saved file,
        // never turn a late cancellation into deletion of that final destination.
        let cleanupError: String?
        let cleanupComplete: Bool
        if placement.stagingName.withCString({ Darwin.unlinkat(placement.parent, $0, AT_REMOVEDIR) }) != 0 {
            cleanupError = "The file was saved, but its private staging directory could not be removed."
            cleanupComplete = false
        } else if Darwin.fsync(placement.parent) != 0 {
            cleanupError = "The file was saved, but syncing its destination directory failed."
            cleanupComplete = true
        } else { cleanupError = nil; cleanupComplete = true }
        if cleanupComplete {
            placements.removeValue(forKey: id)
            Darwin.close(placement.staging)
            Darwin.close(placement.parent)
        }
        return PublishedBrowserDownload(path: placement.finalURL.path, bytes: UInt64(info.st_size), cleanupError: cleanupError, cleanupComplete: cleanupComplete)
    }

    func discard(_ id: UUID) throws {
        guard !unresolvedDirectories.contains(id) else { throw failure("An owned staging directory could not be verified; cleanup requires attention.") }
        guard let placement = placements[id] else { return }
        try validate(placement)
        var info = stat()
        if Darwin.fstatat(placement.staging, "payload", &info, AT_SYMLINK_NOFOLLOW) == 0 {
            guard info.st_mode & S_IFMT == S_IFREG, info.st_uid == geteuid(), info.st_nlink == 1 else {
                throw failure("Cleanup refused a substituted download payload.")
            }
            guard Darwin.unlinkat(placement.staging, "payload", 0) == 0 else { throw systemFailure("remove owned partial download") }
        } else if errno != ENOENT { throw systemFailure("inspect partial download") }
        guard placement.stagingName.withCString({ Darwin.unlinkat(placement.parent, $0, AT_REMOVEDIR) }) == 0 else {
            throw systemFailure("remove owned download directory")
        }
        placements.removeValue(forKey: id)
        Darwin.close(placement.staging)
        Darwin.close(placement.parent)
    }

    private func validate(_ placement: Placement) throws {
        var info = stat()
        guard placement.stagingName.withCString({ Darwin.fstatat(placement.parent, $0, &info, AT_SYMLINK_NOFOLLOW) }) == 0,
              info.st_dev == placement.device, info.st_ino == placement.inode,
              info.st_mode & S_IFMT == S_IFDIR, info.st_uid == geteuid(), info.st_mode & 0o777 == 0o700 else {
            throw failure("The owned staging directory was replaced; cleanup/publication was refused.")
        }
        // This reopen may resolve intermediate aliases, but its final directory
        // must match the retained parent descriptor. Publication/cleanup use that
        // descriptor; they cannot be redirected to a substituted directory.
        let flags = O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC
        let current = Darwin.open(placement.finalURL.deletingLastPathComponent().path, flags)
        guard current >= 0 else { throw systemFailure("verify destination directory") }
        defer { Darwin.close(current) }
        var expected = stat(), actual = stat()
        guard Darwin.fstat(placement.parent, &expected) == 0, Darwin.fstat(current, &actual) == 0,
              actual.st_dev == expected.st_dev, actual.st_ino == expected.st_ino else {
            throw failure("The destination directory changed during the download.")
        }
    }
    private func failure(_ message: String) -> BrowserDownloadFileFailure { BrowserDownloadFileFailure(message: message) }
    private func systemFailure(_ operation: String) -> BrowserDownloadFileFailure {
        BrowserDownloadFileFailure(message: "Could not " + operation + " (errno " + String(errno) + ").")
    }
}

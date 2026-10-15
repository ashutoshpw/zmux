import Darwin
import Foundation
import ImageIO
import UniformTypeIdentifiers

private final class ClipboardDirectoryEnumerationFailure: @unchecked Sendable {
    private let lock = NSLock()
    private var storedDescription: String?

    func store(_ description: String) {
        lock.lock()
        storedDescription = description
        lock.unlock()
    }

    func description() -> String? {
        lock.lock()
        defer { lock.unlock() }
        return storedDescription
    }
}

struct PreparedClipboardImage: Hashable, Sendable {
    let identifier: UUID
    let fileURL: URL
    let byteCount: Int
    let pixelWidth: Int
    let pixelHeight: Int

    var path: String { fileURL.path }

    fileprivate init(identifier: UUID, fileURL: URL, byteCount: Int, pixelWidth: Int, pixelHeight: Int) {
        self.identifier = identifier
        self.fileURL = fileURL
        self.byteCount = byteCount
        self.pixelWidth = pixelWidth
        self.pixelHeight = pixelHeight
    }
}

enum ClipboardImagePreparationError: Error, LocalizedError, Sendable {
    case emptySource
    case sourceTooLarge(actual: Int, limit: Int)
    case unsupportedImage
    case incompleteImage
    case frameCount(Int)
    case missingDimensions
    case invalidDimensions(width: Int, height: Int)
    case pixelLimit(width: Int, height: Int, limit: Int)
    case decodedDimensions(metadataWidth: Int, metadataHeight: Int, actualWidth: Int, actualHeight: Int)
    case pngEncodingFailed
    case encodedImageTooLarge(actual: Int, limit: Int)
    case cacheUnavailable(String)
    case cacheIntegrity(String)
    case cacheFull(used: Int, required: Int, limit: Int)
    case cacheEntryLimit(Int)
    case invalidRelease
    case fileSystem(operation: String, reason: String)
    case cleanupFailed(primary: String, cleanup: String)

    var errorDescription: String? {
        switch self {
        case .emptySource:
            return "The clipboard image is empty."
        case .sourceTooLarge(let actual, let limit):
            return "The clipboard image is \(Self.size(actual)); the input limit is \(Self.size(limit))."
        case .unsupportedImage:
            return "The clipboard data is not a supported image."
        case .incompleteImage:
            return "The clipboard image is incomplete or malformed."
        case .frameCount(let count):
            return "Clipboard image paste accepts one still image; the source contains \(count) frames."
        case .missingDimensions:
            return "The clipboard image does not provide valid pixel dimensions."
        case .invalidDimensions(let width, let height):
            return "The clipboard image dimensions \(width)×\(height) exceed the 16,384-pixel dimension limit."
        case .pixelLimit(let width, let height, let limit):
            return "The clipboard image dimensions \(width)×\(height) exceed the \(limit)-pixel limit."
        case .decodedDimensions(let metadataWidth, let metadataHeight, let actualWidth, let actualHeight):
            return "The decoded image dimensions \(actualWidth)×\(actualHeight) do not match its metadata \(metadataWidth)×\(metadataHeight)."
        case .pngEncodingFailed:
            return "The clipboard image could not be encoded as PNG."
        case .encodedImageTooLarge(let actual, let limit):
            return "The prepared PNG is \(Self.size(actual)); the output limit is \(Self.size(limit))."
        case .cacheUnavailable(let reason):
            return "The private clipboard image cache is unavailable: \(reason)"
        case .cacheIntegrity(let reason):
            return "The private clipboard image cache is unsafe to use: \(reason)"
        case .cacheFull(let used, let required, let limit):
            return "The clipboard image cache is full (\(Self.size(used)) used, \(Self.size(required)) required, \(Self.size(limit)) limit)."
        case .cacheEntryLimit(let limit):
            return "The clipboard image cache already contains \(limit) retained images."
        case .invalidRelease:
            return "The prepared clipboard image does not match the active cache entry."
        case .fileSystem(let operation, let reason):
            return "Could not \(operation): \(reason)"
        case .cleanupFailed(let primary, let cleanup):
            return "Clipboard image preparation failed (\(primary)); cleanup also failed (\(cleanup))."
        }
    }

    private static func size(_ bytes: Int) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
    }
}

/// Prepares untrusted clipboard image bytes away from AppKit's main actor.
///
/// Keep locally inserted results without calling `release`; they remain protected for the
/// lifetime of this actor and are retained on disk for at least seven days across launches.
/// Call `release` after a remote upload finishes or is abandoned to remove its private file.
actor ClipboardImagePreparation {
    static let maximumSourceByteCount = 32 * 1_024 * 1_024
    static let maximumEncodedByteCount = 32 * 1_024 * 1_024
    static let maximumDimension = 16_384
    static let maximumPixelCount = 16_000_000
    static let maximumCacheByteCount = 256 * 1_024 * 1_024
    static let retention: TimeInterval = 7 * 24 * 60 * 60

    private static let maximumCacheEntryCount = 4_096
    private static let applicationCacheComponent = "app.w3dev.zmux.dev"
    private static let imageCacheComponent = "ClipboardImages"
    private static let imageFileName = "image.png"

    private struct EncodedImage {
        let data: Data
        let width: Int
        let height: Int
    }

    private struct CacheState {
        let byteCount: Int
        let entryCount: Int
    }

    private var active: [UUID: PreparedClipboardImage] = [:]

    func prepare(data: Data) async throws -> PreparedClipboardImage {
        guard !data.isEmpty else { throw ClipboardImagePreparationError.emptySource }
        guard data.count <= Self.maximumSourceByteCount else {
            throw ClipboardImagePreparationError.sourceTooLarge(actual: data.count, limit: Self.maximumSourceByteCount)
        }
        try Task.checkCancellation()

        let encoded = try encodePNG(from: data)
        try Task.checkCancellation()

        let root = try cacheRoot()
        let cacheLock = try lockCache(root)
        defer { Darwin.close(cacheLock) }
        let before = try pruneExpiredAndMeasure(in: root, protecting: [])
        guard before.entryCount < Self.maximumCacheEntryCount else {
            throw ClipboardImagePreparationError.cacheEntryLimit(Self.maximumCacheEntryCount)
        }
        guard before.byteCount <= Self.maximumCacheByteCount - encoded.data.count else {
            throw ClipboardImagePreparationError.cacheFull(
                used: before.byteCount,
                required: encoded.data.count,
                limit: Self.maximumCacheByteCount
            )
        }

        let identifier = UUID()
        let directory = imageDirectory(for: identifier, in: root)
        let fileURL = directory.appendingPathComponent(Self.imageFileName, isDirectory: false)
        var directoryCreated = false

        do {
            try createPrivateDirectory(at: directory)
            directoryCreated = true
            try writePrivateFile(encoded.data, to: fileURL)
            try Task.checkCancellation()

            let after = try pruneExpiredAndMeasure(in: root, protecting: [identifier])
            guard after.entryCount <= Self.maximumCacheEntryCount,
                  after.byteCount <= Self.maximumCacheByteCount else {
                throw ClipboardImagePreparationError.cacheFull(
                    used: max(0, after.byteCount - encoded.data.count),
                    required: encoded.data.count,
                    limit: Self.maximumCacheByteCount
                )
            }

            let prepared = PreparedClipboardImage(
                identifier: identifier,
                fileURL: fileURL,
                byteCount: encoded.data.count,
                pixelWidth: encoded.width,
                pixelHeight: encoded.height
            )
            active[identifier] = prepared
            return prepared
        } catch {
            guard directoryCreated else { throw error }
            do {
                try removeOwnedImage(identifier: identifier, from: root)
            } catch let cleanupError {
                throw ClipboardImagePreparationError.cleanupFailed(
                    primary: Self.describe(error),
                    cleanup: Self.describe(cleanupError)
                )
            }
            throw error
        }
    }

    func release(_ prepared: PreparedClipboardImage) async throws {
        guard let tracked = active[prepared.identifier] else { return }
        guard tracked == prepared else { throw ClipboardImagePreparationError.invalidRelease }

        let root = try cacheRoot()
        let cacheLock = try lockCache(root)
        defer { Darwin.close(cacheLock) }
        guard prepared.fileURL.standardizedFileURL == imageFileURL(for: prepared.identifier, in: root).standardizedFileURL else {
            throw ClipboardImagePreparationError.invalidRelease
        }
        try removeOwnedImage(identifier: prepared.identifier, from: root)
        active.removeValue(forKey: prepared.identifier)
    }

    private func encodePNG(from data: Data) throws -> EncodedImage {
        let sourceOptions = [kCGImageSourceShouldCache as String: false] as CFDictionary
        guard let source = CGImageSourceCreateWithData(data as CFData, sourceOptions),
              let sourceIdentifier = CGImageSourceGetType(source),
              let sourceType = UTType(sourceIdentifier as String),
              sourceType.conforms(to: .image) else {
            throw ClipboardImagePreparationError.unsupportedImage
        }
        guard CGImageSourceGetStatus(source) == .statusComplete else {
            throw ClipboardImagePreparationError.incompleteImage
        }

        let frameCount = CGImageSourceGetCount(source)
        guard frameCount == 1 else { throw ClipboardImagePreparationError.frameCount(frameCount) }
        guard CGImageSourceGetStatusAtIndex(source, 0) == .statusComplete,
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, sourceOptions),
              let width = Self.dimension(kCGImagePropertyPixelWidth, in: properties),
              let height = Self.dimension(kCGImagePropertyPixelHeight, in: properties) else {
            throw ClipboardImagePreparationError.missingDimensions
        }
        try Self.validate(width: width, height: height)
        try Task.checkCancellation()

        let orientation = ((properties as NSDictionary)[kCGImagePropertyOrientation] as? NSNumber)?.intValue ?? 1
        guard (1...8).contains(orientation) else { throw ClipboardImagePreparationError.unsupportedImage }
        let rotated = (5...8).contains(orientation)
        let expectedWidth = rotated ? height : width
        let expectedHeight = rotated ? width : height
        // Normalize orientation into the PNG pixels rather than dropping EXIF orientation.
        // Always generate from the full image; the original longest edge prevents scaling.
        let decodeOptions: [String: Any] = [
            kCGImageSourceShouldCache as String: true,
            kCGImageSourceShouldCacheImmediately as String: true,
            kCGImageSourceCreateThumbnailFromImageAlways as String: true,
            kCGImageSourceCreateThumbnailWithTransform as String: true,
            kCGImageSourceThumbnailMaxPixelSize as String: max(width, height)
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, decodeOptions as CFDictionary) else {
            throw ClipboardImagePreparationError.incompleteImage
        }
        try Self.validate(width: image.width, height: image.height)
        guard image.width == expectedWidth, image.height == expectedHeight else {
            throw ClipboardImagePreparationError.decodedDimensions(
                metadataWidth: expectedWidth,
                metadataHeight: expectedHeight,
                actualWidth: image.width,
                actualHeight: image.height
            )
        }
        try Task.checkCancellation()

        guard let output = CFDataCreateMutable(kCFAllocatorDefault, 0),
              let destination = CGImageDestinationCreateWithData(
                  output,
                  UTType.png.identifier as CFString,
                  1,
                  nil
              ) else {
            throw ClipboardImagePreparationError.pngEncodingFailed
        }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination) else {
            throw ClipboardImagePreparationError.pngEncodingFailed
        }

        let encoded = output as Data
        guard !encoded.isEmpty else { throw ClipboardImagePreparationError.pngEncodingFailed }
        guard encoded.count <= Self.maximumEncodedByteCount else {
            throw ClipboardImagePreparationError.encodedImageTooLarge(
                actual: encoded.count,
                limit: Self.maximumEncodedByteCount
            )
        }
        return EncodedImage(data: encoded, width: image.width, height: image.height)
    }

    private static func dimension(_ key: CFString, in properties: CFDictionary) -> Int? {
        guard let number = (properties as NSDictionary)[key] as? NSNumber else { return nil }
        let value = number.doubleValue
        // Double(Int.max) rounds upward on 64-bit platforms; accepting equality could
        // trap during Int conversion for maliciously large image metadata.
        guard value.isFinite, value > 0, value < Double(Int.max), value.rounded(.towardZero) == value else {
            return nil
        }
        return Int(value)
    }

    private static func validate(width: Int, height: Int) throws {
        guard width > 0, height > 0, width <= maximumDimension, height <= maximumDimension else {
            throw ClipboardImagePreparationError.invalidDimensions(width: width, height: height)
        }
        guard width <= maximumPixelCount / height else {
            throw ClipboardImagePreparationError.pixelLimit(width: width, height: height, limit: maximumPixelCount)
        }
    }

    private func cacheRoot() throws -> URL {
        let manager = FileManager.default
        let base: URL
        do {
            base = try manager.url(
                for: .cachesDirectory,
                in: .userDomainMask,
                appropriateFor: nil,
                create: true
            ).standardizedFileURL
        } catch {
            throw ClipboardImagePreparationError.cacheUnavailable(Self.describe(error))
        }

        let applicationDirectory = base.appendingPathComponent(Self.applicationCacheComponent, isDirectory: true)
        let root = applicationDirectory.appendingPathComponent(Self.imageCacheComponent, isDirectory: true)
        try ensurePrivateDirectory(at: applicationDirectory)
        try ensurePrivateDirectory(at: root)
        return root
    }

    /// Multiple development instances share this cache. Never prune another instance's
    /// partially written entry; a nonblocking directory lock keeps waits bounded.
    private func lockCache(_ root: URL) throws -> Int32 {
        let descriptor = root.withUnsafeFileSystemRepresentation { path in
            guard let path else { errno = EINVAL; return Int32(-1) }
            return Darwin.open(path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        }
        guard descriptor >= 0 else {
            throw ClipboardImagePreparationError.fileSystem(operation: "open the clipboard cache lock", reason: String(cString: strerror(errno)))
        }
        guard flock(descriptor, LOCK_EX | LOCK_NB) == 0 else {
            let code = errno
            Darwin.close(descriptor)
            throw ClipboardImagePreparationError.fileSystem(operation: "lock the clipboard image cache", reason: code == EWOULDBLOCK ? "another instance is using it; paste again shortly" : String(cString: strerror(code)))
        }
        return descriptor
    }

    private func ensurePrivateDirectory(at url: URL) throws {
        let manager = FileManager.default
        if !manager.fileExists(atPath: url.path) {
            do {
                try manager.createDirectory(
                    at: url,
                    withIntermediateDirectories: false,
                    attributes: [.posixPermissions: 0o700]
                )
            } catch {
                throw ClipboardImagePreparationError.fileSystem(
                    operation: "create the private clipboard image directory",
                    reason: Self.describe(error)
                )
            }
        }
        try validateDirectory(url)
        do {
            try manager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: url.path)
        } catch {
            throw ClipboardImagePreparationError.fileSystem(
                operation: "secure the private clipboard image directory",
                reason: Self.describe(error)
            )
        }
        try validateDirectory(url, permissions: 0o700)
    }

    private func createPrivateDirectory(at url: URL) throws {
        do {
            try FileManager.default.createDirectory(
                at: url,
                withIntermediateDirectories: false,
                attributes: [.posixPermissions: 0o700]
            )
        } catch {
            throw ClipboardImagePreparationError.fileSystem(
                operation: "create a private clipboard image entry",
                reason: Self.describe(error)
            )
        }
        try validateDirectory(url, permissions: 0o700)
    }

    private func validateDirectory(_ url: URL, permissions: Int? = nil) throws {
        do {
            let values = try url.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
            guard values.isDirectory == true, values.isSymbolicLink != true else {
                throw ClipboardImagePreparationError.cacheIntegrity("an owned cache directory is missing or is a symbolic link")
            }
            let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
            guard let owner = attributes[.ownerAccountID] as? NSNumber,
                  owner.uint32Value == getuid() else {
                throw ClipboardImagePreparationError.cacheIntegrity("an owned cache directory does not belong to the current user")
            }
            if let permissions {
                guard let mode = attributes[.posixPermissions] as? NSNumber,
                      mode.intValue & 0o7777 == permissions else {
                    throw ClipboardImagePreparationError.cacheIntegrity("an owned cache directory does not have mode 0700")
                }
            }
        } catch let error as ClipboardImagePreparationError {
            throw error
        } catch {
            throw ClipboardImagePreparationError.fileSystem(
                operation: "inspect the private clipboard image directory",
                reason: Self.describe(error)
            )
        }
    }

    private func writePrivateFile(_ data: Data, to url: URL) throws {
        do {
            try data.write(to: url, options: .withoutOverwriting)
            let createdValues = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
            let createdAttributes = try FileManager.default.attributesOfItem(atPath: url.path)
            guard createdValues.isRegularFile == true, createdValues.isSymbolicLink != true,
                  let createdOwner = createdAttributes[.ownerAccountID] as? NSNumber,
                  createdOwner.uint32Value == getuid() else {
                throw ClipboardImagePreparationError.cacheIntegrity("a newly prepared PNG is not a current-user regular file")
            }
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
            let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
            let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
            guard values.isRegularFile == true, values.isSymbolicLink != true,
                  values.fileSize == data.count,
                  let owner = attributes[.ownerAccountID] as? NSNumber,
                  owner.uint32Value == getuid(),
                  let mode = attributes[.posixPermissions] as? NSNumber,
                  mode.intValue & 0o7777 == 0o600 else {
                throw ClipboardImagePreparationError.cacheIntegrity("a prepared PNG is not a private regular file")
            }
        } catch let error as ClipboardImagePreparationError {
            throw error
        } catch {
            throw ClipboardImagePreparationError.fileSystem(
                operation: "write the prepared clipboard image",
                reason: Self.describe(error)
            )
        }
    }

    private func boundedContents(
        of directory: URL,
        includingPropertiesForKeys keys: [URLResourceKey],
        maximumCount: Int,
        overflowError: ClipboardImagePreparationError,
        operation: String
    ) throws -> [URL] {
        let failure = ClipboardDirectoryEnumerationFailure()
        guard let enumerator = FileManager.default.enumerator(
            at: directory,
            includingPropertiesForKeys: keys,
            options: [.skipsSubdirectoryDescendants],
            errorHandler: { _, error in
                failure.store(error.localizedDescription)
                return false
            }
        ) else {
            throw ClipboardImagePreparationError.fileSystem(
                operation: operation,
                reason: "Foundation could not enumerate the directory"
            )
        }

        var entries: [URL] = []
        while let item = enumerator.nextObject() {
            guard let url = item as? URL else {
                throw ClipboardImagePreparationError.cacheIntegrity("directory enumeration returned an invalid entry")
            }
            entries.append(url)
            guard entries.count <= maximumCount else { throw overflowError }
        }
        if let reason = failure.description() {
            throw ClipboardImagePreparationError.fileSystem(operation: operation, reason: reason)
        }
        return entries
    }

    private func pruneExpiredAndMeasure(in root: URL, protecting: Set<UUID>) throws -> CacheState {
        try validateDirectory(root, permissions: 0o700)
        let entries = try boundedContents(
            of: root,
            includingPropertiesForKeys: [.isDirectoryKey, .isSymbolicLinkKey],
            maximumCount: Self.maximumCacheEntryCount,
            overflowError: .cacheEntryLimit(Self.maximumCacheEntryCount),
            operation: "inspect the clipboard image cache"
        )

        var bytes = 0
        var count = 0
        let expiration = Date().addingTimeInterval(-Self.retention)
        for directory in entries {
            guard let identifier = Self.identifier(for: directory.lastPathComponent) else {
                throw ClipboardImagePreparationError.cacheIntegrity("the cache contains an unknown entry")
            }
            try validateDirectory(directory, permissions: 0o700)

            let contents = try boundedContents(
                of: directory,
                includingPropertiesForKeys: [
                    .isRegularFileKey,
                    .isSymbolicLinkKey,
                    .fileSizeKey,
                    .contentModificationDateKey
                ],
                maximumCount: 1,
                overflowError: .cacheIntegrity("a cache entry contains an unknown file"),
                operation: "inspect a clipboard image cache entry"
            )

            let isProtected = active[identifier] != nil || protecting.contains(identifier)
            if contents.isEmpty {
                guard !isProtected else {
                    throw ClipboardImagePreparationError.cacheIntegrity("an active cache entry has no image file")
                }
                try removeEmptyOwnedDirectory(directory)
                continue
            }
            guard contents.count == 1, contents[0].lastPathComponent == Self.imageFileName else {
                throw ClipboardImagePreparationError.cacheIntegrity("a cache entry contains an unknown file")
            }

            let fileURL = contents[0]
            let values: URLResourceValues
            let attributes: [FileAttributeKey: Any]
            do {
                values = try fileURL.resourceValues(forKeys: [
                    .isRegularFileKey,
                    .isSymbolicLinkKey,
                    .fileSizeKey,
                    .contentModificationDateKey
                ])
                attributes = try FileManager.default.attributesOfItem(atPath: fileURL.path)
            } catch {
                throw ClipboardImagePreparationError.fileSystem(
                    operation: "inspect a prepared clipboard image",
                    reason: Self.describe(error)
                )
            }
            guard values.isRegularFile == true, values.isSymbolicLink != true,
                  let fileSize = values.fileSize, fileSize > 0,
                  let modificationDate = values.contentModificationDate,
                  let owner = attributes[.ownerAccountID] as? NSNumber,
                  owner.uint32Value == getuid(),
                  let mode = attributes[.posixPermissions] as? NSNumber,
                  mode.intValue & 0o7777 == 0o600 else {
                throw ClipboardImagePreparationError.cacheIntegrity("a cached image is not a valid regular file")
            }
            guard fileSize <= Self.maximumEncodedByteCount else {
                throw ClipboardImagePreparationError.cacheIntegrity("a cached image exceeds the encoded PNG limit")
            }

            if !isProtected, modificationDate < expiration {
                try removeValidatedOwnedImage(fileURL: fileURL, directory: directory)
                continue
            }
            let (sum, overflow) = bytes.addingReportingOverflow(fileSize)
            guard !overflow else {
                throw ClipboardImagePreparationError.cacheIntegrity("the cache size cannot be represented safely")
            }
            bytes = sum
            count += 1
        }
        return CacheState(byteCount: bytes, entryCount: count)
    }

    private func removeOwnedImage(identifier: UUID, from root: URL) throws {
        try validateDirectory(root, permissions: 0o700)
        let expectedDirectory = imageDirectory(for: identifier, in: root).standardizedFileURL
        let entries = try boundedContents(
            of: root,
            includingPropertiesForKeys: [.isDirectoryKey, .isSymbolicLinkKey],
            maximumCount: Self.maximumCacheEntryCount,
            overflowError: .cacheEntryLimit(Self.maximumCacheEntryCount),
            operation: "inspect the clipboard image cache for cleanup"
        )
        guard let directory = entries.first(where: { $0.standardizedFileURL == expectedDirectory }) else { return }
        try validateDirectory(directory, permissions: 0o700)

        let contents = try boundedContents(
            of: directory,
            includingPropertiesForKeys: [.isRegularFileKey, .isSymbolicLinkKey],
            maximumCount: 1,
            overflowError: .cacheIntegrity("cleanup found an unknown file in a cache entry"),
            operation: "inspect a clipboard image entry for cleanup"
        )
        if contents.isEmpty {
            try removeEmptyOwnedDirectory(directory)
            return
        }
        guard contents.count == 1, contents[0].lastPathComponent == Self.imageFileName else {
            throw ClipboardImagePreparationError.cacheIntegrity("cleanup found an unknown file in a cache entry")
        }
        try removeValidatedOwnedImage(fileURL: contents[0], directory: directory)
    }

    private func removeValidatedOwnedImage(fileURL: URL, directory: URL) throws {
        do {
            let values = try fileURL.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
            let attributes = try FileManager.default.attributesOfItem(atPath: fileURL.path)
            guard values.isRegularFile == true, values.isSymbolicLink != true,
                  let owner = attributes[.ownerAccountID] as? NSNumber,
                  owner.uint32Value == getuid() else {
                throw ClipboardImagePreparationError.cacheIntegrity("cleanup refused a non-regular or unowned cached image")
            }
        } catch let error as ClipboardImagePreparationError {
            throw error
        } catch {
            throw ClipboardImagePreparationError.fileSystem(
                operation: "inspect a clipboard image before cleanup",
                reason: Self.describe(error)
            )
        }
        let unlinkResult = fileURL.withUnsafeFileSystemRepresentation { path in
            guard let path else { errno = EINVAL; return -1 }
            return Darwin.unlink(path)
        }
        guard unlinkResult == 0 else {
            let code = errno
            throw ClipboardImagePreparationError.fileSystem(
                operation: "remove a prepared clipboard image",
                reason: String(cString: strerror(code))
            )
        }
        try removeEmptyOwnedDirectory(directory)
    }

    private func removeEmptyOwnedDirectory(_ directory: URL) throws {
        try validateDirectory(directory, permissions: 0o700)
        _ = try boundedContents(
            of: directory,
            includingPropertiesForKeys: [],
            maximumCount: 0,
            overflowError: .cacheIntegrity("cleanup refused a non-empty cache directory"),
            operation: "inspect an empty clipboard image directory"
        )
        let removeResult = directory.withUnsafeFileSystemRepresentation { path in
            guard let path else { errno = EINVAL; return -1 }
            return Darwin.rmdir(path)
        }
        guard removeResult == 0 else {
            let code = errno
            throw ClipboardImagePreparationError.fileSystem(
                operation: "remove an empty clipboard image directory",
                reason: String(cString: strerror(code))
            )
        }
    }

    private func imageDirectory(for identifier: UUID, in root: URL) -> URL {
        root.appendingPathComponent(identifier.uuidString.lowercased(), isDirectory: true)
    }

    private func imageFileURL(for identifier: UUID, in root: URL) -> URL {
        imageDirectory(for: identifier, in: root).appendingPathComponent(Self.imageFileName, isDirectory: false)
    }

    private static func identifier(for component: String) -> UUID? {
        guard let identifier = UUID(uuidString: component),
              identifier.uuidString.lowercased() == component else { return nil }
        return identifier
    }

    private static func describe(_ error: any Error) -> String {
        if let description = (error as? any LocalizedError)?.errorDescription, !description.isEmpty {
            return description
        }
        return error.localizedDescription
    }
}

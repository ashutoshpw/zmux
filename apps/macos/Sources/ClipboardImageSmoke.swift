import AppKit
import CoreGraphics
import Darwin
import Foundation
import ImageIO
import UniformTypeIdentifiers

private struct ClipboardImageSmokeFailure: Error, LocalizedError, Sendable {
    let message: String
    var errorDescription: String? { message }
}

/// Native owner-boundary checks that require AppKit, ImageIO and a real user cache.
enum ClipboardImageSmoke {
    private static let fixtureWidth = 2
    private static let fixtureHeight = 2

    /// Validates clipboard classification and private image preparation, then returns the
    /// fixture for the packaged app's end-to-end local paste check.
    @MainActor
    static func validate(pane: Pane) async throws -> Data {
        let fixture = try await Task.detached {
            try makeFixturePNG()
        }.value
        let orientedFixture = try await Task.detached {
            try makeOrientationTIFF()
        }.value

        let preparation = ClipboardImagePreparation()
        try await validateRejectedImages(with: preparation)
        try await validateOrientedImage(orientedFixture, with: preparation)

        let prepared = try await preparation.prepare(data: fixture)
        do {
            try await Task.detached {
                try validatePreparedImage(prepared)
            }.value
            try await preparation.release(prepared)
            try await Task.detached {
                try requireMissing(prepared.fileURL, context: "release left its prepared PNG behind")
                try requireMissing(
                    prepared.fileURL.deletingLastPathComponent(),
                    context: "release left its private entry directory behind"
                )
            }.value
        } catch {
            let primary = error
            do {
                try await preparation.release(prepared)
            } catch {
                throw combinedFailure(primary: primary, cleanup: error)
            }
            throw primary
        }

        let scratch = FileManager.default.temporaryDirectory
            .appendingPathComponent("zmux-clipboard-smoke-\(UUID().uuidString.lowercased())", isDirectory: true)
        let sentinel = scratch.appendingPathComponent("sentinel", isDirectory: false)
        try await Task.detached {
            try createScratchDirectory(scratch, sentinel: sentinel)
        }.value
        do {
            try await validateSymlinkCleanupRefusal(
                fixture: fixture,
                sentinel: sentinel,
                preparation: preparation
            )
            try validatePasteboard(fixture: fixture, fileURL: sentinel, pane: pane)
            try await Task.detached {
                try cleanupScratchDirectory(scratch, sentinel: sentinel)
            }.value
            return fixture
        } catch {
            let primary = error
            do {
                try await Task.detached {
                    try cleanupScratchDirectory(scratch, sentinel: sentinel)
                }.value
            } catch {
                throw combinedFailure(primary: primary, cleanup: error)
            }
            throw primary
        }
    }

    static func makeFixturePNG() throws -> Data {
        var pixels = [UInt8](repeating: 0, count: fixtureWidth * fixtureHeight * 4)
        return try pixels.withUnsafeMutableBytes { buffer in
            guard let baseAddress = buffer.baseAddress,
                  let context = CGContext(
                    data: baseAddress,
                    width: fixtureWidth,
                    height: fixtureHeight,
                    bitsPerComponent: 8,
                    bytesPerRow: fixtureWidth * 4,
                    space: CGColorSpaceCreateDeviceRGB(),
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                        | CGBitmapInfo.byteOrder32Big.rawValue
                  ) else {
                throw ClipboardImageSmokeFailure(message: "Could not create the clipboard PNG fixture context")
            }
            context.setShouldAntialias(false)
            context.setFillColor(red: 1, green: 0, blue: 0, alpha: 1)
            context.fill(CGRect(x: 0, y: 0, width: 1, height: 1))
            context.setFillColor(red: 0, green: 1, blue: 0, alpha: 1)
            context.fill(CGRect(x: 1, y: 0, width: 1, height: 1))
            context.setFillColor(red: 0, green: 0, blue: 1, alpha: 1)
            context.fill(CGRect(x: 0, y: 1, width: 1, height: 1))
            context.setFillColor(red: 1, green: 1, blue: 1, alpha: 1)
            context.fill(CGRect(x: 1, y: 1, width: 1, height: 1))

            guard let image = context.makeImage(),
                  let output = CFDataCreateMutable(kCFAllocatorDefault, 0),
                  let destination = CGImageDestinationCreateWithData(
                    output,
                    UTType.png.identifier as CFString,
                    1,
                    nil
                  ) else {
                throw ClipboardImageSmokeFailure(message: "Could not create the clipboard PNG fixture encoder")
            }
            CGImageDestinationAddImage(destination, image, nil)
            guard CGImageDestinationFinalize(destination) else {
                throw ClipboardImageSmokeFailure(message: "Could not encode the clipboard PNG fixture")
            }
            let data = output as Data
            guard !data.isEmpty else {
                throw ClipboardImageSmokeFailure(message: "The clipboard PNG fixture was empty")
            }
            return data
        }
    }

    private static func makeOrientationTIFF() throws -> Data {
        let width = 1
        let height = 2
        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        return try pixels.withUnsafeMutableBytes { buffer in
            guard let baseAddress = buffer.baseAddress,
                  let context = CGContext(
                    data: baseAddress,
                    width: width,
                    height: height,
                    bitsPerComponent: 8,
                    bytesPerRow: width * 4,
                    space: CGColorSpaceCreateDeviceRGB(),
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                        | CGBitmapInfo.byteOrder32Big.rawValue
                  ) else {
                throw ClipboardImageSmokeFailure(message: "Could not create the oriented TIFF fixture context")
            }
            context.setShouldAntialias(false)
            context.setFillColor(red: 1, green: 0, blue: 0, alpha: 1)
            context.fill(CGRect(x: 0, y: 0, width: 1, height: 1))
            context.setFillColor(red: 0, green: 0, blue: 1, alpha: 1)
            context.fill(CGRect(x: 0, y: 1, width: 1, height: 1))

            guard let image = context.makeImage(),
                  let output = CFDataCreateMutable(kCFAllocatorDefault, 0),
                  let destination = CGImageDestinationCreateWithData(
                    output,
                    UTType.tiff.identifier as CFString,
                    1,
                    nil
                  ) else {
                throw ClipboardImageSmokeFailure(message: "Could not create the oriented TIFF fixture encoder")
            }
            let properties = [kCGImagePropertyOrientation as String: 6] as CFDictionary
            CGImageDestinationAddImage(destination, image, properties)
            guard CGImageDestinationFinalize(destination) else {
                throw ClipboardImageSmokeFailure(message: "Could not encode the oriented TIFF fixture")
            }
            let data = output as Data
            guard !data.isEmpty else {
                throw ClipboardImageSmokeFailure(message: "The oriented TIFF fixture was empty")
            }
            return data
        }
    }

    private static func validateRejectedImages(with preparation: ClipboardImagePreparation) async throws {
        do {
            _ = try await preparation.prepare(data: Data())
            throw ClipboardImageSmokeFailure(message: "Image preparation accepted an empty source")
        } catch let error as ClipboardImagePreparationError {
            guard case .emptySource = error else {
                throw ClipboardImageSmokeFailure(
                    message: "Image preparation returned the wrong empty-source error: \(error.localizedDescription)"
                )
            }
        }

        do {
            _ = try await preparation.prepare(data: Data("not an image".utf8))
            throw ClipboardImageSmokeFailure(message: "Image preparation accepted corrupt bytes")
        } catch let error as ClipboardImagePreparationError {
            guard case .unsupportedImage = error else {
                throw ClipboardImageSmokeFailure(
                    message: "Image preparation returned the wrong corrupt-image error: \(error.localizedDescription)"
                )
            }
        }
    }

    private static func validatePreparedImage(_ prepared: PreparedClipboardImage) throws {
        guard prepared.pixelWidth == fixtureWidth, prepared.pixelHeight == fixtureHeight else {
            throw ClipboardImageSmokeFailure(
                message: "Prepared PNG dimensions were \(prepared.pixelWidth)x\(prepared.pixelHeight), expected 2x2"
            )
        }

        let entryDirectory = prepared.fileURL.deletingLastPathComponent()
        let cacheRoot = entryDirectory.deletingLastPathComponent()
        let applicationCache = cacheRoot.deletingLastPathComponent()
        try requireMode(applicationCache, type: S_IFDIR, permissions: 0o700)
        try requireMode(cacheRoot, type: S_IFDIR, permissions: 0o700)
        try requireMode(entryDirectory, type: S_IFDIR, permissions: 0o700)
        try requireMode(prepared.fileURL, type: S_IFREG, permissions: 0o600)

        let data = try Data(contentsOf: prepared.fileURL, options: [.mappedIfSafe])
        guard data.count == prepared.byteCount else {
            throw ClipboardImageSmokeFailure(
                message: "Prepared PNG byte count \(data.count) did not match receipt \(prepared.byteCount)"
            )
        }
        try validateDecodedPixels(
            data,
            width: fixtureWidth,
            height: fixtureHeight,
            expectedColors: [0xff0000ff, 0x00ff00ff, 0x0000ffff, 0xffffffff]
        )
    }

    private static func validateOrientedImage(
        _ fixture: Data,
        with preparation: ClipboardImagePreparation
    ) async throws {
        let prepared = try await preparation.prepare(data: fixture)
        do {
            try await Task.detached {
                guard prepared.pixelWidth == 2, prepared.pixelHeight == 1 else {
                    throw ClipboardImageSmokeFailure(
                        message: "Orientation-6 TIFF prepared as \(prepared.pixelWidth)x\(prepared.pixelHeight), expected 2x1"
                    )
                }
                let data = try Data(contentsOf: prepared.fileURL, options: [.mappedIfSafe])
                try validateDecodedPixels(
                    data,
                    width: 2,
                    height: 1,
                    expectedColors: [0xff0000ff, 0x0000ffff]
                )
            }.value
            try await preparation.release(prepared)
            try await Task.detached {
                try requireMissing(
                    prepared.fileURL.deletingLastPathComponent(),
                    context: "orientation fixture release left its private entry directory behind"
                )
            }.value
        } catch {
            let primary = error
            do {
                try await preparation.release(prepared)
            } catch {
                throw combinedFailure(primary: primary, cleanup: error)
            }
            throw primary
        }
    }

    private static func validateDecodedPixels(
        _ data: Data,
        width: Int,
        height: Int,
        expectedColors: Set<UInt32>
    ) throws {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
              CGImageSourceGetStatus(source) == .statusComplete,
              CGImageSourceGetCount(source) == 1,
              let sourceType = CGImageSourceGetType(source),
              sourceType as String == UTType.png.identifier,
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
              image.width == width,
              image.height == height else {
            throw ClipboardImageSmokeFailure(
                message: "Prepared bytes were not a complete \(width)x\(height) single-frame PNG"
            )
        }

        var decoded = [UInt8](repeating: 0, count: width * height * 4)
        let rendered = decoded.withUnsafeMutableBytes { buffer -> Bool in
            guard let baseAddress = buffer.baseAddress,
                  let context = CGContext(
                    data: baseAddress,
                    width: width,
                    height: height,
                    bitsPerComponent: 8,
                    bytesPerRow: width * 4,
                    space: CGColorSpaceCreateDeviceRGB(),
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                        | CGBitmapInfo.byteOrder32Big.rawValue
                  ) else { return false }
            context.setShouldAntialias(false)
            context.interpolationQuality = .none
            context.draw(
                image,
                in: CGRect(x: 0, y: 0, width: CGFloat(width), height: CGFloat(height))
            )
            return true
        }
        guard rendered else {
            throw ClipboardImageSmokeFailure(message: "Could not independently render the prepared PNG")
        }

        var actual = Set<UInt32>()
        for index in stride(from: 0, to: decoded.count, by: 4) {
            actual.insert(
                UInt32(decoded[index]) << 24
                    | UInt32(decoded[index + 1]) << 16
                    | UInt32(decoded[index + 2]) << 8
                    | UInt32(decoded[index + 3])
            )
        }
        guard actual == expectedColors else {
            throw ClipboardImageSmokeFailure(message: "Prepared PNG pixels did not preserve the known fixture colors")
        }
    }

    private static func validateSymlinkCleanupRefusal(
        fixture: Data,
        sentinel: URL,
        preparation: ClipboardImagePreparation
    ) async throws {
        let prepared = try await preparation.prepare(data: fixture)
        do {
            try await Task.detached {
                try exactUnlinkIfPresent(prepared.fileURL)
                try createSymbolicLink(at: prepared.fileURL, pointingTo: sentinel)
            }.value

            do {
                try await preparation.release(prepared)
                throw ClipboardImageSmokeFailure(message: "Image cleanup followed or removed a substituted symbolic link")
            } catch let error as ClipboardImagePreparationError {
                guard case .cacheIntegrity = error else {
                    throw ClipboardImageSmokeFailure(
                        message: "Symlink cleanup returned the wrong error: \(error.localizedDescription)"
                    )
                }
            }

            try await Task.detached {
                try requireType(prepared.fileURL, type: S_IFLNK, context: "cleanup did not preserve the refused symbolic link")
                guard try Data(contentsOf: sentinel) == Data("unrelated sentinel".utf8) else {
                    throw ClipboardImageSmokeFailure(message: "Image cleanup modified the unrelated symlink target")
                }
                try exactUnlinkIfPresent(prepared.fileURL)
            }.value
            try await preparation.release(prepared)
            try await Task.detached {
                try requireMissing(
                    prepared.fileURL.deletingLastPathComponent(),
                    context: "cleanup left the emptied symlink test entry behind"
                )
            }.value
        } catch {
            let primary = error
            let cleanup = await cleanupTamperedImage(prepared, preparation: preparation)
            if let cleanup {
                throw ClipboardImageSmokeFailure(
                    message: "\(describe(primary)); cleanup also failed: \(cleanup)"
                )
            }
            throw primary
        }
    }

    @MainActor
    private static func validatePasteboard(fixture: Data, fileURL: URL, pane: Pane) throws {
        let pasteboard = NSPasteboard(name: NSPasteboard.Name("app.w3dev.zmux.clipboard-smoke.\(UUID().uuidString)"))
        defer { pasteboard.clearContents() }

        let terminal = TerminalView(pane: pane)
        var files: [[String]] = []
        var images: [Data] = []
        var inputs: [String] = []
        var errors: [String] = []
        terminal.onFiles = { files.append($0) }
        terminal.onImage = { images.append($0); return true }
        terminal.onInput = { inputs.append($0) }
        terminal.onPasteError = { errors.append($0) }

        pasteboard.clearContents()
        pasteboard.declareTypes([.png, .string], owner: nil)
        guard pasteboard.setData(fixture, forType: .png),
              pasteboard.setString("text fallback", forType: .string),
              terminal.paste(from: pasteboard),
              images == [fixture], files.isEmpty, inputs.isEmpty, errors.isEmpty else {
            throw ClipboardImageSmokeFailure(message: "PNG did not take precedence over text on the terminal paste path")
        }

        images.removeAll()
        pasteboard.clearContents()
        pasteboard.declareTypes([.fileURL, .png], owner: nil)
        guard pasteboard.setString(fileURL.absoluteString, forType: .fileURL),
              pasteboard.setData(fixture, forType: .png),
              terminal.paste(from: pasteboard),
              files == [[fileURL.path]], images.isEmpty, inputs.isEmpty, errors.isEmpty else {
            throw ClipboardImageSmokeFailure(message: "File URLs did not take precedence over PNG data on the terminal paste path")
        }

        files.removeAll()
        pasteboard.clearContents()
        pasteboard.declareTypes([.string], owner: nil)
        guard pasteboard.setString("first\r\nsecond\nthird", forType: .string),
              terminal.paste(from: pasteboard),
              inputs == ["first\rsecond\rthird"], files.isEmpty, images.isEmpty, errors.isEmpty else {
            throw ClipboardImageSmokeFailure(message: "Terminal text paste did not normalize line endings")
        }

        inputs.removeAll()
        pasteboard.clearContents()
        pasteboard.declareTypes([.string], owner: nil)
        guard pasteboard.setString("must not be pasted", forType: .string),
              !terminal.paste(from: pasteboard, includeText: false),
              inputs.isEmpty, files.isEmpty, images.isEmpty, errors.isEmpty else {
            throw ClipboardImageSmokeFailure(message: "A file/image-only paste accepted plain text")
        }

        let corrupt = Data("not an image".utf8)
        pasteboard.clearContents()
        pasteboard.declareTypes([.png], owner: nil)
        guard pasteboard.setData(corrupt, forType: .png),
              terminal.paste(from: pasteboard),
              images == [corrupt], files.isEmpty, inputs.isEmpty, errors.isEmpty else {
            throw ClipboardImageSmokeFailure(message: "The pasteboard classifier decoded or rejected advertised raw image bytes")
        }
    }

    private static func createScratchDirectory(_ directory: URL, sentinel: URL) throws {
        do {
            try FileManager.default.createDirectory(
                at: directory,
                withIntermediateDirectories: false,
                attributes: [.posixPermissions: 0o700]
            )
            try Data("unrelated sentinel".utf8).write(to: sentinel, options: .withoutOverwriting)
        } catch {
            let primary = error
            do {
                try cleanupScratchDirectory(directory, sentinel: sentinel)
            } catch {
                throw combinedFailure(primary: primary, cleanup: error)
            }
            throw ClipboardImageSmokeFailure(message: "Could not create clipboard smoke scratch data: \(describe(primary))")
        }
    }

    private static func cleanupScratchDirectory(_ directory: URL, sentinel: URL) throws {
        var failures: [String] = []
        do { try exactUnlinkIfPresent(sentinel) }
        catch { failures.append(describe(error)) }
        do { try exactRmdirIfPresent(directory) }
        catch { failures.append(describe(error)) }
        guard failures.isEmpty else {
            throw ClipboardImageSmokeFailure(message: failures.joined(separator: "; "))
        }
    }

    private static func cleanupTamperedImage(
        _ prepared: PreparedClipboardImage,
        preparation: ClipboardImagePreparation
    ) async -> String? {
        var failures: [String] = []
        do {
            try await Task.detached {
                try exactUnlinkIfPresent(prepared.fileURL)
            }.value
        } catch {
            failures.append(describe(error))
        }
        do {
            try await preparation.release(prepared)
        } catch {
            failures.append(describe(error))
        }
        return failures.isEmpty ? nil : failures.joined(separator: "; ")
    }

    private static func createSymbolicLink(at link: URL, pointingTo target: URL) throws {
        let result = link.withUnsafeFileSystemRepresentation { linkPath in
            target.withUnsafeFileSystemRepresentation { targetPath in
                guard let linkPath, let targetPath else { errno = EINVAL; return Int32(-1) }
                return Darwin.symlink(targetPath, linkPath)
            }
        }
        guard result == 0 else {
            throw posixFailure("create the clipboard smoke symbolic link")
        }
    }

    private static func requireMode(_ url: URL, type: mode_t, permissions: mode_t) throws {
        guard let information = try fileInformationIfPresent(at: url) else {
            throw ClipboardImageSmokeFailure(message: "Clipboard cache item is missing: \(url.path)")
        }
        guard (information.st_mode & S_IFMT) == type else {
            throw ClipboardImageSmokeFailure(message: "Clipboard cache item has the wrong filesystem type: \(url.path)")
        }
        guard (information.st_mode & 0o7777) == permissions else {
            throw ClipboardImageSmokeFailure(
                message: "Clipboard cache item \(url.path) has mode \(String(information.st_mode & 0o7777, radix: 8)); expected \(String(permissions, radix: 8))"
            )
        }
    }

    private static func requireType(_ url: URL, type: mode_t, context: String) throws {
        guard let information = try fileInformationIfPresent(at: url),
              (information.st_mode & S_IFMT) == type else {
            throw ClipboardImageSmokeFailure(message: context)
        }
    }

    private static func requireMissing(_ url: URL, context: String) throws {
        if try fileInformationIfPresent(at: url) != nil {
            throw ClipboardImageSmokeFailure(message: context)
        }
    }

    private static func fileInformationIfPresent(at url: URL) throws -> stat? {
        var information = stat()
        let result = url.withUnsafeFileSystemRepresentation { path in
            guard let path else { errno = EINVAL; return Int32(-1) }
            return Darwin.lstat(path, &information)
        }
        if result != 0, errno == ENOENT { return nil }
        guard result == 0 else { throw posixFailure("inspect \(url.path)") }
        return information
    }

    private static func exactUnlinkIfPresent(_ url: URL) throws {
        let result = url.withUnsafeFileSystemRepresentation { path in
            guard let path else { errno = EINVAL; return Int32(-1) }
            return Darwin.unlink(path)
        }
        guard result == 0 || errno == ENOENT else { throw posixFailure("remove \(url.path)") }
    }

    private static func exactRmdirIfPresent(_ url: URL) throws {
        let result = url.withUnsafeFileSystemRepresentation { path in
            guard let path else { errno = EINVAL; return Int32(-1) }
            return Darwin.rmdir(path)
        }
        guard result == 0 || errno == ENOENT else { throw posixFailure("remove \(url.path)") }
    }

    private static func posixFailure(_ operation: String) -> ClipboardImageSmokeFailure {
        let code = errno
        return ClipboardImageSmokeFailure(message: "Could not \(operation): \(String(cString: strerror(code)))")
    }

    private static func combinedFailure(primary: any Error, cleanup: any Error) -> ClipboardImageSmokeFailure {
        ClipboardImageSmokeFailure(
            message: "\(describe(primary)); cleanup also failed: \(describe(cleanup))"
        )
    }

    private static func describe(_ error: any Error) -> String {
        if let description = (error as? any LocalizedError)?.errorDescription, !description.isEmpty {
            return description
        }
        return error.localizedDescription
    }
}

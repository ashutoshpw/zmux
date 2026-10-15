import AppKit
import UniformTypeIdentifiers

/// Read representations in terminal order without decoding images on the UI thread.
enum TerminalPasteboardContent {
    case files([String])
    case image(Data)
    case text(String)
    case empty

    // Image data types used by common screenshot, browser and image-editor drags.
    static let dragTypes: [NSPasteboard.PasteboardType] = [
        .fileURL, .png, .tiff,
        .init(UTType.jpeg.identifier), .init(UTType.heic.identifier),
        .init(UTType.gif.identifier), .init(UTType.webP.identifier)
    ]

    @MainActor
    static func hasFilesOrImage(_ pasteboard: NSPasteboard) -> Bool {
        pasteboard.canReadObject(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true])
            || imageType(in: pasteboard) != nil
    }

    @MainActor
    private static func imageType(in pasteboard: NSPasteboard) -> NSPasteboard.PasteboardType? {
        // Prefer PNG/TIFF when a source offers several representations. Inspecting the
        // advertised types avoids creating an NSImage or triggering image decompression.
        for type in [.png, .tiff] as [NSPasteboard.PasteboardType] where pasteboard.types?.contains(type) == true {
            return type
        }
        return pasteboard.types?.first { UTType($0.rawValue)?.conforms(to: .image) == true }
    }

    @MainActor
    static func read(_ pasteboard: NSPasteboard, includeText: Bool) throws -> TerminalPasteboardContent {
        let paths = (pasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? [])
            .filter { $0.isFileURL }.map { $0.path }
        if !paths.isEmpty { return .files(paths) }
        if let type = imageType(in: pasteboard) {
            guard let data = pasteboard.data(forType: type), !data.isEmpty else {
                throw ClipboardImagePreparationError.emptySource
            }
            guard data.count <= ClipboardImagePreparation.maximumSourceByteCount else {
                throw ClipboardImagePreparationError.sourceTooLarge(actual: data.count, limit: ClipboardImagePreparation.maximumSourceByteCount)
            }
            return .image(data)
        }
        if includeText, let text = pasteboard.string(forType: .string), !text.isEmpty { return .text(text) }
        return .empty
    }
}

import AppKit
import Foundation
import ImageIO

/// How the Rust file service classified a file tab. Mode is fixed when the tab is created.
enum FilePanelMode: String, Sendable {
    case text, markdown, image, pdf
    var isEditableText: Bool { self == .text || self == .markdown }
}

/// A presentation failure with a stable code, shown verbatim in the panel (never a placeholder).
struct FilePanelFailure: Error, Sendable {
    let code: String
    let message: String
}

/// A fully decoded image. `CGImage` is immutable, so handing it across tasks is safe.
final class DecodedImage: @unchecked Sendable {
    let image: CGImage
    init(_ image: CGImage) { self.image = image }
    var width: Int { image.width }
    var height: Int { image.height }
}

/// ImageIO decoding that never runs on the main actor and refuses decompression bombs.
enum FileImageDecoder {
    /// 100 megapixels, the same bound the design states for the native image preview.
    static let maximumPixels = 100_000_000

    static func decode(_ data: Data) -> Result<DecodedImage, FilePanelFailure> {
        guard !data.isEmpty else {
            return .failure(FilePanelFailure(code: "image_undecodable", message: "The image file is empty."))
        }
        let sourceOptions = [kCGImageSourceShouldCache as String: false] as CFDictionary
        guard let source = CGImageSourceCreateWithData(data as CFData, sourceOptions),
              CGImageSourceGetCount(source) >= 1,
              CGImageSourceGetStatusAtIndex(source, 0) == .statusComplete,
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, sourceOptions),
              let width = dimension(kCGImagePropertyPixelWidth, in: properties),
              let height = dimension(kCGImagePropertyPixelHeight, in: properties) else {
            return .failure(FilePanelFailure(code: "image_undecodable",
                message: "This image could not be decoded. The file may be corrupt or use an unsupported format."))
        }
        let (pixels, overflow) = width.multipliedReportingOverflow(by: height)
        guard !overflow, pixels <= maximumPixels else {
            return .failure(FilePanelFailure(code: "image_too_large",
                message: "This image has \(width) × \(height) pixels, above the 100 megapixel preview limit."))
        }
        // Decode the full image once, here, with the EXIF orientation applied.
        let decodeOptions: [String: Any] = [
            kCGImageSourceShouldCache as String: true,
            kCGImageSourceShouldCacheImmediately as String: true,
            kCGImageSourceCreateThumbnailFromImageAlways as String: true,
            kCGImageSourceCreateThumbnailWithTransform as String: true,
            kCGImageSourceThumbnailMaxPixelSize as String: max(width, height)
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, decodeOptions as CFDictionary) else {
            return .failure(FilePanelFailure(code: "image_undecodable",
                message: "This image could not be decoded. The file may be corrupt or truncated."))
        }
        return .success(DecodedImage(image))
    }

    private static func dimension(_ key: CFString, in properties: CFDictionary) -> Int? {
        guard let number = (properties as NSDictionary)[key] as? NSNumber else { return nil }
        let value = number.doubleValue
        guard value.isFinite, value > 0, value < Double(Int.max), value.rounded(.towardZero) == value else { return nil }
        return Int(value)
    }
}

/// Styled output of the Markdown renderer. The attributed string is built once and never mutated
/// afterwards, so moving it from the render task to the main actor is safe.
struct RenderedMarkdown: @unchecked Sendable {
    let text: NSAttributedString
    /// Set when the parser refused the document; `text` is then the plain source.
    let failure: String?
}

/// Renders Markdown with the system parser (`AttributedString(markdown:)`, no dependency) and
/// maps block and inline intents to AppKit fonts and paragraph styles. Links open only when their
/// scheme is `http`, `https` or `mailto`; every other link, image, table border and raw HTML
/// degrades to inert text (a documented gap, see the CU-14 receipt).
enum FileMarkdownRenderer {
    static let allowedLinkSchemes: Set<String> = ["http", "https", "mailto"]
    private static let baseSize: CGFloat = 14

    static func isAllowedLink(_ url: URL) -> Bool {
        guard let scheme = url.scheme?.lowercased() else { return false }
        return allowedLinkSchemes.contains(scheme)
    }

    private struct BlockStyle {
        var headerLevel: Int?
        var isCode = false
        var quoteDepth = 0
        var listDepth = 0
        var listItemIdentity: Int?
        var listOrdinal = 0
        var ordered: Bool?
        var inTableCell = false
        var rowIdentity: Int?

        init(_ intent: PresentationIntent?) {
            guard let intent else { return }
            // Components run from the innermost block to the outermost one.
            for component in intent.components {
                switch component.kind {
                case .header(let level):
                    if headerLevel == nil { headerLevel = level }
                case .codeBlock:
                    isCode = true
                case .blockQuote:
                    quoteDepth += 1
                case .listItem(let ordinal):
                    if listItemIdentity == nil { listItemIdentity = component.identity; listOrdinal = ordinal }
                    listDepth += 1
                case .orderedList:
                    if ordered == nil { ordered = true }
                case .unorderedList:
                    if ordered == nil { ordered = false }
                case .tableCell:
                    inTableCell = true
                case .tableRow, .tableHeaderRow:
                    if rowIdentity == nil { rowIdentity = component.identity }
                default:
                    break
                }
            }
        }
    }

    static func plain(_ source: String) -> NSAttributedString {
        NSAttributedString(string: source, attributes: [
            .font: NSFont.monospacedSystemFont(ofSize: 13, weight: .regular),
            .foregroundColor: NSColor.labelColor
        ])
    }

    static func render(_ source: String) -> RenderedMarkdown {
        var options = AttributedString.MarkdownParsingOptions()
        options.interpretedSyntax = .full
        options.failurePolicy = .returnPartiallyParsedIfPossible
        let parsed: AttributedString
        do { parsed = try AttributedString(markdown: source, options: options) }
        catch {
            return RenderedMarkdown(text: plain(source),
                failure: "Markdown could not be rendered (" + error.localizedDescription + "). Showing the source text.")
        }
        let output = NSMutableAttributedString()
        var started = false
        var previousBlock: Int?
        var previousRow: Int?
        var previousListItem: Int?
        for run in parsed.runs {
            let intent = run.presentationIntent
            let block = intent?.components.first?.identity
            let style = BlockStyle(intent)
            if !started || block != previousBlock {
                if started {
                    if style.inTableCell {
                        output.append(NSAttributedString(string: style.rowIdentity == previousRow ? "\t" : "\n"))
                    } else if style.listItemIdentity != nil && previousListItem != nil {
                        output.append(NSAttributedString(string: "\n"))
                    } else {
                        output.append(NSAttributedString(string: "\n"))
                        output.append(NSAttributedString(string: "\n", attributes: [.font: NSFont.systemFont(ofSize: 6)]))
                    }
                }
                if let item = style.listItemIdentity, item != previousListItem {
                    let marker = style.ordered == true ? "\(style.listOrdinal). " : "\u{2022} "
                    output.append(NSAttributedString(string: marker, attributes: attributes(for: style, inline: [])))
                }
                started = true
                previousBlock = block
                previousRow = style.rowIdentity
                previousListItem = style.listItemIdentity
            }
            let inline = run.inlinePresentationIntent ?? []
            var piece = String(parsed[run.range].characters)
            if inline.contains(.softBreak) { piece = " " }
            else if inline.contains(.lineBreak) { piece = "\n" }
            if style.isCode, piece.hasSuffix("\n") { piece.removeLast() }
            var attrs = attributes(for: style, inline: inline)
            if let url = run.link, isAllowedLink(url) {
                attrs[.link] = url
                attrs[.foregroundColor] = NSColor.linkColor
                attrs[.underlineStyle] = NSUnderlineStyle.single.rawValue
            }
            output.append(NSAttributedString(string: piece, attributes: attrs))
        }
        return RenderedMarkdown(text: output, failure: nil)
    }

    private static func attributes(for style: BlockStyle, inline: InlinePresentationIntent) -> [NSAttributedString.Key: Any] {
        var size = baseSize
        var bold = false
        var mono = false
        if let level = style.headerLevel {
            let sizes: [CGFloat] = [26, 21, 18, 16, 14, 13]
            size = sizes[min(max(level, 1), sizes.count) - 1]
            bold = true
        }
        if style.isCode { size = 12.5; mono = true }
        if inline.contains(.stronglyEmphasized) { bold = true }
        if inline.contains(.code) { mono = true; size = min(size, 13) }
        var font = mono
            ? NSFont.monospacedSystemFont(ofSize: size, weight: bold ? .bold : .regular)
            : NSFont.systemFont(ofSize: size, weight: bold ? .bold : .regular)
        if inline.contains(.emphasized) {
            let traits = font.fontDescriptor.symbolicTraits.union(.italic)
            if let italic = NSFont(descriptor: font.fontDescriptor.withSymbolicTraits(traits), size: size) { font = italic }
        }
        let paragraph = NSMutableParagraphStyle()
        let depth = style.listDepth
        if depth > 0 {
            paragraph.firstLineHeadIndent = CGFloat(depth - 1) * 18 + CGFloat(style.quoteDepth) * 14
            paragraph.headIndent = CGFloat(depth) * 18 + CGFloat(style.quoteDepth) * 14
        } else if style.quoteDepth > 0 {
            paragraph.firstLineHeadIndent = CGFloat(style.quoteDepth) * 14
            paragraph.headIndent = CGFloat(style.quoteDepth) * 14
        }
        var attrs: [NSAttributedString.Key: Any] = [
            .font: font,
            .foregroundColor: style.quoteDepth > 0 ? NSColor.secondaryLabelColor : NSColor.labelColor,
            .paragraphStyle: paragraph
        ]
        if style.isCode || inline.contains(.code) { attrs[.backgroundColor] = NSColor.quaternaryLabelColor }
        if inline.contains(.strikethrough) { attrs[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
        return attrs
    }
}

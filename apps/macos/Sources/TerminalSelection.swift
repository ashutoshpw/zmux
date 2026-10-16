import AppKit

struct TerminalSelectionPoint: Comparable {
    let row: Int
    let column: Int

    static func < (lhs: Self, rhs: Self) -> Bool {
        (lhs.row, lhs.column) < (rhs.row, rhs.column)
    }
}

struct TerminalSelectionRange {
    let lower: TerminalSelectionPoint
    let upper: TerminalSelectionPoint
    var isEmpty: Bool { lower == upper }
}

/// Visible cell selection shared by mouse tracking, drawing and clipboard copy.
/// AppKit supplies word boundaries; terminal runs supply authoritative cell spans.
@MainActor
final class TerminalSelection {
    private enum Granularity { case character, word, row }
    private struct Cell {
        let lower: Int
        let upper: Int
        let text: String
        let utf16: NSRange
    }
    private struct Row {
        let text: String
        let cells: [Cell]
    }
    private lazy var wordSelector = NSTextView(frame: .zero)
    private var granularity = Granularity.character
    private var anchor: TerminalSelectionRange?
    private(set) var range: TerminalSelectionRange?

    func clear() { anchor = nil; range = nil }

    func begin(row: Int, column: Int, clicks: Int, viewport: Viewport) {
        granularity = clicks >= 3 ? .row : clicks == 2 ? .word : .character
        let point = clamped(row: row, column: column, viewport: viewport)
        let selected = expanded(point, viewport: viewport)
        anchor = selected
        range = selected
    }

    func extend(row: Int, column: Int, viewport: Viewport) {
        guard let anchor else { return }
        let target = expanded(clamped(row: row, column: column, viewport: viewport), viewport: viewport)
        let selected: TerminalSelectionRange
        if target.upper <= anchor.lower {
            selected = TerminalSelectionRange(lower: target.lower, upper: anchor.upper)
        } else if target.lower >= anchor.upper {
            selected = TerminalSelectionRange(lower: anchor.lower, upper: target.upper)
        } else {
            selected = anchor
        }
        range = snapToCells(selected, viewport: viewport)
    }

    func selectAll(viewport: Viewport) {
        guard viewport.size.rows > 0 else { clear(); return }
        let all = TerminalSelectionRange(lower: TerminalSelectionPoint(row: 0, column: 0),
            upper: TerminalSelectionPoint(row: viewport.size.rows - 1, column: viewport.size.cols))
        anchor = all; range = all; granularity = .character
    }

    func text(viewport: Viewport) -> String? {
        guard let range, !range.isEmpty else { return nil }
        var pieces: [String] = []
        for row in range.lower.row...range.upper.row {
            let lower = row == range.lower.row ? range.lower.column : 0
            let upper = row == range.upper.row ? range.upper.column : viewport.size.cols
            // Metadata is validated against this viewport during decoding. Empty
            // physical rows retain hard separators even if marked as wrapped.
            var joinsNext = false
            var copyUpper = upper
            if row < range.upper.row, let metadata = viewport.rowCopy,
               metadata[row].wrapsToNext, metadata[row].writtenEnd > 0, metadata[row + 1].writtenEnd > 0 {
                joinsNext = true
                copyUpper = min(upper, metadata[row].writtenEnd)
            }
            var text = cells(row, viewport: viewport).cells
                .filter { $0.lower < copyUpper && $0.upper > lower }.map(\.text).joined()
            // Preserve written spaces at a soft boundary. Whole hard rows and
            // physical breaks omit padding; explicit partial same-row edge
            // spaces remain copyable, as before.
            if !joinsNext && upper == viewport.size.cols && (lower == 0 || range.lower.row != range.upper.row) {
                while text.last == " " { text.removeLast() }
            }
            pieces.append(text)
            if row < range.upper.row { pieces.append(joinsNext ? "" : "\n") }
        }
        return pieces.joined()
    }

    private func expanded(_ point: TerminalSelectionPoint, viewport: Viewport) -> TerminalSelectionRange {
        switch granularity {
        case .character:
            return TerminalSelectionRange(lower: point, upper: point)
        case .row:
            return TerminalSelectionRange(lower: TerminalSelectionPoint(row: point.row, column: 0),
                upper: TerminalSelectionPoint(row: point.row, column: viewport.size.cols))
        case .word:
            let row = cells(point.row, viewport: viewport)
            guard let clicked = row.cells.first(where: { $0.upper > min(point.column, viewport.size.cols - 1) }) else {
                return TerminalSelectionRange(lower: point, upper: point)
            }
            wordSelector.string = row.text
            let selected = wordSelector.selectionRange(forProposedRange: NSRange(location: clicked.utf16.location, length: 0), granularity: .selectByWord)
            guard selected.location != NSNotFound, selected.length > 0,
                  let lower = row.cells.first(where: { NSMaxRange($0.utf16) > selected.location }),
                  let upper = row.cells.last(where: { $0.utf16.location < NSMaxRange(selected) }) else {
                return TerminalSelectionRange(lower: point, upper: point)
            }
            return TerminalSelectionRange(lower: TerminalSelectionPoint(row: point.row, column: lower.lower),
                upper: TerminalSelectionPoint(row: point.row, column: upper.upper))
        }
    }

    private func snapToCells(_ selected: TerminalSelectionRange, viewport: Viewport) -> TerminalSelectionRange {
        guard !selected.isEmpty else { return selected }
        let lowerCell = cells(selected.lower.row, viewport: viewport).cells.first {
            $0.lower < selected.lower.column && $0.upper > selected.lower.column
        }
        let upperCell = cells(selected.upper.row, viewport: viewport).cells.first {
            $0.lower < selected.upper.column && $0.upper > selected.upper.column
        }
        return TerminalSelectionRange(
            lower: TerminalSelectionPoint(row: selected.lower.row, column: lowerCell?.lower ?? selected.lower.column),
            upper: TerminalSelectionPoint(row: selected.upper.row, column: upperCell?.upper ?? selected.upper.column))
    }

    private func clamped(row: Int, column: Int, viewport: Viewport) -> TerminalSelectionPoint {
        TerminalSelectionPoint(row: max(0, min(viewport.size.rows - 1, row)),
                               column: max(0, min(viewport.size.cols, column)))
    }

    private func cells(_ row: Int, viewport: Viewport) -> Row {
        guard viewport.lines.indices.contains(row) else { return Row(text: "", cells: []) }
        var output = ""
        var result: [Cell] = []
        var column = 0
        var utf16 = 0
        func append(_ text: String, width: Int) {
            let length = (text as NSString).length
            result.append(Cell(lower: column, upper: column + width, text: text,
                               utf16: NSRange(location: utf16, length: length)))
            output += text; column += width; utf16 += length
        }
        for run in viewport.lines[row] {
            guard run.column >= column, run.width > 0, run.column < viewport.size.cols else { continue }
            while column < run.column { append(" ", width: 1) }
            let width = min(run.width, viewport.size.cols - column)
            if run.text.utf8.allSatisfy({ $0 < 128 }), run.text.utf8.count == width {
                for byte in run.text.utf8 { append(String(UnicodeScalar(byte)), width: 1) }
            } else {
                append(run.text.isEmpty ? " " : run.text, width: width)
            }
        }
        while column < viewport.size.cols { append(" ", width: 1) }
        return Row(text: output, cells: result)
    }
}

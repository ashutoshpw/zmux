import Foundation

private struct ContractFailure: Error { let message: String }

/// Independent malformed-wire checks against the actual production decoder.
/// No TerminalView, selection algorithm or platform behavior is substituted.
@main
enum CopyMetadataContract {
    static func main() throws {
        func row(_ end: Int) -> [String: Any] {
            ["wraps_to_next": true, "written_end": end]
        }
        func decode(_ metadata: Any?, rows: Int = 2, cols: Int = 4) throws -> Viewport {
            var value: [String: Any] = [
                "size": ["rows": rows, "cols": cols], "contents": "", "cursor_row": 0, "cursor_col": 0,
                "lines": Array(repeating: [Any](), count: rows), "application_cursor": false, "bracketed_paste": false,
                "hide_cursor": false, "scrollback_offset": 0, "scrollback_lines": 0, "alternate_screen": false
            ]
            if let metadata { value["row_copy"] = metadata }
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            return try decoder.decode(Viewport.self, from: JSONSerialization.data(withJSONObject: value))
        }
        let valid = try decode([row(4), row(1)])
        guard let records = valid.rowCopy, records.count == 2, records[0].wrapsToNext,
              records[0].writtenEnd == 4, records[1].writtenEnd == 1,
              try decode(nil).rowCopy == nil, try decode(NSNull()).rowCopy == nil else {
            throw ContractFailure(message: "Valid or explicitly unavailable copy metadata failed decoding")
        }
        let invalid: [(String, Any, Int, Int)] = [
            ("row_count", [row(4)], 2, 4),
            ("negative_extent", [row(-1), row(1)], 2, 4),
            ("past_right_edge", [row(5), row(1)], 2, 4),
            ("row_limit", Array(repeating: row(1), count: 121), 121, 4),
            ("column_limit", [row(4), row(1)], 2, 241)
        ]
        var rejected: [String] = []
        for (name, metadata, rows, cols) in invalid {
            do {
                _ = try decode(metadata, rows: rows, cols: cols)
                throw ContractFailure(message: "Malformed copy metadata was accepted: " + name)
            } catch is DecodingError {
                rejected.append(name)
            }
        }
        let result: [String: Any] = ["valid_metadata": true, "unavailable_metadata": true, "rejected": rejected]
        print(String(decoding: try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys]), as: UTF8.self))
    }
}

// SPDX-License-Identifier: MIT
// Isolated compatibility exporter: existing public Viewport types, no application linkage.
mod owner;
use owner::Engine;
use std::{ffi::c_void, mem::size_of};
use zmux_terminal_adapter::{Color, Run, Size, Style, Viewport};

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct RawColor {
    kind: u8,
    index: u8,
    r: u8,
    g: u8,
    b: u8,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct RawCell {
    text_offset: u32,
    text_length: u32,
    row: u16,
    column: u16,
    width: u8,
    foreground: RawColor,
    background: RawColor,
    bold: u8,
    italic: u8,
    underline: u8,
    inverse: u8,
}
#[derive(Default)]
#[repr(C)]
struct RawFrame {
    version: u32,
    unsupported: u32,
    history_rows: u64,
    scroll_total: u64,
    scroll_offset: u64,
    scroll_len: u64,
    cells_length: usize,
    text_length: usize,
    rows: u16,
    cols: u16,
    cursor_x: u16,
    cursor_y: u16,
    pending_wrap: u8,
    cursor_visible: u8,
    application_cursor: u8,
    bracketed_paste: u8,
    alternate: u8,
}
unsafe extern "C" {
    fn probe_frame(
        p: *mut c_void,
        info: *mut RawFrame,
        info_size: usize,
        cells: *mut RawCell,
        cell_size: usize,
        cells_cap: usize,
        text: *mut u8,
        text_cap: usize,
    ) -> i32;
    fn probe_scroll(p: *mut c_void, lines: i32) -> i32;
}

#[derive(Debug, PartialEq, Eq)]
enum ExportError {
    Boundary(i32),
    Unsupported(u32),
    Invalid(&'static str),
}
fn color(value: RawColor) -> Result<Color, ExportError> {
    match value.kind {
        0 => Ok(Color::Default),
        1 => Ok(Color::Indexed(value.index)),
        2 => Ok(Color::Rgb([value.r, value.g, value.b])),
        _ => Err(ExportError::Invalid("color tag")),
    }
}
fn boolean(value: u8) -> Result<bool, ExportError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ExportError::Invalid("boolean")),
    }
}
fn snapshot(engine: &mut Engine) -> Result<Viewport, ExportError> {
    let mut cells = vec![RawCell::default(); 120 * 240];
    let mut text = vec![0u8; 512 * 1024];
    let mut info = RawFrame::default();
    let result = unsafe {
        probe_frame(
            engine.raw(),
            &mut info,
            size_of::<RawFrame>(),
            cells.as_mut_ptr(),
            size_of::<RawCell>(),
            cells.len(),
            text.as_mut_ptr(),
            text.len(),
        )
    };
    if result != 0 {
        return Err(ExportError::Boundary(result));
    }
    if info.unsupported != 0 {
        return Err(ExportError::Unsupported(info.unsupported));
    }
    if info.version != 1 || info.cells_length > cells.len() || info.text_length > text.len() {
        return Err(ExportError::Invalid("frame lengths/version"));
    }
    let size = Size {
        rows: info.rows,
        cols: info.cols,
    }
    .validate()
    .map_err(|_| ExportError::Invalid("size"))?;
    if info.cursor_y >= size.rows || info.cursor_x >= size.cols {
        return Err(ExportError::Invalid("cursor"));
    }
    let history = info
        .scroll_total
        .checked_sub(info.scroll_len)
        .ok_or(ExportError::Invalid("scrollbar length"))?;
    let offset = history
        .checked_sub(info.scroll_offset)
        .ok_or(ExportError::Invalid("scrollbar offset"))?;
    if info.scroll_len != u64::from(size.rows) || history != info.history_rows {
        return Err(ExportError::Invalid("scrollbar geometry"));
    }
    let mut lines: Vec<Vec<Run>> = (0..size.rows).map(|_| Vec::new()).collect();
    let mut next_column = vec![0u16; usize::from(size.rows)];
    let mut next_text = 0usize;
    for cell in cells.iter().take(info.cells_length) {
        let row = usize::from(cell.row);
        let start = usize::try_from(cell.text_offset).unwrap();
        let end = start
            .checked_add(usize::try_from(cell.text_length).unwrap())
            .ok_or(ExportError::Invalid("cell text range"))?;
        if row >= lines.len()
            || cell.column != next_column[row]
            || !matches!(cell.width, 1 | 2)
            || u32::from(cell.column) + u32::from(cell.width) > u32::from(size.cols)
            || start != next_text
            || end > info.text_length
            || start == end
        {
            return Err(ExportError::Invalid("cell bounds/order"));
        }
        next_column[row] += u16::from(cell.width);
        next_text = end;
        let value = std::str::from_utf8(&text[start..end])
            .map_err(|_| ExportError::Invalid("cell UTF-8"))?;
        let style = Style {
            foreground: color(cell.foreground)?,
            background: color(cell.background)?,
            bold: boolean(cell.bold)?,
            italic: boolean(cell.italic)?,
            underline: boolean(cell.underline)?,
            inverse: boolean(cell.inverse)?,
        };
        if value.is_ascii()
            && let Some(last) = lines[row].last_mut()
            && last.text.is_ascii()
            && last.style == style
            && last.column + last.width == cell.column
        {
            last.text.push_str(value);
            last.width += u16::from(cell.width);
        } else {
            lines[row].push(Run {
                column: cell.column,
                width: u16::from(cell.width),
                text: value.into(),
                style,
            });
        }
    }
    if next_text != info.text_length || next_column.iter().any(|column| *column != size.cols) {
        return Err(ExportError::Invalid("incomplete frame"));
    }
    let mut rows: Vec<String> = lines
        .iter()
        .map(|runs| {
            let joined: String = runs.iter().map(|run| run.text.as_str()).collect();
            joined.trim_end_matches(' ').to_owned()
        })
        .collect();
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    Ok(Viewport {
        notices: Vec::new(),
        title: String::new(),
        working_directory: None,
        size,
        contents: rows.join("\n"),
        // Preserve the existing Viewport's logical pending-wrap column == cols.
        cursor_col: if boolean(info.pending_wrap)? {
            size.cols
        } else {
            info.cursor_x
        },
        cursor_row: info.cursor_y,
        lines,
        application_cursor: boolean(info.application_cursor)?,
        bracketed_paste: boolean(info.bracketed_paste)?,
        hide_cursor: !boolean(info.cursor_visible)?,
        scrollback_offset: usize::try_from(offset).map_err(|_| ExportError::Invalid("offset"))?,
        scrollback_lines: usize::try_from(history).map_err(|_| ExportError::Invalid("history"))?,
        alternate_screen: boolean(info.alternate)?,
    })
}
fn scroll(engine: &mut Engine, lines: i32) {
    assert_eq!(unsafe { probe_scroll(engine.raw(), lines) }, 0);
}
fn runs(view: &Viewport, row: usize) -> Vec<(u16, u16, &str)> {
    view.lines[row]
        .iter()
        .map(|r| (r.column, r.width, r.text.as_str()))
        .collect()
}
fn main() {
    let mut e = Engine::new(8, 2);
    for byte in "界e\u{301}x".as_bytes() {
        e.feed(&[*byte]);
    }
    let retained = snapshot(&mut e).unwrap();
    assert_eq!(retained.contents, "界e\u{301}x");
    assert_eq!(
        runs(&retained, 0),
        [(0, 2, "界"), (2, 1, "e\u{301}"), (3, 5, "x    ")]
    );
    assert_eq!((retained.cursor_row, retained.cursor_col), (0, 4));
    e.feed(b"\x1b[2J");
    e.resize(4, 2);
    drop(e);
    assert_eq!(retained.contents, "界e\u{301}x");
    println!("compatibility: wide/combining runs and retained owned Viewport pass");
    println!(
        "compatibility_sample: contents={:?} runs={:?}",
        retained.contents,
        runs(&retained, 0)
    );

    let mut e = Engine::new(4, 3);
    e.feed("abc界z".as_bytes());
    let v = snapshot(&mut e).unwrap();
    assert_eq!(v.contents, "abc\n界z");
    assert_eq!(runs(&v, 0), [(0, 4, "abc ")]);
    assert_eq!(runs(&v, 1), [(0, 2, "界"), (2, 2, "z ")]);
    for (input, expected) in [
        ("\u{1b}[?2027h👩\u{200d}💻x", "👩\u{200d}💻x"),
        ("\u{1b}[?2027h❤\u{fe0f}x", "❤\u{fe0f}x"),
    ] {
        let mut e = Engine::new(8, 2);
        for byte in input.as_bytes() {
            e.feed(&[*byte]);
        }
        let v = snapshot(&mut e).unwrap();
        assert_eq!(v.contents, expected);
        assert_eq!(v.cursor_col, 3);
        assert_eq!((v.lines[0][0].column, v.lines[0][0].width), (0, 2));
    }
    println!("compatibility: soft-wrap spacer and mode-2027 grapheme runs pass");

    let mut e = Engine::new(8, 2);
    e.feed(b"\x1b[31;44;1;3;4;7mA\x1b[0mB\x1b[38;2;1;2;3;48;5;200mC\x1b[0m");
    let v = snapshot(&mut e).unwrap();
    assert_eq!(v.contents, "ABC");
    assert_eq!(
        v.lines[0][0].style,
        Style {
            foreground: Color::Indexed(1),
            background: Color::Indexed(4),
            bold: true,
            italic: true,
            underline: true,
            inverse: true
        }
    );
    assert_eq!(v.lines[0][1].style.foreground, Color::Default);
    assert!(!v.lines[0][1].style.bold && !v.lines[0][1].style.inverse);
    assert_eq!(v.lines[0][2].style.foreground, Color::Rgb([1, 2, 3]));
    assert_eq!(v.lines[0][2].style.background, Color::Indexed(200));
    let mut e = Engine::new(4, 2);
    e.feed(b"\x1b[48;5;196m\x1b[2J");
    let v = snapshot(&mut e).unwrap();
    assert_eq!(v.contents, "");
    assert!(
        v.lines
            .iter()
            .flatten()
            .all(|r| r.style.background == Color::Indexed(196))
    );
    println!("compatibility: indexed/RGB/default styles, inverse and erased backgrounds pass");

    let mut e = Engine::new(4, 2);
    e.feed(b"ABCD");
    assert_eq!(snapshot(&mut e).unwrap().cursor_col, 4);
    e.feed(b"E\x1b[?1h\x1b[?2004h\x1b[?25l");
    let v = snapshot(&mut e).unwrap();
    assert_eq!((v.cursor_row, v.cursor_col), (1, 1));
    assert!(v.application_cursor && v.bracketed_paste && v.hide_cursor);
    e.feed(b"\x1b[?1l\x1b[?2004l\x1b[?25h");
    let v = snapshot(&mut e).unwrap();
    assert!(!v.application_cursor && !v.bracketed_paste && !v.hide_cursor);
    println!("compatibility: pending wrap cursor and mode transitions pass");

    let mut e = Engine::new(6, 3);
    e.feed(b"0\r\n1\r\n2\r\n3\r\n4");
    let v = snapshot(&mut e).unwrap();
    assert_eq!(v.contents, "2\n3\n4");
    assert_eq!((v.scrollback_lines, v.scrollback_offset), (2, 0));
    scroll(&mut e, 1);
    let v = snapshot(&mut e).unwrap();
    assert_eq!(v.contents, "1\n2\n3");
    assert_eq!(v.scrollback_offset, 1);
    scroll(&mut e, 1000);
    let v = snapshot(&mut e).unwrap();
    assert_eq!(v.contents, "0\n1\n2");
    assert_eq!(v.scrollback_offset, 2);
    scroll(&mut e, 0);
    assert_eq!(snapshot(&mut e).unwrap().contents, "2\n3\n4");
    e.feed(b"\x1b[?1049h\x1b[HALT");
    scroll(&mut e, 1000);
    let v = snapshot(&mut e).unwrap();
    assert_eq!(v.contents, "ALT");
    assert!(v.alternate_screen);
    assert_eq!((v.scrollback_lines, v.scrollback_offset), (0, 0));
    e.feed(b"\x1b[?1049l");
    let v = snapshot(&mut e).unwrap();
    assert!(!v.alternate_screen);
    assert_eq!(v.contents, "2\n3\n4");
    println!("compatibility: history direction/clamping and alternate-screen isolation pass");

    let mut e = Engine::new(10, 4);
    e.feed(b"abcdefghijKLMNOPQRST");
    e.resize(5, 4);
    assert_eq!(
        snapshot(&mut e).unwrap().contents,
        "abcde\nfghij\nKLMNO\nPQRST"
    );
    e.resize(10, 4);
    assert_eq!(snapshot(&mut e).unwrap().contents, "abcdefghij\nKLMNOPQRST");
    println!("compatibility: reflow exported through Viewport pass");

    for (input, expected) in [
        (b"\x1b[9mX".as_slice(), 1),
        (b"\x1b]8;;https://example.invalid\x1b\\X\x1b]8;;\x1b\\", 2),
        (b"\x1b]4;1;rgb:01/02/03\x1b\\X", 4),
        (b"\x1b]2;title\x07X", 8),
    ] {
        let mut e = Engine::new(8, 2);
        e.feed(input);
        assert!(
            matches!(snapshot(&mut e),Err(ExportError::Unsupported(bits)) if bits & expected != 0)
        );
    }
    let mut e = Engine::new(8, 2);
    let mut info = RawFrame::default();
    let mut cells = [RawCell::default(); 16];
    let mut text = [0x7fu8; 1];
    assert_eq!(
        unsafe {
            probe_frame(
                e.raw(),
                &mut info,
                size_of::<RawFrame>(),
                cells.as_mut_ptr(),
                size_of::<RawCell>(),
                cells.len(),
                text.as_mut_ptr(),
                0,
            )
        },
        -2
    );
    assert_eq!(text, [0x7f]);
    assert_eq!(
        unsafe {
            probe_frame(
                e.raw(),
                &mut info,
                size_of::<RawFrame>() - 1,
                cells.as_mut_ptr(),
                size_of::<RawCell>(),
                cells.len(),
                text.as_mut_ptr(),
                text.len(),
            )
        },
        -2
    );
    let mut e = Engine::new(8, 2);
    // A base plus 64 four-byte combining marks exceeds the 256-byte export cap
    // while staying within this engine pin's 64-suffix-codepoint retention cap.
    e.feed(format!("e{}", "\u{1d185}".repeat(64)).as_bytes());
    assert!(matches!(snapshot(&mut e), Err(ExportError::Boundary(_))));
    println!("compatibility: unsupported style/link/palette/metadata and ABI/buffer refusal pass");
    println!(
        "unsupported_contracts=title,cwd,bell,OSC_notices,extended_styles,hyperlinks,dynamic_colors,input_encoding,exact_history_cap,cursor_shape,selection,graphics; compatibility_success=true"
    );
}

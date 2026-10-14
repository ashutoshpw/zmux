use zmux_terminal_adapter::{Size, Terminal};

#[test]
fn split_utf8_and_escape_sequences_preserve_visible_output() {
    let mut terminal = Terminal::new(Size { rows: 4, cols: 30 }).unwrap();
    // Independent expected output; byte-by-byte delivery includes fragmented UTF-8 and CSI.
    for byte in "discard\r\x1b[2Khéllo 世界\r\nnext".as_bytes() {
        terminal.process(&[*byte]).unwrap();
    }
    assert_eq!(terminal.viewport().contents, "héllo 世界\nnext");
    assert_eq!(terminal.viewport().cursor_row, 1);
}

#[test]
fn invalid_resize_preserves_screen_and_valid_resize_updates_geometry() {
    let mut terminal = Terminal::new(Size { rows: 4, cols: 30 }).unwrap();
    terminal.process(b"keep").unwrap();
    assert!(terminal.resize(Size { rows: 0, cols: 30 }).is_err());
    assert_eq!(terminal.viewport().contents, "keep");
    assert_eq!(terminal.viewport().size, Size { rows: 4, cols: 30 });
    terminal.resize(Size { rows: 8, cols: 50 }).unwrap();
    assert_eq!(terminal.viewport().size, Size { rows: 8, cols: 50 });
    assert_eq!(terminal.viewport().contents, "keep");
}

#[test]
fn unterminated_control_strings_fail_explicitly_instead_of_growing_without_bound() {
    use zmux_terminal_adapter::{MAX_CONTROL_STRING_BYTES, TerminalError};
    let mut terminal = Terminal::new(Size { rows: 4, cols: 30 }).unwrap();
    terminal.process(b"\x1b]0;").unwrap();
    let result = terminal.process(&vec![b'x'; MAX_CONTROL_STRING_BYTES]);
    assert!(matches!(result, Err(TerminalError::ControlStringTooLong)));
}

#[test]
fn terminated_control_strings_and_ordinary_unicode_do_not_accumulate_the_limit() {
    let mut terminal = Terminal::new(Size { rows: 4, cols: 30 }).unwrap();
    for _ in 0..20 {
        terminal.process(b"\x1b]0;title\x07").unwrap();
        terminal.process(b"\x1b]0;title\x1b\\").unwrap();
    }
    terminal.process("ĝ".repeat(20_000).as_bytes()).unwrap();
    terminal.process(b"\r\x1b[2Kstill-responsive").unwrap();
    assert!(terminal.viewport().contents.contains("still-responsive"));
}

#[test]
fn native_runs_export_color_and_keep_wide_unicode_at_grid_positions() {
    use zmux_terminal_adapter::Color;
    let mut terminal = Terminal::new(Size { rows: 4, cols: 30 }).unwrap();
    terminal
        .process("\x1b[31mRED\x1b[0m界x".as_bytes())
        .unwrap();
    let viewport = terminal.viewport();
    assert_eq!(viewport.lines[0][0].text, "RED");
    assert_eq!(viewport.lines[0][0].style.foreground, Color::Indexed(1));
    assert_eq!(viewport.lines[0][1].column, 3);
    assert_eq!(viewport.lines[0][1].width, 2);
    assert_eq!(viewport.lines[0][1].text, "界");
    assert_eq!(viewport.lines[0][2].column, 5);
    assert!(viewport.lines[0][2].text.starts_with('x'));
}

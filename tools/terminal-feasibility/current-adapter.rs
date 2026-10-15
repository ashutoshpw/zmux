// SPDX-License-Identifier: MIT
// Original current-adapter observation driver; records known gaps without claiming parity.
use zmux_terminal_adapter::{Size, Terminal};
fn show(name: &str, terminal: &Terminal) {
    let v = terminal.viewport();
    let rows: Vec<_> = v
        .lines
        .iter()
        .map(|runs| {
            runs.iter()
                .map(|r| r.text.as_str())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect();
    println!(
        "{name}: rows={rows:?} cursor=({}, {}) history={}",
        v.cursor_row, v.cursor_col, v.scrollback_lines
    );
}
fn main() {
    let mut t = Terminal::new(Size { rows: 4, cols: 10 }).unwrap();
    t.process(b"abcdefghijKLMNOPQRST").unwrap();
    show("wrapped_10", &t);
    t.resize(Size { rows: 4, cols: 5 }).unwrap();
    show("narrow_5", &t);
    t.resize(Size { rows: 4, cols: 10 }).unwrap();
    show("wide_10", &t);
    for (name, text) in [
        ("combining", "e\u{301}x"),
        ("cjk", "界x"),
        ("zwj_mode2027", "\u{1b}[?2027h👩\u{200d}💻x"),
        ("emoji_vs16_mode2027", "\u{1b}[?2027h❤\u{fe0f}x"),
    ] {
        let mut t = Terminal::new(Size { rows: 2, cols: 12 }).unwrap();
        for b in text.as_bytes() {
            t.process(&[*b]).unwrap();
        }
        show(name, &t);
    }
}

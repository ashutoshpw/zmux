// SPDX-License-Identifier: MIT
// Original MIT fixture using the shared single-threaded C owner.
mod owner;
use owner::Engine;
fn show(name: &str, e: &mut Engine) -> Vec<String> {
    let (rows, x, y) = e.snapshot();
    println!("{name}: rows={rows:?} cursor=({y}, {x})");
    rows
}
fn main() {
    let mut e = Engine::new(10, 4);
    e.feed(b"abcdefghijKLMNOPQRST");
    let original = show("wrapped_10", &mut e);
    e.resize(5, 4);
    let narrow = show("narrow_5", &mut e);
    assert_eq!(narrow, ["abcde", "fghij", "KLMNO", "PQRST"]);
    e.resize(10, 4);
    let wide = show("wide_10", &mut e);
    assert_eq!(wide.concat(), "abcdefghijKLMNOPQRST");
    assert_eq!(original, ["abcdefghij", "KLMNOPQRST", "", ""]);
    for (name, text) in [
        ("combining", "e\u{301}x"),
        ("cjk", "界x"),
        ("zwj_mode2027", "\u{1b}[?2027h👩\u{200d}💻x"),
        ("emoji_vs16_mode2027", "\u{1b}[?2027h❤\u{fe0f}x"),
    ] {
        let mut e = Engine::new(12, 2);
        for b in text.as_bytes() {
            e.feed(&[*b]);
        }
        show(name, &mut e);
    }
    let mut e = Engine::new(10, 4);
    e.feed(b"\x1b[6n");
    assert_eq!(e.replies(), b"\x1b[1;1R");
    println!("DSR=1b5b313b3152");
    for _ in 0..128 {
        let mut e = Engine::new(10, 4);
        e.feed(b"ok");
        assert_eq!(e.snapshot().0[0], "ok");
    }
    println!("owned_snapshot_survives_resize=true; lifecycle_iterations=128; success=true");
}

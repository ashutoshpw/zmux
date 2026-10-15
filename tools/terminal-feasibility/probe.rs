// SPDX-License-Identifier: MIT
// Original MIT fixture. A single-threaded Rust owner never exports borrowed C pointers.
use std::{ffi::c_void, marker::PhantomData, ptr::NonNull, rc::Rc};
unsafe extern "C" {
    fn probe_new(cols: u16, rows: u16) -> *mut c_void;
    fn probe_free(p: *mut c_void);
    fn probe_feed(p: *mut c_void, bytes: *const u8, n: usize) -> i32;
    fn probe_resize(p: *mut c_void, cols: u16, rows: u16) -> i32;
    fn probe_copy(
        p: *mut c_void,
        out: *mut u8,
        cap: usize,
        n: *mut usize,
        x: *mut u16,
        y: *mut u16,
    ) -> i32;
    fn probe_replies(p: *mut c_void, out: *mut u8, cap: usize, n: *mut usize) -> i32;
}
struct Engine {
    raw: NonNull<c_void>,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Engine {
    fn drop(&mut self) {
        unsafe { probe_free(self.raw.as_ptr()) }
    }
}
impl Engine {
    fn new(cols: u16, rows: u16) -> Self {
        Self {
            raw: NonNull::new(unsafe { probe_new(cols, rows) }).expect("create"),
            _thread: PhantomData,
        }
    }
    fn feed(&mut self, s: &[u8]) {
        assert_eq!(
            unsafe { probe_feed(self.raw.as_ptr(), s.as_ptr(), s.len()) },
            0
        );
    }
    fn resize(&mut self, cols: u16, rows: u16) {
        assert_eq!(unsafe { probe_resize(self.raw.as_ptr(), cols, rows) }, 0);
    }
    fn snapshot(&mut self) -> (Vec<String>, u16, u16) {
        let mut b = vec![0u8; 512 * 1024];
        let (mut n, mut x, mut y) = (0, 0, 0);
        assert_eq!(
            unsafe {
                probe_copy(
                    self.raw.as_ptr(),
                    b.as_mut_ptr(),
                    b.len(),
                    &mut n,
                    &mut x,
                    &mut y,
                )
            },
            0
        );
        assert!(n <= b.len());
        b.truncate(n);
        let text = String::from_utf8(b).expect("UTF8");
        (
            text.lines().map(|s| s.trim_end().to_owned()).collect(),
            x,
            y,
        )
    }
    fn replies(&mut self) -> Vec<u8> {
        let mut b = vec![0u8; 4096];
        let mut n = 0;
        assert_eq!(
            unsafe { probe_replies(self.raw.as_ptr(), b.as_mut_ptr(), b.len(), &mut n) },
            0
        );
        assert!(n <= b.len());
        b.truncate(n);
        b
    }
}
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

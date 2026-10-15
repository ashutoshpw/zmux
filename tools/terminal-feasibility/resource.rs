// SPDX-License-Identifier: MIT
// Independent real-engine retention and mapping-quota contracts; no application linkage.
use std::{ffi::c_void, marker::PhantomData, mem::size_of, ptr::NonNull, rc::Rc};

const LIMIT: usize = 16 * 1024 * 1024;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Stats {
    reserved: u64,
    peak: u64,
    general: u64,
    pages: u64,
    denials: u64,
    invalid: u64,
    calls: u64,
    general_calls: u64,
    page_calls: u64,
    live: u64,
}
unsafe extern "C" {
    fn resource_new(
        cols: u16,
        rows: u16,
        history: usize,
        total: usize,
        general: usize,
        pages: usize,
        deny_at: usize,
        bounded: u8,
    ) -> *mut c_void;
    fn resource_ready(p: *mut c_void) -> i32;
    fn resource_stats(p: *mut c_void, out: *mut Stats, size: usize) -> i32;
    fn resource_close(p: *mut c_void);
    fn resource_free(p: *mut c_void);
    fn resource_limits(p: *mut c_void, total: usize, general: usize, pages: usize) -> i32;
    fn resource_feed(p: *mut c_void, bytes: *const u8, n: usize) -> i32;
    fn resource_resize(p: *mut c_void, cols: u16, rows: u16) -> i32;
    fn resource_clone(p: *mut c_void, history: *mut usize) -> i32;
    fn resource_history(p: *mut c_void, history: *mut usize) -> i32;
    fn resource_scroll(p: *mut c_void, lines: i32) -> i32;
    fn resource_copy(p: *mut c_void, out: *mut u8, cap: usize, n: *mut usize) -> i32;
    fn resource_host_storage_bytes() -> usize;
}
struct Engine {
    raw: NonNull<c_void>,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Engine {
    fn drop(&mut self) {
        unsafe { resource_free(self.raw.as_ptr()) }
    }
}
impl Engine {
    fn new(
        cols: u16,
        rows: u16,
        history: usize,
        pages: usize,
        deny_at: usize,
        bounded: bool,
    ) -> Self {
        Self {
            raw: NonNull::new(unsafe {
                resource_new(
                    cols,
                    rows,
                    history,
                    LIMIT,
                    LIMIT,
                    pages,
                    deny_at,
                    u8::from(bounded),
                )
            })
            .expect("fixture owner"),
            _thread: PhantomData,
        }
    }
    fn ptr(&self) -> *mut c_void {
        self.raw.as_ptr()
    }
    fn ready(&self) -> bool {
        unsafe { resource_ready(self.ptr()) == 1 }
    }
    fn stats(&self) -> Stats {
        let mut out = Stats::default();
        assert_eq!(
            unsafe { resource_stats(self.ptr(), &mut out, size_of::<Stats>()) },
            0
        );
        assert_eq!(out.invalid, 0, "{out:?}");
        assert!(
            out.reserved <= LIMIT as u64 && out.peak <= LIMIT as u64,
            "{out:?}"
        );
        assert_eq!(out.reserved, out.pages + out.general);
        out
    }
    fn history(&self) -> usize {
        let mut n = 0;
        assert_eq!(unsafe { resource_history(self.ptr(), &mut n) }, 0);
        n
    }
    fn feed(&mut self, bytes: &[u8]) -> i32 {
        unsafe { resource_feed(self.ptr(), bytes.as_ptr(), bytes.len()) }
    }
    fn resize(&mut self, cols: u16, rows: u16) -> i32 {
        unsafe { resource_resize(self.ptr(), cols, rows) }
    }
    fn scroll(&mut self, lines: i32) {
        assert_eq!(unsafe { resource_scroll(self.ptr(), lines) }, 0);
    }
    fn contents(&mut self) -> String {
        let mut out = vec![0; 512 * 1024];
        let mut n = 0;
        assert_eq!(
            unsafe { resource_copy(self.ptr(), out.as_mut_ptr(), out.len(), &mut n) },
            0
        );
        assert!(n <= out.len());
        out.truncate(n);
        let value = String::from_utf8(out).unwrap();
        let mut rows: Vec<_> = value.lines().map(|v| v.trim_end_matches(' ')).collect();
        while rows.last() == Some(&"") {
            rows.pop();
        }
        rows.join("\n")
    }
    fn clone_storage(&mut self) -> Result<usize, ()> {
        let mut n = 0;
        if unsafe { resource_clone(self.ptr(), &mut n) } == 0 {
            Ok(n)
        } else {
            Err(())
        }
    }
    fn limits(&mut self, total: usize, general: usize, pages: usize) {
        assert_eq!(
            unsafe { resource_limits(self.ptr(), total, general, pages) },
            0
        );
    }
    fn close(&mut self) {
        unsafe { resource_close(self.ptr()) };
        let stats = self.stats();
        assert_eq!(
            (stats.reserved, stats.general, stats.pages, stats.live),
            (0, 0, 0, 0),
            "{stats:?}"
        );
    }
}
fn exact_history(bounded: bool) {
    let mut e = Engine::new(8, 3, 1, LIMIT, 0, bounded);
    assert!(e.ready());
    for byte in b"0\r\n1\r\n2\r\n3\r\n4" {
        assert_eq!(e.feed(&[*byte]), 0);
        assert!(
            e.history() <= 1,
            "retained {} rows with limit1",
            e.history()
        );
    }
    assert_eq!(e.history(), 1);
    assert_eq!(e.contents(), "2\n3\n4");
    e.scroll(1);
    assert_eq!(e.contents(), "1\n2\n3");
    e.scroll(0);
    assert_eq!(e.feed(b"\r\n5"), 0);
    assert_eq!(e.contents(), "3\n4\n5");
    assert_eq!(e.history(), 1);
    e.close();
}
fn main() {
    let original = std::env::args().nth(1);
    if original.as_deref() == Some("--original-constructor-history") {
        exact_history(false);
        return;
    }
    let bounded = original.as_deref() != Some("--original-constructor-pages");
    let mut denied = Engine::new(8, 3, 1000, 0, 0, bounded);
    assert!(
        !denied.ready(),
        "zero page quota must reject construction before mapping preheat"
    );
    assert!(denied.stats().denials > 0);
    denied.close();
    exact_history(true);
    let mut zero = Engine::new(8, 3, 0, LIMIT, 0, true);
    assert!(zero.ready());
    assert_eq!(zero.feed(b"0\r\n1\r\n2\r\n3\r\n4"), 0);
    assert_eq!(zero.history(), 0);
    assert_eq!(zero.contents(), "2\n3\n4");
    zero.close();
    println!("resource: zero page quota and fragmented exact-one-row retention pass");

    let mut e = Engine::new(12, 3, 1000, LIMIT, 0, true);
    assert!(e.ready());
    let construction_calls = e.stats().calls;
    assert!(e.stats().pages > 0 && e.stats().general > 0);
    let input = (0..1003)
        .map(|i| format!("L{i:04}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    for chunk in input.as_bytes().chunks(137) {
        assert_eq!(e.feed(chunk), 0);
        assert!(e.history() <= 1000);
        e.stats();
    }
    assert_eq!(e.history(), 1000);
    assert_eq!(e.contents(), "L1000\nL1001\nL1002");
    e.scroll(1000);
    assert_eq!(e.contents(), "L0000\nL0001\nL0002");
    e.scroll(0);
    for (next, oldest) in [(1003, "L0001\nL0002\nL0003"), (1004, "L0002\nL0003\nL0004")] {
        assert_eq!(e.feed(format!("\r\nL{next:04}").as_bytes()), 0);
        assert_eq!(e.history(), 1000);
        e.scroll(1000);
        assert_eq!(e.contents(), oldest);
        e.scroll(0);
    }
    assert_eq!(e.contents(), "L1002\nL1003\nL1004");
    e.scroll(1000);
    assert_eq!(e.contents(), "L0002\nL0003\nL0004");
    e.scroll(0);
    let before = e.stats();
    assert_eq!(e.clone_storage(), Ok(1000));
    let after = e.stats();
    assert_eq!((before.reserved, before.live), (after.reserved, after.live));
    assert!(after.page_calls > before.page_calls && after.peak > before.reserved);
    assert_eq!(e.resize(3, 2), 0);
    assert_eq!(e.history(), 1000);
    assert_eq!(e.contents(), "L10\n04");
    assert_eq!(e.resize(12, 5), 0);
    assert!(e.history() <= 1000);
    assert_eq!(e.contents(), "L1000\nL1001\nL1002\nL1003\nL1004");
    assert_eq!(e.feed(b"\r\nEND"), 0);
    assert!(e.contents().ends_with("END"));
    let pages_before = e.stats().page_calls;
    assert_eq!(e.feed(b"\x1b[?1049h\x1b[HALT"), 0);
    assert_eq!(e.history(), 0);
    assert!(e.stats().page_calls > pages_before);
    assert_eq!(e.resize(10, 4), 0);
    assert_eq!(e.history(), 0);
    assert_eq!(e.feed(b"\x1b[?1049l\r\nBACK"), 0);
    assert!(e.history() <= 1000);
    assert!(e.contents().ends_with("BACK"));
    assert_eq!(e.feed(b"\x1bcRESET"), 0);
    assert_eq!(e.history(), 0);
    assert_eq!(e.contents(), "RESET");
    println!(
        "resource: exact1000/newest contents, clone, resize/reflow, alternate resizing/reset pass; peak={}",
        e.stats().peak
    );
    e.close();

    for deny_at in 1..=usize::try_from(construction_calls).unwrap() {
        let mut e = Engine::new(12, 3, 1000, LIMIT, deny_at, true);
        assert!(!e.ready(), "denied constructor allocation{deny_at}");
        assert!(e.stats().denials > 0);
        e.close();
    }
    println!(
        "resource: all {construction_calls} observed constructor allocation failure positions clean up"
    );
    for action in [
        "alternate-pages",
        "history-pages",
        "clone-pages",
        "resize-pages",
        "alternate-general",
        "aggregate",
    ] {
        let (cols, rows) = if action == "history-pages" {
            (240, 120)
        } else {
            (12, 3)
        };
        let mut e = Engine::new(cols, rows, 1000, LIMIT, 0, true);
        assert!(e.ready());
        assert_eq!(e.feed(b"initial\r\nnext"), 0);
        if action == "resize-pages" {
            assert_eq!(e.resize(240, 3), 0);
            let line = format!("{}\r\n", "x".repeat(240));
            for _ in 0..1005 {
                assert_eq!(e.feed(line.as_bytes()), 0);
            }
        }
        let stats = e.stats();
        match action {
            "alternate-general" => e.limits(LIMIT, stats.general as usize, LIMIT),
            "aggregate" => e.limits(stats.reserved as usize, LIMIT, LIMIT),
            _ => e.limits(LIMIT, LIMIT, stats.pages as usize),
        }
        let result = match action {
            "clone-pages" => e.clone_storage().map_or(-1, |_| 0),
            "resize-pages" => e.resize(1, 120),
            "history-pages" => {
                let line = format!("{}\r\n", "x".repeat(240));
                let mut result = 0;
                for _ in 0..4096 {
                    result = e.feed(line.as_bytes());
                    if result != 0 {
                        break;
                    }
                    assert!(e.history() <= 1000);
                }
                assert!(e.stats().page_calls > stats.page_calls);
                result
            }
            _ => e.feed(b"\x1b[?1049h"),
        };
        assert_eq!(result, -1, "quota denied {action}");
        assert!(e.stats().denials > 0);
        assert!(!e.ready());
        assert_eq!(e.feed(b"must not consume"), -1);
        e.close();
        println!("resource: sticky {action} denial and zero live mappings pass");
    }
    println!(
        "resource_host_storage_bytes={}; engine_reserved_mapping_limit={LIMIT}; total_app_memory_bound=false; resource_success=true",
        unsafe { resource_host_storage_bytes() }
    );
}

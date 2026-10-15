// SPDX-License-Identifier: MIT
// Real bounded terminal + callback collector, with two preallocated publication slots.
use std::{ffi::c_void, marker::PhantomData, mem::size_of, ptr::NonNull, rc::Rc};
const ENGINE_LIMIT: usize = 16 * 1024 * 1024;
const TEXT: usize = 512 * 1024;
const EVENTS: usize = 1024;
const REPLIES: usize = 4096;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct EngineStats {
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
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Stats {
    engine: EngineStats,
    fixed_host: u64,
    managed_limit: u64,
    managed_current: u64,
    managed_peak: u64,
    operation_id: u64,
    committed_id: u64,
    failures: u32,
    phase: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Frame {
    operation_id: u64,
    text_length: usize,
    event_length: usize,
    reply_length: usize,
    x: u16,
    y: u16,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Event {
    kind: u32,
    offset: u32,
    length: u32,
}
unsafe extern "C" {
    fn integrated_fixed_host_bytes() -> usize;
    fn integrated_new(
        cols: u16,
        rows: u16,
        history: usize,
        managed: usize,
        general: usize,
        pages: usize,
        deny_at: usize,
        replies: usize,
        events: usize,
    ) -> *mut c_void;
    fn integrated_stats(p: *mut c_void, out: *mut Stats, size: usize) -> i32;
    fn integrated_limits(p: *mut c_void, total: usize, general: usize, pages: usize) -> i32;
    fn integrated_feed(p: *mut c_void, bytes: *const u8, n: usize) -> i32;
    fn integrated_resize(p: *mut c_void, cols: u16, rows: u16) -> i32;
    fn integrated_scroll(p: *mut c_void, lines: i32) -> i32;
    fn integrated_history(p: *mut c_void, history: *mut usize) -> i32;
    fn integrated_clone(p: *mut c_void, history: *mut usize) -> i32;
    fn integrated_export(
        p: *mut c_void,
        frame: *mut Frame,
        frame_size: usize,
        text: *mut u8,
        text_cap: usize,
        events: *mut Event,
        event_size: usize,
        event_cap: usize,
        replies: *mut u8,
        reply_cap: usize,
    ) -> i32;
    fn integrated_commit(p: *mut c_void, operation: u64) -> i32;
    fn integrated_abort(p: *mut c_void);
    fn integrated_close(p: *mut c_void);
    fn integrated_free(p: *mut c_void);
}
struct Raw {
    ptr: NonNull<c_void>,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Raw {
    fn drop(&mut self) {
        unsafe { integrated_free(self.ptr.as_ptr()) }
    }
}
impl Raw {
    fn try_new(
        cols: u16,
        rows: u16,
        history: usize,
        deny_at: usize,
        replies: usize,
        events: usize,
    ) -> Result<Self, HostError> {
        let fixed = unsafe { integrated_fixed_host_bytes() };
        Ok(Self {
            ptr: NonNull::new(unsafe {
                integrated_new(
                    cols,
                    rows,
                    history,
                    fixed + ENGINE_LIMIT,
                    ENGINE_LIMIT,
                    ENGINE_LIMIT,
                    deny_at,
                    replies,
                    events,
                )
            })
            .ok_or(HostError::EngineConstruction)?,
            _thread: PhantomData,
        })
    }
    fn new(
        cols: u16,
        rows: u16,
        history: usize,
        deny_at: usize,
        replies: usize,
        events: usize,
    ) -> Self {
        Self::try_new(cols, rows, history, deny_at, replies, events)
            .expect("fixture integrated owner")
    }
    fn ptr(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
    fn stats(&self) -> Stats {
        let mut s = Stats::default();
        assert_eq!(
            unsafe { integrated_stats(self.ptr(), &mut s, size_of::<Stats>()) },
            0
        );
        assert_eq!(s.engine.invalid, 0, "{s:?}");
        assert_eq!(s.engine.reserved, s.engine.general + s.engine.pages);
        assert_eq!(s.managed_current, s.fixed_host + s.engine.reserved);
        assert_eq!(s.managed_peak, s.fixed_host + s.engine.peak);
        assert!(
            s.managed_current <= s.managed_limit && s.engine.peak <= ENGINE_LIMIT as u64,
            "{s:?}"
        );
        s
    }
    fn feed(&mut self, b: &[u8]) -> i32 {
        unsafe { integrated_feed(self.ptr(), b.as_ptr(), b.len()) }
    }
    fn limits(&mut self, total: usize, general: usize, pages: usize) {
        assert_eq!(
            unsafe { integrated_limits(self.ptr(), total, general, pages) },
            0
        )
    }
    fn history(&self) -> usize {
        let mut n = 0;
        assert_eq!(unsafe { integrated_history(self.ptr(), &mut n) }, 0);
        n
    }
    fn close(&mut self) {
        unsafe { integrated_close(self.ptr()) };
        let s = self.stats();
        assert_eq!(
            (
                s.engine.reserved,
                s.engine.general,
                s.engine.pages,
                s.engine.live
            ),
            (0, 0, 0, 0),
            "{s:?}"
        );
        assert_eq!(s.phase, 3)
    }
}
struct Slot {
    frame: Frame,
    text: Vec<u8>,
    events: Vec<Event>,
    replies: Vec<u8>,
    notices: [u64; 32],
    notice_count: usize,
    next_notice: u64,
    evicted: u64,
}
fn buffer<T: Clone + Default>(n: usize) -> Result<Vec<T>, std::collections::TryReserveError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n)?;
    v.resize(n, T::default());
    Ok(v)
}
impl Slot {
    fn new() -> Result<Self, std::collections::TryReserveError> {
        Ok(Self {
            frame: Frame::default(),
            text: buffer(TEXT)?,
            events: buffer(EVENTS)?,
            replies: buffer(REPLIES)?,
            notices: [0; 32],
            notice_count: 0,
            next_notice: 0,
            evicted: 0,
        })
    }
    fn capacity_bytes(&self) -> usize {
        self.text.capacity() + self.events.capacity() * size_of::<Event>() + self.replies.capacity()
    }
    fn text(&self) -> &str {
        std::str::from_utf8(&self.text[..self.frame.text_length]).unwrap()
    }
    fn replies(&self) -> &[u8] {
        &self.replies[..self.frame.reply_length]
    }
    fn export(&mut self, raw: &Raw, event_cap: usize, reply_cap: usize) -> i32 {
        unsafe {
            integrated_export(
                raw.ptr(),
                &mut self.frame,
                size_of::<Frame>(),
                self.text.as_mut_ptr(),
                TEXT,
                self.events.as_mut_ptr(),
                size_of::<Event>(),
                event_cap,
                self.replies.as_mut_ptr(),
                reply_cap,
            )
        }
    }
    fn validate(&mut self, last: &Slot) -> Result<(), ()> {
        if self.frame.text_length > TEXT
            || self.frame.event_length > EVENTS
            || self.frame.reply_length > REPLIES
            || self.frame.operation_id <= last.frame.operation_id
            || std::str::from_utf8(&self.text[..self.frame.text_length]).is_err()
        {
            return Err(());
        }
        self.notices = last.notices;
        self.notice_count = last.notice_count;
        self.next_notice = last.next_notice;
        self.evicted = last.evicted;
        let mut offset = 0usize;
        for e in &self.events[..self.frame.event_length] {
            match e.kind {
                1 if e.offset == 0 && e.length == 0 => {
                    self.next_notice = self.next_notice.checked_add(1).ok_or(())?;
                    if self.notice_count == 32 {
                        self.notices.copy_within(1..32, 0);
                        self.notice_count = 31;
                        self.evicted = self.evicted.checked_add(1).ok_or(())?
                    }
                    self.notices[self.notice_count] = self.next_notice;
                    self.notice_count += 1;
                }
                2 if e.offset as usize == offset => {
                    offset = offset.checked_add(e.length as usize).ok_or(())?;
                    if offset > self.frame.reply_length {
                        return Err(());
                    }
                }
                3 if e.offset == 0 && e.length == 0 => {}
                _ => return Err(()),
            }
        }
        if offset != self.frame.reply_length {
            return Err(());
        }
        Ok(())
    }
}
struct Owner {
    raw: Raw,
    last: Slot,
    candidate: Slot,
    failed: bool,
}
#[derive(Debug)]
enum HostError {
    Budget,
    Allocation,
    EngineConstruction,
}
impl Owner {
    fn try_new(
        cols: u16,
        rows: u16,
        history: usize,
        host_budget: usize,
    ) -> Result<Self, HostError> {
        // All backing storage is fallibly reserved before engine construction.
        let minimum = 2 * (TEXT + EVENTS * size_of::<Event>() + REPLIES) + size_of::<Self>();
        if host_budget < minimum {
            return Err(HostError::Budget);
        }
        let last = Slot::new().map_err(|_| HostError::Allocation)?;
        let candidate = Slot::new().map_err(|_| HostError::Allocation)?;
        if last.capacity_bytes() + candidate.capacity_bytes() + size_of::<Self>() > host_budget {
            return Err(HostError::Budget);
        }
        let raw = Raw::try_new(cols, rows, history, 0, REPLIES, EVENTS)?;
        if raw.stats().phase != 0 {
            return Err(HostError::EngineConstruction);
        }
        Ok(Self {
            raw,
            last,
            candidate,
            failed: false,
        })
    }
    fn new(cols: u16, rows: u16, history: usize) -> Self {
        Self::try_new(cols, rows, history, 2 * 1024 * 1024).expect("bounded host slots")
    }
    fn finish(&mut self, status: i32) -> Result<(), ()> {
        if status != 0
            || self.candidate.export(&self.raw, EVENTS, REPLIES) != 0
            || self.candidate.validate(&self.last).is_err()
        {
            self.failed = true;
            unsafe { integrated_abort(self.raw.ptr()) };
            return Err(());
        }
        if unsafe { integrated_commit(self.raw.ptr(), self.candidate.frame.operation_id) } != 0 {
            self.failed = true;
            unsafe { integrated_abort(self.raw.ptr()) };
            return Err(());
        }
        std::mem::swap(&mut self.last, &mut self.candidate);
        Ok(())
    }
    fn feed(&mut self, b: &[u8]) -> Result<(), ()> {
        if self.failed {
            return Err(());
        }
        let status = self.raw.feed(b);
        self.finish(status)
    }
    fn resize(&mut self, c: u16, r: u16) -> Result<(), ()> {
        if self.failed {
            return Err(());
        }
        let status = unsafe { integrated_resize(self.raw.ptr(), c, r) };
        self.finish(status)
    }
    fn scroll(&mut self, n: i32) -> Result<(), ()> {
        if self.failed {
            return Err(());
        }
        let status = unsafe { integrated_scroll(self.raw.ptr(), n) };
        self.finish(status)
    }
    fn contents(&self) -> String {
        self.last
            .text()
            .lines()
            .map(|s| s.trim_end_matches(' '))
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end_matches('\n')
            .to_owned()
    }
    fn assert_sticky(&mut self, generation: u64, contents: &str) {
        assert!(self.failed);
        assert_eq!(self.last.frame.operation_id, generation);
        assert_eq!(self.contents(), contents);
        assert!(self.feed(b"do not consume or replay").is_err());
        assert!(self.resize(8, 3).is_err());
        self.raw.close()
    }
}
fn main() {
    let fixed = unsafe { integrated_fixed_host_bytes() };
    assert!(
        unsafe { integrated_new(8, 3, 1, fixed - 1, 0, 0, 0, REPLIES, EVENTS) }.is_null(),
        "host allowance must be reserved before engine construction"
    );
    assert!(matches!(Owner::try_new(8, 3, 1, 0), Err(HostError::Budget)));
    assert!(
        matches!(
            Owner::try_new(0, 3, 1, 2 * 1024 * 1024),
            Err(HostError::EngineConstruction)
        ),
        "nullable C construction must return an error, not panic"
    );
    let mut o = Owner::new(8, 3, 1);
    let constructor_calls = o.raw.stats().engine.calls;
    for b in b"0\r\n1\r\n2\r\n3\r\n4" {
        o.feed(&[*b]).unwrap();
        assert!(o.raw.history() <= 1)
    }
    assert_eq!(o.contents(), "2\n3\n4");
    o.scroll(1).unwrap();
    assert_eq!(o.contents(), "1\n2\n3");
    o.scroll(0).unwrap();
    o.feed(b"\x07\x1b[6n\x1bc\x07").unwrap();
    assert_eq!(
        o.last.events[..o.last.frame.event_length]
            .iter()
            .map(|e| e.kind)
            .collect::<Vec<_>>(),
        [1, 2, 3, 1]
    );
    assert_eq!(o.last.replies(), b"\x1b[3;2R");
    assert_eq!(&o.last.notices[..o.last.notice_count], [1, 2]);
    o.feed(b"owned").unwrap();
    o.resize(3, 2).unwrap();
    assert_eq!(o.contents(), "own\ned");
    o.resize(12, 3).unwrap();
    assert_eq!(o.contents(), "owned");
    let mut history = 0;
    let status = unsafe { integrated_clone(o.raw.ptr(), &mut history) };
    o.finish(status).unwrap();
    assert!(history <= 1);
    o.feed(b"\x1b[?1049hALT").unwrap();
    assert_eq!(o.raw.history(), 0);
    o.feed(b"\x1b[?1049l").unwrap();
    assert_eq!(o.contents(), "owned");
    let owned = o.contents();
    let cap = o.last.capacity_bytes() + o.candidate.capacity_bytes();
    o.raw.close();
    drop(o);
    assert_eq!(owned, "owned");
    println!(
        "integrated: one bounded terminal exact retention/order/reset/reflow/clone/alternate and owned copy pass; rust_slot_capacity_bytes={cap}; fixed_c_bytes={fixed}"
    );
    let mut thousand = Owner::new(12, 3, 1000);
    let input = (0..1005)
        .map(|n| format!("L{n:04}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    for chunk in input.as_bytes().chunks(137) {
        thousand.feed(chunk).unwrap();
        assert!(thousand.raw.history() <= 1000);
    }
    thousand.feed(b"\x07\x1b[6n").unwrap();
    assert_eq!(thousand.raw.history(), 1000);
    assert_eq!(thousand.contents(), "L1002\nL1003\nL1004");
    assert_eq!(thousand.last.replies(), b"\x1b[3;6R");
    thousand.scroll(1000).unwrap();
    assert_eq!(thousand.contents(), "L0002\nL0003\nL0004");
    thousand.scroll(0).unwrap();
    thousand.resize(3, 2).unwrap();
    assert_eq!(thousand.contents(), "L10\n04");
    assert_eq!(thousand.raw.history(), 1000);
    thousand.raw.close();
    println!(
        "integrated: exact1000/newest rows, callbacks and reflow share one allocator/terminal; rust_owner_inline_bytes={}",
        size_of::<Owner>()
    );
    for deny in 1..=constructor_calls {
        let mut r = Raw::new(8, 3, 1, deny as usize, REPLIES, EVENTS);
        let s = r.stats();
        assert_eq!(s.phase, 2);
        assert!(s.engine.denials > 0);
        r.close()
    }
    println!("integrated: all{constructor_calls} constructor denial positions clean up");
    for action in ["pages", "general", "aggregate", "history"] {
        let mut o = if action == "history" {
            Owner::new(240, 120, 1000)
        } else {
            Owner::new(8, 3, 1)
        };
        o.feed(b"last good").unwrap();
        let generation = o.last.frame.operation_id;
        let contents = o.contents();
        let s = o.raw.stats();
        match action {
            "general" => o.raw.limits(
                s.managed_limit as usize,
                s.engine.general as usize,
                ENGINE_LIMIT,
            ),
            "aggregate" => o
                .raw
                .limits(s.managed_current as usize, ENGINE_LIMIT, ENGINE_LIMIT),
            _ => o.raw.limits(
                s.managed_limit as usize,
                ENGINE_LIMIT,
                s.engine.pages as usize,
            ),
        }
        if action == "history" {
            let line = format!("\x07\x1b[6n{}\r\n", "x".repeat(240));
            let mut failed = false;
            for _ in 0..4096 {
                if o.feed(line.as_bytes()).is_err() {
                    failed = true;
                    break;
                }
            }
            assert!(failed)
        } else {
            assert!(o.feed(b"\x07\x1b[6n\x1b[?1049h\x1bc\x07").is_err());
        }
        let s = o.raw.stats();
        assert!(s.failures & 1 != 0 && s.engine.denials > 0);
        assert_eq!(s.engine.invalid, 0);
        if action == "history" {
            let committed = o.raw.stats().committed_id;
            assert_eq!(o.last.frame.operation_id, committed);
            o.assert_sticky(committed, &o.contents());
        } else {
            o.assert_sticky(generation, &contents)
        }
        println!(
            "integrated: {action} allocation denial fences callbacks/publication and zero teardown"
        );
    }
    // A successful mutation can need fresh render arrays. Tighten only after the
    // resize and before export, independently observing that the denial is in export.
    let mut o = Owner::new(8, 3, 1);
    o.feed(b"last good\x07").unwrap();
    let generation = o.last.frame.operation_id;
    let contents = o.contents();
    assert_eq!(unsafe { integrated_resize(o.raw.ptr(), 240, 120) }, 0);
    let before = o.raw.stats();
    assert_eq!(before.failures, 0);
    assert_eq!(before.phase, 1);
    o.raw.limits(
        before.managed_limit as usize,
        before.engine.general as usize,
        ENGINE_LIMIT,
    );
    assert!(
        o.finish(0).is_err(),
        "post-render allocation denial must fence publication"
    );
    let after = o.raw.stats();
    assert!(
        after.engine.denials > before.engine.denials,
        "fixture must actually deny render allocation"
    );
    assert!(after.failures & 1 != 0 && after.failures & 32 != 0);
    o.assert_sticky(generation, &contents);
    println!("integrated: actual post-render allocation denial preserves last-good generation");
    for kind in ["reply", "event", "snapshot"] {
        let mut o = Owner::new(8, 3, 1);
        o.feed(b"last good\x07").unwrap();
        let generation = o.last.frame.operation_id;
        let contents = o.contents();
        let input = match kind {
            "reply" => b"\x1b[6n".repeat(683),
            "event" => vec![7; 1025],
            _ => format!("\x1b[2J\x1b[Ha{}\x07", "\u{1d185}".repeat(64)).into_bytes(),
        };
        assert!(o.feed(&input).is_err());
        let s = o.raw.stats();
        assert_eq!(s.engine.denials, 0);
        assert!(
            s.failures
                & match kind {
                    "reply" => 4,
                    "event" => 8,
                    _ => 32,
                }
                != 0
        );
        o.assert_sticky(generation, &contents)
    }
    let mut raw = Raw::new(8, 3, 1, 0, REPLIES, EVENTS);
    assert_eq!(raw.feed(b"X"), 0);
    assert_eq!(
        raw.feed(b"Y"),
        -3,
        "even an empty effect batch must fence the staged frame"
    );
    let mut staged_history = 0;
    assert_eq!(
        unsafe { integrated_history(raw.ptr(), &mut staged_history) },
        -2
    );
    let mut slot = Slot::new().unwrap();
    assert_eq!(unsafe { integrated_commit(raw.ptr(), 1) }, -2);
    assert_eq!(slot.export(&raw, EVENTS, REPLIES), 0);
    assert_eq!(
        slot.export(&raw, EVENTS, REPLIES),
        -3,
        "validated export is sealed until commit"
    );
    let sealed = raw.stats();
    assert_eq!(
        unsafe {
            integrated_limits(
                raw.ptr(),
                sealed.managed_limit as usize,
                ENGINE_LIMIT,
                ENGINE_LIMIT,
            )
        },
        -2
    );
    assert_eq!(raw.stats().managed_limit, sealed.managed_limit);
    assert_eq!(unsafe { integrated_commit(raw.ptr(), 2) }, -2);
    assert_eq!(unsafe { integrated_commit(raw.ptr(), 1) }, 0);
    assert_eq!(unsafe { integrated_commit(raw.ptr(), 1) }, -2);
    assert_eq!(raw.feed(b"\x1b[6n"), 0);
    assert_eq!(slot.export(&raw, EVENTS, 5), -2);
    assert_eq!(raw.stats().phase, 1);
    assert_eq!(slot.export(&raw, EVENTS, REPLIES), 0);
    assert_eq!(unsafe { integrated_commit(raw.ptr(), 2) }, 0);
    raw.close();
    for _ in 0..128 {
        let mut o = Owner::new(8, 3, 1);
        o.feed(b"\x07\x1b[6n\x1bc").unwrap();
        o.raw.close()
    }
    println!(
        "integrated: reply/event/snapshot failures, exact staged commit and128 teardowns pass; total_app_memory_bound=false; real_pty_round_trip=false; integrated_success=true"
    );
}

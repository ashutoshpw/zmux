// SPDX-License-Identifier: MIT
// Original bounded coordinator: kernel reply progress precedes exact publication.
use std::time::{Duration, Instant};
const QUEUE: usize = 8192;
const REPLY_DEADLINE: Duration = Duration::from_millis(250);
const PRE: &[u8] = b"PRE:";
const POST: &[u8] = b":POST\0";
const QUERY1: &[u8] = b"\x1b]0;pty;title\x07\x1b]7;file://host/pty;path\x07\x07\x1b[6n";
const QUERY2: &[u8] = b"\x1b]777;notify;title;body;tail\x07\x1b[5n\x1b[6n";
const SEED: &[u8] = b"\x1b]2;last-good\x07";
const N: usize = 32768;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct PtyStats {
    read_bytes: u64,
    written_bytes: u64,
    write_calls: u64,
    eagain: u64,
    short_writes: u64,
    open_fds: u64,
    child_live: u64,
    cleanup_errors: u64,
    reaped: u64,
    exit_code: u64,
    cleanup_stage: u64,
    cleanup_errno: u64,
}
unsafe extern "C" {
    fn pty_new(
        python: *const std::ffi::c_char,
        script: *const std::ffi::c_char,
        mode: *const std::ffi::c_char,
    ) -> *mut c_void;
    fn pty_free(p: *mut c_void);
    fn pty_read(p: *mut c_void, out: *mut u8, cap: usize) -> std::ffi::c_long;
    fn pty_write(p: *mut c_void, bytes: *const u8, n: usize) -> std::ffi::c_long;
    fn pty_report(p: *mut c_void, out: *mut u8, cap: usize) -> std::ffi::c_long;
    fn pty_control(p: *mut c_void, byte: u8) -> i32;
    fn pty_poll(p: *mut c_void, milliseconds: u32) -> i32;
    fn pty_resize(p: *mut c_void, rows: u16, cols: u16) -> i32;
    fn pty_size(p: *mut c_void, rows: *mut u16, cols: *mut u16) -> i32;
    fn pty_wait(p: *mut c_void, milliseconds: u32) -> i32;
    fn pty_stop(p: *mut c_void) -> i32;
    fn pty_close_master(p: *mut c_void) -> i32;
    fn pty_closed_fds(p: *mut c_void) -> i32;
    fn pty_stats(p: *mut c_void, out: *mut PtyStats, size: usize) -> i32;
    fn pty_fixed_bytes() -> usize;
}
struct Transport {
    ptr: NonNull<c_void>,
}
impl Drop for Transport {
    fn drop(&mut self) {
        unsafe { pty_free(self.ptr.as_ptr()) }
    }
}
impl Transport {
    fn new(python: &str, script: &str, mode: &str) -> Self {
        let args = [python, script, mode].map(|s| std::ffi::CString::new(s).unwrap());
        Self {
            ptr: NonNull::new(unsafe {
                pty_new(args[0].as_ptr(), args[1].as_ptr(), args[2].as_ptr())
            })
            .unwrap_or_else(|| {
                panic!(
                    "kernel PTY child creation: {}",
                    std::io::Error::last_os_error()
                )
            }),
        }
    }
    fn ptr(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
    fn stats(&self) -> PtyStats {
        let mut s = PtyStats::default();
        assert_eq!(
            unsafe { pty_stats(self.ptr(), &mut s, size_of::<PtyStats>()) },
            0
        );
        s
    }
    fn control(&self, byte: u8) {
        assert_eq!(unsafe { pty_control(self.ptr(), byte) }, 0);
    }
    fn size(&self) -> (u16, u16) {
        let (mut r, mut c) = (0, 0);
        assert_eq!(unsafe { pty_size(self.ptr(), &mut r, &mut c) }, 0);
        (r, c)
    }
    fn stop(&mut self) {
        let result = unsafe { pty_stop(self.ptr()) };
        let s = self.stats();
        let stage = match s.cleanup_stage {
            0 => "none",
            1 => "close descriptor",
            2 => "waitpid child",
            3 => "SIGTERM child",
            4 => "SIGKILL child",
            5 => "read report",
            6 => "child cleanup deadline",
            _ => "invalid stage",
        };
        assert_eq!(
            result, 0,
            "PTY cleanup failed at {stage}, errno={}: {s:?}",
            s.cleanup_errno
        );
        assert_eq!(
            (s.open_fds, s.child_live, s.cleanup_errors, s.reaped),
            (0, 0, 0, 1)
        );
        assert_eq!(unsafe { pty_closed_fds(self.ptr()) }, 0);
    }
}
#[derive(Debug, PartialEq, Eq)]
enum Progress {
    Idle,
    Advanced,
    Staged,
    Committed,
    Blocked,
    Eof,
}
#[derive(Debug, PartialEq, Eq)]
enum Admission {
    Busy,
    Invalid,
    Failed,
    Closed,
}
struct PtyOwner {
    engine: MetadataOwner,
    transport: Transport,
    input: [u8; QUEUE],
    output: [u8; 512],
    start: usize,
    end: usize,
    staged: Option<Instant>,
    queued: Option<Instant>,
    closed: bool,
    failed: bool,
    timeout: bool,
    report: [bool; 256],
    max_pending: usize,
}
impl PtyOwner {
    fn new(python: &str, script: &str, mode: &str) -> Self {
        // All storage is fixed/reserved before the child can produce any output.
        let engine = MetadataOwner::new();
        let input = [0; QUEUE];
        let output = [0; 512];
        let report = [false; 256];
        let transport = Transport::new(python, script, mode);
        Self {
            engine,
            transport,
            input,
            output,
            start: 0,
            end: 0,
            staged: None,
            queued: None,
            closed: false,
            failed: false,
            timeout: false,
            report,
            max_pending: 0,
        }
    }
    fn reports(&mut self) {
        let mut bytes = [0; 128];
        let n = unsafe { pty_report(self.transport.ptr(), bytes.as_mut_ptr(), bytes.len()) };
        if n > 0 {
            for &b in &bytes[..n as usize] {
                assert_ne!(b, b'O', "child exact input order differs");
                assert!(
                    matches!(b, b'R' | b'A' | b'W' | b'V' | b'C'),
                    "unexpected child report"
                );
                self.report[b as usize] = true;
            }
        } else {
            assert!(matches!(n, 0 | -2 | -3), "child report I/O failed: {n}");
        }
    }
    fn ready(&mut self) {
        let end = Instant::now() + Duration::from_secs(2);
        while !self.report[b'R' as usize] {
            assert!(Instant::now() < end, "child readiness deadline");
            self.reports();
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn fail(&mut self) -> Result<Progress, Admission> {
        self.failed = true;
        self.engine.failed = true;
        unsafe { integrated_abort(self.engine.raw.ptr()) };
        self.transport.stop();
        Err(Admission::Failed)
    }
    fn cancel(&mut self) {
        if !self.failed {
            let _ = self.fail();
        }
    }
    fn user(&mut self, bytes: &[u8]) -> Result<(), Admission> {
        if self.failed {
            return Err(Admission::Failed);
        }
        if self.closed {
            return Err(Admission::Closed);
        }
        if self.staged.is_some() || self.start != self.end {
            return Err(Admission::Busy);
        }
        if bytes.len() > 4096 {
            return Err(Admission::Invalid);
        }
        self.input[..bytes.len()].copy_from_slice(bytes);
        self.start = 0;
        self.end = bytes.len();
        self.queued = if bytes.is_empty() {
            None
        } else {
            Some(Instant::now())
        };
        self.max_pending = self.max_pending.max(self.end);
        Ok(())
    }
    fn prepare(&mut self, status: i32) -> Result<(), Admission> {
        let e = &mut self.engine;
        if status != 0
            || unsafe {
                metadata_copy(
                    e.ptr.as_ptr(),
                    e.records.as_mut_ptr(),
                    size_of::<MetaRecord>(),
                    EVENTS,
                    &mut e.count,
                    e.bytes.as_mut_ptr(),
                    META_BYTES,
                    &mut e.length,
                )
            } != 0
            || e.candidate.export(&e.raw, EVENTS, REPLIES) != 0
            || e.validate().is_err()
        {
            self.fail()?;
            unreachable!();
        }
        self.staged = Some(Instant::now());
        self.queued = self.staged;
        let n = e.candidate.frame.reply_length;
        assert!(n <= REPLIES && n <= QUEUE);
        self.input[..n].copy_from_slice(&e.candidate.replies[..n]);
        self.start = 0;
        self.end = n;
        self.max_pending = self.max_pending.max(n);
        Ok(())
    }
    fn commit(&mut self) -> Result<Progress, Admission> {
        assert_eq!(self.start, self.end);
        let e = &mut self.engine;
        if unsafe { metadata_commit(e.ptr.as_ptr(), e.candidate.frame.operation_id) } != 0 {
            return self.fail();
        }
        std::mem::swap(&mut e.last, &mut e.candidate);
        std::mem::swap(&mut e.state, &mut e.next_state);
        self.staged = None;
        self.queued = None;
        self.start = 0;
        self.end = 0;
        Ok(Progress::Committed)
    }
    fn resize(&mut self, cols: u16, rows: u16) -> Result<(), Admission> {
        if self.failed {
            return Err(Admission::Failed);
        }
        if self.closed {
            return Err(Admission::Closed);
        }
        if self.staged.is_some() || self.start != self.end {
            return Err(Admission::Busy);
        }
        if cols == 0 || cols > 240 || rows == 0 || rows > 120 {
            return Err(Admission::Invalid);
        }
        let status = unsafe { integrated_resize(self.engine.raw.ptr(), cols, rows) };
        self.prepare(status)?;
        if unsafe { pty_resize(self.transport.ptr(), rows, cols) } != 0 {
            self.fail()?;
            unreachable!();
        }
        if self.start == self.end {
            self.commit()?;
        }
        Ok(())
    }
    fn pump(&mut self, write_cap: usize) -> Result<Progress, Admission> {
        if self.failed {
            return Err(Admission::Failed);
        }
        if self.closed {
            return Ok(Progress::Eof);
        }
        if self
            .queued
            .is_some_and(|began| began.elapsed() >= REPLY_DEADLINE)
        {
            self.timeout = true;
            return self.fail();
        }
        if self.start != self.end {
            let flags = unsafe { pty_poll(self.transport.ptr(), 1) };
            if flags < 0 || flags & 12 != 0 {
                return self.fail();
            }
            let length = (self.end - self.start).min(write_cap);
            assert!(length > 0 && length <= QUEUE);
            let n = unsafe {
                pty_write(
                    self.transport.ptr(),
                    self.input[self.start..].as_ptr(),
                    length,
                )
            };
            if n == -2 || n == -3 {
                return Ok(Progress::Blocked);
            }
            if n <= 0 || n as usize > length {
                return self.fail();
            }
            self.start += n as usize;
            if self.start == self.end {
                if self.staged.is_some() {
                    return self.commit();
                }
                self.start = 0;
                self.end = 0;
                self.queued = None;
            }
            return Ok(Progress::Advanced);
        }
        assert!(self.staged.is_none());
        // Queue reservation precedes reading irreversible PTY bytes. Its empty
        // fixed capacity always covers the accepted owner's 4096-byte reply cap.
        assert!(QUEUE >= REPLIES);
        let n = unsafe {
            pty_read(
                self.transport.ptr(),
                self.output.as_mut_ptr(),
                self.output.len(),
            )
        };
        if n == -2 || n == -3 {
            return Ok(Progress::Idle);
        }
        if n == 0 || n == -4 {
            self.closed = true;
            return Ok(Progress::Eof);
        }
        if n < 0 {
            return self.fail();
        }
        let status = self.engine.raw.feed(&self.output[..n as usize]);
        self.prepare(status)?;
        if self.start == self.end {
            self.commit()
        } else {
            Ok(Progress::Staged)
        }
    }
    fn close(&mut self) {
        self.transport.stop();
        self.engine.raw.close();
        assert_eq!(self.transport.stats().open_fds, 0);
    }
    fn wait_success(&mut self) {
        assert_eq!(unsafe { pty_wait(self.transport.ptr(), 500) }, 1);
        assert_eq!(
            self.transport.stats().exit_code,
            0,
            "child must validate the exact physical stream"
        );
        self.close();
    }
}
fn tick() {
    std::thread::sleep(Duration::from_millis(1));
}
fn boundary(p: &mut PtyOwner) -> (u64, u64, String) {
    (
        p.engine.last.frame.operation_id,
        p.engine.state.next,
        p.engine.state.title().to_owned(),
    )
}
fn assert_boundary(p: &mut PtyOwner, last: &(u64, u64, String)) {
    assert_eq!(
        p.engine.last.frame.operation_id, last.0,
        "blocked/failing replies must not publish candidate"
    );
    assert_eq!(p.engine.state.next, last.1);
    assert_eq!(p.engine.state.title(), last.2);
}
pub fn pty_main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3);
    let (python, script) = (&args[1], &args[2]);
    let mut basic = PtyOwner::new(python, script, "basic");
    basic.ready();
    assert_eq!(basic.user(&[0; 4097]), Err(Admission::Invalid));
    assert_eq!(basic.resize(0, 4), Err(Admission::Invalid));
    assert_eq!(basic.transport.size(), (3, 8));
    basic.user(PRE).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let (mut resized, mut posted) = (false, false);
    let mut partial_replies = 0;
    loop {
        assert!(Instant::now() < deadline, "basic exact reply deadline");
        basic.reports();
        if basic.report[b'V' as usize] {
            break;
        }
        if basic.report[b'A' as usize]
            && !resized
            && basic.staged.is_none()
            && basic.start == basic.end
        {
            basic.resize(12, 4).unwrap();
            assert_eq!(basic.transport.size(), (4, 12));
            assert_eq!(basic.engine.last.text().lines().count(), 4);
            basic.transport.control(b'Z');
            resized = true;
        }
        if basic.transport.stats().read_bytes as usize == QUERY1.len() + QUERY2.len()
            && basic.staged.is_none()
            && basic.start == basic.end
            && !posted
        {
            basic.user(POST).unwrap();
            posted = true;
        }
        let before = boundary(&mut basic);
        let written = basic.transport.stats().written_bytes;
        if basic.pump(3).is_err() {
            basic.reports();
            panic!("basic transport failure");
        }
        if basic.staged.is_some() && basic.start < basic.end {
            assert_boundary(&mut basic, &before);
            if basic.transport.stats().written_bytes > written {
                partial_replies += 1;
            }
        }
        tick();
    }
    assert!(
        partial_replies > 0,
        "actual partial reply writes must be observed"
    );
    assert!(resized && posted);
    assert_eq!(basic.engine.state.title(), "pty;title");
    assert_eq!(basic.engine.state.cwd(), Some("/pty;path"));
    assert_eq!(basic.engine.state.next, 2);
    assert_eq!(basic.engine.state.notice_text(1), ("title", "body;tail"));
    assert_eq!(
        basic.transport.stats().written_bytes as usize,
        PRE.len() + 6 + 4 + 6 + POST.len()
    );
    basic.wait_success();
    println!(
        "pty: actual child validates PRE/cursor/status/POST order, metadata and resized kernel geometry"
    );
    println!(
        "pty: partial_reply_writes={partial_replies} preserve publication before suffix completion"
    );
    drop(basic);
    for mode in ["flood", "stall"] {
        let mut p = PtyOwner::new(python, script, mode);
        p.ready();
        p.user(PRE).unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        let (mut blocked, mut resized, mut posted) = (false, false, false);
        loop {
            assert!(
                Instant::now() < deadline,
                "bounded backpressure journey deadline"
            );
            p.reports();
            if p.report[b'V' as usize] {
                break;
            }
            if blocked && !resized && p.staged.is_none() && p.start == p.end && mode == "flood" {
                p.resize(12, 4).unwrap();
                resized = true;
            }
            if p.transport.stats().read_bytes as usize == 4 * N
                && p.staged.is_none()
                && p.start == p.end
                && !posted
            {
                p.user(POST).unwrap();
                posted = true;
            }
            let before = boundary(&mut p);
            match p.pump(QUEUE) {
                Ok(Progress::Blocked) if p.transport.stats().eagain > 0 => {
                    assert_boundary(&mut p, &before);
                    assert_eq!(p.resize(12, 4), Err(Admission::Busy));
                    assert_eq!(p.user(b"must remain unadmitted"), Err(Admission::Busy));
                    if !blocked {
                        assert_eq!(p.transport.size(), (3, 8));
                        if mode == "flood" {
                            p.transport.control(b'G');
                        }
                    }
                    blocked = true;
                }
                Err(Admission::Failed) => {
                    assert_eq!(mode, "stall");
                    assert!(blocked && p.timeout);
                    assert_boundary(&mut p, &before);
                    break;
                }
                Ok(_) => {}
                Err(error) => panic!("unexpected admission: {error:?}"),
            }
            if p.staged.is_some() && p.start < p.end {
                assert_boundary(&mut p, &before);
            }
            tick();
        }
        let s = p.transport.stats();
        assert!(
            blocked && s.eagain > 0,
            "actual kernel EAGAIN must be observed"
        );
        assert!(p.max_pending <= QUEUE);
        if mode == "flood" {
            assert!(resized && posted);
            assert_eq!(s.written_bytes as usize, PRE.len() + 6 * N + POST.len());
            p.wait_success();
        } else {
            assert!(p.failed);
            let calls = p.transport.stats().write_calls;
            assert_eq!(p.pump(QUEUE), Err(Admission::Failed));
            assert_eq!(p.transport.stats().write_calls, calls);
            p.close();
        }
        println!(
            "pty: {mode} actual_eagain={} kernel_short_writes={} max_pending={} timeout={}",
            s.eagain, s.short_writes, p.max_pending, p.timeout
        );
    }
    for mode in ["cancel", "close", "resize-error", "engine-error"] {
        let mut p = PtyOwner::new(python, script, mode);
        p.ready();
        let deadline = Instant::now() + Duration::from_secs(3);
        while p.transport.stats().read_bytes < (SEED.len() as u64) || p.staged.is_some() {
            assert!(Instant::now() < deadline);
            p.pump(QUEUE).unwrap();
            tick();
        }
        assert_eq!(p.engine.state.title(), "last-good");
        let last = boundary(&mut p);
        p.transport.control(b'B');
        if mode == "resize-error" {
            assert_eq!(unsafe { pty_close_master(p.transport.ptr()) }, 0);
            assert_eq!(p.resize(12, 4), Err(Admission::Failed));
            assert_boundary(&mut p, &last);
        } else {
            loop {
                assert!(
                    Instant::now() < deadline,
                    "partial/failure journey deadline"
                );
                p.reports();
                if p.report[b'C' as usize] {
                    assert_eq!(
                        p.transport.stats().written_bytes,
                        3,
                        "child must consume only the confirmed prefix"
                    );
                    assert_boundary(&mut p, &last);
                    if mode == "cancel" {
                        p.cancel();
                        assert_eq!(p.transport.stats().exit_code, 137);
                    } else {
                        assert_eq!(unsafe { pty_wait(p.transport.ptr(), 500) }, 1);
                        assert_eq!(p.pump(3), Err(Admission::Failed));
                    }
                    break;
                }
                let before = boundary(&mut p);
                let written = p.transport.stats().written_bytes;
                let result = p.pump(3);
                if result == Err(Admission::Failed) {
                    assert_eq!(mode, "engine-error");
                    assert_boundary(&mut p, &before);
                    assert_eq!(p.transport.stats().written_bytes, written);
                    break;
                }
                // After one short syscall, wait for the child consumption receipt
                // before another write, preserving a real unpublished suffix.
                if p.transport.stats().written_bytes == 3 {
                    while !p.report[b'C' as usize] {
                        assert!(Instant::now() < deadline);
                        p.reports();
                        tick();
                    }
                }
                tick();
            }
        }
        assert!(p.failed);
        if mode != "engine-error" {
            assert_boundary(&mut p, &last);
        }
        p.close();
        println!(
            "pty: {mode} keeps last-good publication and closes/reaps physical owned resources"
        );
    }
    let reply_storage = 2 * REPLIES + 2 * REPLIES + QUEUE;
    println!(
        "pty_reply_reserved_capacities: probe_unused={} collector={} publication_slots={} input={} total={reply_storage}; included in fixed C/slot/inline accounting",
        REPLIES,
        REPLIES,
        2 * REPLIES,
        QUEUE
    );
    println!(
        "pty_success=true transport_c_fixed_bytes={} coordinator_inline_bytes={} fixed_input_bytes={} read_bytes={} reply_deadline_ms=250",
        unsafe { pty_fixed_bytes() },
        size_of::<PtyOwner>(),
        QUEUE,
        512
    );
}

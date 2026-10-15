// SPDX-License-Identifier: MIT
// Original real-engine effect/publication fixture; never writes a PTY or invokes UI.
use std::{ffi::c_void, marker::PhantomData, mem::size_of, ptr::NonNull, rc::Rc};
use zmux_terminal_adapter::TerminalNotice;
const REPLIES: usize = 4096;
const EVENTS: usize = 1024;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Event {
    kind: u32,
    offset: u32,
    length: u32,
}
unsafe extern "C" {
    fn effects_new(reply_limit: usize, event_limit: usize) -> *mut c_void;
    fn effects_free(p: *mut c_void);
    fn effects_error(p: *const c_void) -> i32;
    fn effects_feed(p: *mut c_void, bytes: *const u8, length: usize) -> i32;
    fn effects_resize(p: *mut c_void, cols: u16, rows: u16) -> i32;
    fn effects_snapshot(
        p: *mut c_void,
        bytes: *mut u8,
        capacity: usize,
        length: *mut usize,
        x: *mut u16,
        y: *mut u16,
    ) -> i32;
    fn effects_drain(
        p: *mut c_void,
        events: *mut Event,
        event_size: usize,
        event_capacity: usize,
        event_length: *mut usize,
        replies: *mut u8,
        reply_capacity: usize,
        reply_length: *mut usize,
    ) -> i32;
    fn effects_host_storage_bytes() -> usize;
}
struct Raw {
    ptr: NonNull<c_void>,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Raw {
    fn drop(&mut self) {
        unsafe { effects_free(self.ptr.as_ptr()) }
    }
}
impl Raw {
    fn new(replies: usize, events: usize) -> Self {
        Self {
            ptr: NonNull::new(unsafe { effects_new(replies, events) }).expect("effect owner"),
            _thread: PhantomData,
        }
    }
    fn feed(&mut self, bytes: &[u8]) -> i32 {
        unsafe { effects_feed(self.ptr.as_ptr(), bytes.as_ptr(), bytes.len()) }
    }
    fn error(&self) -> i32 {
        unsafe { effects_error(self.ptr.as_ptr()) }
    }
    fn drain(&mut self) -> Result<(Vec<Event>, Vec<u8>), i32> {
        let mut events = vec![Event::default(); EVENTS];
        let mut replies = vec![0; REPLIES];
        let (mut event_length, mut reply_length) = (0, 0);
        let result = unsafe {
            effects_drain(
                self.ptr.as_ptr(),
                events.as_mut_ptr(),
                size_of::<Event>(),
                events.len(),
                &mut event_length,
                replies.as_mut_ptr(),
                replies.len(),
                &mut reply_length,
            )
        };
        if result != 0 {
            return Err(result);
        }
        assert!(event_length <= EVENTS && reply_length <= REPLIES);
        events.truncate(event_length);
        replies.truncate(reply_length);
        Ok((events, replies))
    }
}
#[derive(Clone, Debug)]
struct Published {
    generation: u64,
    contents: String,
    cursor: (u16, u16),
    events: Vec<Event>,
    replies: Vec<u8>,
    notices: Vec<TerminalNotice>,
    notices_evicted: u64,
}
struct Owner {
    raw: Raw,
    last_good: Option<Published>,
    next_notice: u64,
    failure: Option<i32>,
}
impl Owner {
    fn new(replies: usize, events: usize) -> Self {
        Self {
            raw: Raw::new(replies, events),
            last_good: None,
            next_notice: 0,
            failure: None,
        }
    }
    fn feed(&mut self, bytes: &[u8]) -> Result<Published, i32> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let result = self.raw.feed(bytes);
        self.finish(result)
    }
    fn resize(&mut self, cols: u16, rows: u16) -> Result<Published, i32> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let result = unsafe { effects_resize(self.raw.ptr.as_ptr(), cols, rows) };
        self.finish(result)
    }
    fn finish(&mut self, result: i32) -> Result<Published, i32> {
        let candidate = if result == 0 {
            self.candidate()
        } else {
            Err(match self.raw.error() {
                0 => result,
                error => error,
            })
        };
        match candidate {
            Ok((frame, sequence)) => {
                self.next_notice = sequence;
                self.last_good = Some(frame.clone());
                Ok(frame)
            }
            Err(error) => {
                self.failure = Some(error);
                Err(error)
            }
        }
    }
    fn candidate(&mut self) -> Result<(Published, u64), i32> {
        let mut bytes = vec![0; 512 * 1024];
        let (mut length, mut x, mut y) = (0, 0, 0);
        if unsafe {
            effects_snapshot(
                self.raw.ptr.as_ptr(),
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut length,
                &mut x,
                &mut y,
            )
        } != 0
        {
            return Err(self.raw.error());
        }
        if length > bytes.len() {
            return Err(100);
        }
        bytes.truncate(length);
        let text = String::from_utf8(bytes).map_err(|_| 101)?;
        let contents = text
            .lines()
            .map(|line| line.trim_end_matches(' '))
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end_matches('\n')
            .to_owned();
        let (events, replies) = self.raw.drain().map_err(|_| 102)?;
        let mut notices = self
            .last_good
            .as_ref()
            .map_or_else(Vec::new, |old| old.notices.clone());
        let mut evicted = self.last_good.as_ref().map_or(0, |old| old.notices_evicted);
        let mut sequence = self.next_notice;
        let mut reply_offset = 0;
        for event in &events {
            match event.kind {
                1 if event.offset == 0 && event.length == 0 => {
                    sequence = sequence.checked_add(1).ok_or(103)?;
                    if notices.len() == 32 {
                        notices.remove(0);
                        evicted = evicted.checked_add(1).ok_or(104)?;
                    }
                    notices.push(TerminalNotice {
                        sequence,
                        source: "bell".into(),
                        title: "Terminal needs attention".into(),
                        body: String::new(),
                    });
                }
                2 if event.offset as usize == reply_offset => {
                    reply_offset = reply_offset.checked_add(event.length as usize).ok_or(105)?;
                    if reply_offset > replies.len() {
                        return Err(106);
                    }
                }
                3 if event.offset == 0 && event.length == 0 => {} // Reset is ordered, not a notice or sequence restart.
                _ => return Err(107),
            }
        }
        if reply_offset != replies.len() {
            return Err(108);
        }
        let generation = self
            .last_good
            .as_ref()
            .map_or(Ok(1), |old| old.generation.checked_add(1).ok_or(109))?;
        Ok((
            Published {
                generation,
                contents,
                cursor: (x, y),
                events,
                replies,
                notices,
                notices_evicted: evicted,
            },
            sequence,
        ))
    }
}
fn last_good_is_retained(owner: &mut Owner, old: &Published, error: i32) {
    assert_eq!(owner.failure, Some(error));
    let last = owner.last_good.as_ref().unwrap();
    assert_eq!(last.generation, old.generation);
    assert_eq!(last.contents, old.contents);
    assert_eq!(last.notices.len(), old.notices.len());
    assert_eq!(last.replies, old.replies);
    assert_eq!(owner.feed(b"must not replay or consume").err(), Some(error));
    assert_eq!(owner.resize(4, 2).err(), Some(error));
}
fn main() {
    let mut owner = Owner::new(REPLIES, EVENTS);
    let mixed = owner.feed(b"\x07\x1b[6n\x1bc\x07").unwrap();
    assert_eq!(
        mixed.events,
        [
            Event {
                kind: 1,
                offset: 0,
                length: 0
            },
            Event {
                kind: 2,
                offset: 0,
                length: 6
            },
            Event {
                kind: 3,
                offset: 0,
                length: 0
            },
            Event {
                kind: 1,
                offset: 0,
                length: 0
            }
        ],
        "callback order must preserve BEL/reply/RESET"
    );
    assert_eq!(mixed.replies, b"\x1b[1;1R");
    assert_eq!(
        mixed.notices.iter().map(|n| n.sequence).collect::<Vec<_>>(),
        [1, 2]
    );
    assert!(
        mixed.notices.iter().all(|n| n.source == "bell"
            && n.title == "Terminal needs attention"
            && n.body.is_empty())
    );
    owner.feed(b"\x1b[").unwrap();
    assert_eq!(owner.feed(b"6n").unwrap().replies, b"\x1b[1;1R");
    let text = owner.feed(b"owned").unwrap();
    assert_eq!(text.contents, "owned");
    assert_eq!(text.cursor, (5, 0));
    owner.resize(4, 2).unwrap();
    owner.feed(b"later").unwrap();
    let ring = owner.feed(&[7; 40]).unwrap();
    assert_eq!(ring.notices.len(), 32);
    assert_eq!(
        (
            ring.notices.first().unwrap().sequence,
            ring.notices.last().unwrap().sequence,
            ring.notices_evicted
        ),
        (11, 42, 10)
    );
    drop(owner);
    assert_eq!(mixed.replies, b"\x1b[1;1R");
    assert_eq!(text.contents, "owned");
    println!(
        "effects: ordered BEL/reply/RESET, fragmented query, notice sequence/eviction and owned frame lifetime pass"
    );

    let mut raw = Raw::new(REPLIES, EVENTS);
    assert_eq!(raw.feed(b"\x1b[6n"), 0);
    assert_eq!(raw.feed(b"not consumed"), -3);
    let mut events = [Event {
        kind: 99,
        offset: 99,
        length: 99,
    }; 1];
    let mut bytes = [99; 6];
    let (mut n, mut m) = (99, 99);
    let ptr = raw.ptr.as_ptr();
    assert_eq!(
        unsafe {
            effects_drain(
                ptr,
                events.as_mut_ptr(),
                size_of::<Event>(),
                1,
                &mut n,
                bytes.as_mut_ptr(),
                5,
                &mut m,
            )
        },
        -2
    );
    assert_eq!((events[0].kind, bytes, n, m), (99, [99; 6], 99, 99));
    assert_eq!(
        unsafe {
            effects_drain(
                ptr,
                events.as_mut_ptr(),
                size_of::<Event>(),
                0,
                &mut n,
                bytes.as_mut_ptr(),
                6,
                &mut m,
            )
        },
        -2
    );
    assert_eq!((events[0].kind, bytes, n, m), (99, [99; 6], 99, 99));
    assert_eq!(
        unsafe {
            effects_drain(
                ptr,
                events.as_mut_ptr(),
                size_of::<Event>() - 1,
                1,
                &mut n,
                bytes.as_mut_ptr(),
                6,
                &mut m,
            )
        },
        -2
    );
    let (events, bytes) = raw.drain().unwrap();
    assert_eq!(
        (events.len(), bytes.as_slice()),
        (1, b"\x1b[1;1R".as_slice())
    );
    assert!(raw.drain().unwrap().0.is_empty());
    assert_eq!(raw.feed(&vec![b'x'; 4097]), -2);
    assert_eq!(raw.feed(b"X"), 0);
    let mut snapshot = [0; 128];
    let (mut length, mut x, mut y) = (0, 0, 0);
    assert_eq!(
        unsafe {
            effects_snapshot(
                ptr,
                snapshot.as_mut_ptr(),
                snapshot.len(),
                &mut length,
                &mut x,
                &mut y,
            )
        },
        0
    );
    assert_eq!(
        x, 1,
        "undrained and oversized feed rejection must precede mutation"
    );
    assert!(snapshot[..length].starts_with(b"X"));
    println!(
        "effects: pending/oversize input and insufficient/ABI-mismatched drains do not consume or partially copy"
    );

    let mut owner = Owner::new(6, EVENTS);
    let good = owner.feed(b"\x1b[6n").unwrap(); // Exactly six reply bytes.
    owner.resize(12, 3).unwrap();
    let good = owner.last_good.clone().unwrap_or(good);
    assert_eq!(
        owner.feed(b"123456789\x1b[6n").err(),
        Some(1),
        "reply limit+1 must fail visibly"
    );
    last_good_is_retained(&mut owner, &good, 1);
    let mut owner = Owner::new(REPLIES, EVENTS);
    let mut exact = b"\x1b[6n".repeat(682);
    exact.extend_from_slice(b"\x1b[5n");
    let good = owner.feed(&exact).unwrap();
    let mut expected = b"\x1b[1;1R".repeat(682);
    expected.extend_from_slice(b"\x1b[0n");
    assert_eq!(good.replies, expected);
    assert_eq!(
        good.replies.len(),
        REPLIES,
        "exact 4096-byte reply batch must succeed"
    );
    let query_flood = b"\x1b[6n".repeat(683);
    assert_eq!(
        owner.feed(&query_flood).err(),
        Some(1),
        "4096-byte reply ceiling must fail visibly"
    );
    last_good_is_retained(&mut owner, &good, 1);
    let mut owner = Owner::new(REPLIES, EVENTS);
    let good = owner.feed(&[7; EVENTS]).unwrap();
    assert_eq!(good.events.len(), EVENTS);
    assert_eq!(
        owner.feed(&[7; EVENTS + 1]).err(),
        Some(2),
        "event limit+1 must fail visibly"
    );
    last_good_is_retained(&mut owner, &good, 2);
    let mut owner = Owner::new(REPLIES, EVENTS);
    let good = owner.feed(b"last good").unwrap();
    let oversized = format!("\x1b[2J\x1b[Ha{}\x07", "\u{1d185}".repeat(64));
    assert_eq!(
        owner.feed(oversized.as_bytes()).err(),
        Some(4),
        "failed snapshot must not publish effects"
    );
    last_good_is_retained(&mut owner, &good, 4);
    for _ in 0..128 {
        let mut owner = Owner::new(REPLIES, EVENTS);
        assert_eq!(owner.feed(b"\x07\x1b[6n").unwrap().replies, b"\x1b[1;1R");
    }
    println!(
        "effects: exact reply/event limits, sticky overflow, last-good publication and128 owner teardowns pass"
    );
    println!(
        "effects_host_storage_bytes={}; callback_reply_bytes={REPLIES}; callback_event_count={EVENTS}; total_app_memory_bound=false; real_pty_round_trip=false; title_cwd_osc_notifications=false; effects_success=true",
        unsafe { effects_host_storage_bytes() }
    );
}

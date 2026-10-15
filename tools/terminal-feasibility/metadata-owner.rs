// SPDX-License-Identifier: MIT
// Original bounded metadata normalization/publication on the existing Integrated.
const META_BYTES: usize = 20480;
const TITLE_BYTES: usize = 1024;
const CWD_BYTES: usize = 4096;
const NOTICE_TITLE: usize = 512;
const NOTICE_BODY: usize = 8192;
const NOTICE_BYTES: usize = NOTICE_TITLE + NOTICE_BODY;
const META_STATE: usize = TITLE_BYTES + CWD_BYTES + 32 * NOTICE_BYTES;
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct MetaRecord {
    source: u32,
    subcommand: u32,
    status: u32,
    terminator: u32,
    offset: u32,
    length: u32,
}
unsafe extern "C" {
    fn metadata_new(cap: usize, bytes: usize, events: usize, deny: usize) -> *mut c_void;
    fn metadata_integrated(p: *mut c_void) -> *mut c_void;
    fn metadata_failure(p: *mut c_void) -> u32;
    fn metadata_copy(
        p: *mut c_void,
        records: *mut MetaRecord,
        size: usize,
        capacity: usize,
        count: *mut usize,
        bytes: *mut u8,
        byte_capacity: usize,
        length: *mut usize,
    ) -> i32;
    fn metadata_commit(p: *mut c_void, operation: u64) -> i32;
    fn metadata_free(p: *mut c_void);
}
#[derive(Clone, Copy, Default)]
struct Notice {
    sequence: u64,
    source: u32,
    title: usize,
    body: usize,
}
struct MetaState {
    data: Vec<u8>,
    title: usize,
    cwd: Option<usize>,
    notices: [Notice; 32],
    count: usize,
    next: u64,
    evicted: u64,
}
fn display_into(out: &mut [u8], mut bytes: &[u8], limit: usize) -> usize {
    let mut length = 0;
    let mut count = 0;
    let mut emit = |c: char| {
        if count == limit
            || c.is_control()
            || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            return;
        }
        let mut encoded = [0; 4];
        let text = c.encode_utf8(&mut encoded);
        out[length..length + text.len()].copy_from_slice(text.as_bytes());
        length += text.len();
        count += 1;
    };
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                for c in text.chars() {
                    emit(c);
                }
                break;
            }
            Err(e) => {
                for c in std::str::from_utf8(&bytes[..e.valid_up_to()])
                    .unwrap()
                    .chars()
                {
                    emit(c);
                }
                emit('\u{fffd}');
                let consumed =
                    e.valid_up_to() + e.error_len().unwrap_or(bytes.len() - e.valid_up_to());
                bytes = &bytes[consumed..];
            }
        }
    }
    length
}
impl MetaState {
    fn new() -> Result<Self, HostError> {
        Ok(Self {
            data: buffer(META_STATE).map_err(|_| HostError::Allocation)?,
            title: 0,
            cwd: None,
            notices: [Notice::default(); 32],
            count: 0,
            next: 0,
            evicted: 0,
        })
    }
    fn copy_from(&mut self, last: &Self) {
        self.title = last.title;
        self.cwd = last.cwd;
        self.notices = last.notices;
        self.count = last.count;
        self.next = last.next;
        self.evicted = last.evicted;
        self.data[..last.title].copy_from_slice(&last.data[..last.title]);
        if let Some(n) = last.cwd {
            self.data[TITLE_BYTES..TITLE_BYTES + n]
                .copy_from_slice(&last.data[TITLE_BYTES..TITLE_BYTES + n]);
        }
        for (i, notice) in last.notices[..last.count].iter().enumerate() {
            let start = TITLE_BYTES + CWD_BYTES + i * NOTICE_BYTES;
            self.data[start..start + notice.title]
                .copy_from_slice(&last.data[start..start + notice.title]);
            self.data[start + NOTICE_TITLE..start + NOTICE_TITLE + notice.body].copy_from_slice(
                &last.data[start + NOTICE_TITLE..start + NOTICE_TITLE + notice.body],
            );
        }
    }
    fn title(&self) -> &str {
        std::str::from_utf8(&self.data[..self.title]).unwrap()
    }
    fn cwd(&self) -> Option<&str> {
        self.cwd
            .map(|n| std::str::from_utf8(&self.data[TITLE_BYTES..TITLE_BYTES + n]).unwrap())
    }
    fn notice_text(&self, i: usize) -> (&str, &str) {
        let n = self.notices[i];
        let start = TITLE_BYTES + CWD_BYTES + i * NOTICE_BYTES;
        (
            std::str::from_utf8(&self.data[start..start + n.title]).unwrap(),
            std::str::from_utf8(&self.data[start + NOTICE_TITLE..start + NOTICE_TITLE + n.body])
                .unwrap(),
        )
    }
    fn notice(&mut self, source: u32, title: &[u8], body: &[u8]) -> Result<(), ()> {
        // Filter first into fixed scratch; suppress empty titles before consuming sequence.
        let mut normalized = [0; NOTICE_TITLE];
        let title_length = display_into(&mut normalized, title, 128);
        if title_length == 0 {
            return Ok(());
        }
        let sequence = self.next.checked_add(1).ok_or(())?;
        if self.count == 32 {
            self.evicted = self.evicted.checked_add(1).ok_or(())?;
            self.notices.copy_within(1..32, 0);
            let start = TITLE_BYTES + CWD_BYTES;
            self.data
                .copy_within(start + NOTICE_BYTES..META_STATE, start);
            self.count = 31;
        }
        let start = TITLE_BYTES + CWD_BYTES + self.count * NOTICE_BYTES;
        self.data[start..start + title_length].copy_from_slice(&normalized[..title_length]);
        let body_length = display_into(
            &mut self.data[start + NOTICE_TITLE..start + NOTICE_BYTES],
            body,
            2048,
        );
        self.notices[self.count] = Notice {
            sequence,
            source,
            title: title_length,
            body: body_length,
        };
        self.count += 1;
        self.next = sequence;
        Ok(())
    }
    fn set_cwd(&mut self, bytes: &[u8]) {
        let Ok(uri) = std::str::from_utf8(bytes) else {
            return;
        };
        let Some(authority) = uri.strip_prefix("file://") else {
            return;
        };
        let Some(start) = authority.find('/') else {
            return;
        };
        let path = &authority.as_bytes()[start..];
        let mut n = 0;
        let mut i = 0;
        let dest = &mut self.data[TITLE_BYTES..TITLE_BYTES + CWD_BYTES];
        while i < path.len() {
            if n == CWD_BYTES {
                self.cwd = None;
                return;
            }
            if path[i] == b'%'
                && i + 2 < path.len()
                && let (Some(a), Some(b)) = (
                    (path[i + 1] as char).to_digit(16),
                    (path[i + 2] as char).to_digit(16),
                )
            {
                dest[n] = (a * 16 + b) as u8;
                i += 3;
            } else {
                dest[n] = path[i];
                i += 1;
            }
            n += 1;
        }
        self.cwd = std::str::from_utf8(&dest[..n])
            .ok()
            .filter(|p| p.starts_with('/') && !p.chars().any(char::is_control))
            .map(|_| n);
    }
    fn apply(&mut self, record: MetaRecord, bytes: &[u8]) -> Result<(), ()> {
        if record.status > 2 {
            return Err(());
        }
        if record.status != 0 {
            return Ok(());
        }
        match record.source {
            0 | 2 => {
                self.title = display_into(&mut self.data[..TITLE_BYTES], bytes, 256);
            }
            7 => self.set_cwd(bytes),
            9 if record.subcommand == 0 => self.notice(9, b"Terminal notification", bytes)?,
            9 if record.subcommand <= 12 => {} // classified ConEmu, never an executable host request
            777 => {
                let value = bytes.strip_prefix(b"notify;").ok_or(())?;
                let split = value.iter().position(|b| *b == b';').ok_or(())?;
                self.notice(777, &value[..split], &value[split + 1..])?;
            }
            1337 if record.subcommand == 9 => {} // compatibility cwd policy
            _ => return Err(()),
        }
        Ok(())
    }
}
struct MetadataOwner {
    ptr: NonNull<c_void>,
    raw: std::mem::ManuallyDrop<Raw>, // Borrowed Integrated; metadata_free is its only destructor.
    last: Slot,
    candidate: Slot,
    state: MetaState,
    next_state: MetaState,
    records: Vec<MetaRecord>,
    bytes: Vec<u8>,
    count: usize,
    length: usize,
    failed: bool,
}
impl Drop for MetadataOwner {
    fn drop(&mut self) {
        unsafe { metadata_free(self.ptr.as_ptr()) }
    }
}
impl MetadataOwner {
    fn try_new(cap: usize, bytes: usize, events: usize, budget: usize) -> Result<Self, HostError> {
        let minimum = 2 * (TEXT + EVENTS * size_of::<Event>() + REPLIES + META_STATE)
            + EVENTS * size_of::<MetaRecord>()
            + META_BYTES
            + size_of::<Self>();
        if budget < minimum {
            return Err(HostError::Budget);
        }
        let last = Slot::new().map_err(|_| HostError::Allocation)?;
        let candidate = Slot::new().map_err(|_| HostError::Allocation)?;
        let state = MetaState::new()?;
        let next_state = MetaState::new()?;
        let records: Vec<MetaRecord> = buffer(EVENTS).map_err(|_| HostError::Allocation)?;
        let raw_bytes: Vec<u8> = buffer(META_BYTES).map_err(|_| HostError::Allocation)?;
        let actual = last.capacity_bytes()
            + candidate.capacity_bytes()
            + state.data.capacity()
            + next_state.data.capacity()
            + records.capacity() * size_of::<MetaRecord>()
            + raw_bytes.capacity()
            + size_of::<Self>();
        if actual > budget {
            return Err(HostError::Budget);
        }
        let ptr = NonNull::new(unsafe { metadata_new(cap, bytes, events, 0) })
            .ok_or(HostError::EngineConstruction)?;
        let raw = std::mem::ManuallyDrop::new(Raw {
            ptr: NonNull::new(unsafe { metadata_integrated(ptr.as_ptr()) }).unwrap(),
            _thread: PhantomData,
        });
        if raw.stats().phase != 0 {
            unsafe { metadata_free(ptr.as_ptr()) };
            return Err(HostError::EngineConstruction);
        }
        Ok(Self {
            ptr,
            raw,
            last,
            candidate,
            state,
            next_state,
            records,
            bytes: raw_bytes,
            count: 0,
            length: 0,
            failed: false,
        })
    }
    fn new() -> Self {
        Self::try_new(16384, META_BYTES, EVENTS, 2 * 1024 * 1024).unwrap()
    }
    fn fail(&mut self) -> Result<(), ()> {
        self.failed = true;
        unsafe { integrated_abort(self.raw.ptr()) };
        Err(())
    }
    fn finish(&mut self, status: i32) -> Result<(), ()> {
        if status != 0
            || unsafe {
                metadata_copy(
                    self.ptr.as_ptr(),
                    self.records.as_mut_ptr(),
                    size_of::<MetaRecord>(),
                    EVENTS,
                    &mut self.count,
                    self.bytes.as_mut_ptr(),
                    META_BYTES,
                    &mut self.length,
                )
            } != 0
            || self.candidate.export(&self.raw, EVENTS, REPLIES) != 0
        {
            return self.fail();
        }
        if self.validate().is_err() {
            return self.fail();
        }
        if unsafe { metadata_commit(self.ptr.as_ptr(), self.candidate.frame.operation_id) } != 0 {
            return self.fail();
        }
        std::mem::swap(&mut self.last, &mut self.candidate);
        std::mem::swap(&mut self.state, &mut self.next_state);
        Ok(())
    }
    fn validate(&mut self) -> Result<(), ()> {
        let frame = self.candidate.frame;
        if frame.text_length > TEXT
            || frame.event_length > EVENTS
            || frame.reply_length > REPLIES
            || frame.operation_id <= self.last.frame.operation_id
            || self.count > EVENTS
            || self.length > META_BYTES
            || std::str::from_utf8(&self.candidate.text[..frame.text_length]).is_err()
        {
            return Err(());
        }
        self.next_state.copy_from(&self.state);
        let mut reply = 0usize;
        let mut metadata = 0usize;
        let mut bytes = 0usize;
        for e in &self.candidate.events[..frame.event_length] {
            match e.kind {
                1 if e.offset == 0 && e.length == 0 => {
                    self.next_state
                        .notice(1, b"Terminal needs attention", b"")?
                }
                2 if e.offset as usize == reply => {
                    reply = reply.checked_add(e.length as usize).ok_or(())?;
                    if reply > frame.reply_length {
                        return Err(());
                    }
                }
                3 if e.offset == 0 && e.length == 0 => {} // Approved migration RIS retention.
                4 if e.offset as usize == metadata && e.length == 0 && metadata < self.count => {
                    let record = self.records[metadata];
                    if record.offset as usize != bytes {
                        return Err(());
                    }
                    bytes = bytes.checked_add(record.length as usize).ok_or(())?;
                    if bytes > self.length {
                        return Err(());
                    }
                    self.next_state
                        .apply(record, &self.bytes[record.offset as usize..bytes])?;
                    metadata += 1;
                }
                _ => return Err(()),
            }
        }
        if reply != frame.reply_length || metadata != self.count || bytes != self.length {
            return Err(());
        }
        Ok(())
    }
    fn feed(&mut self, bytes: &[u8]) -> Result<(), ()> {
        if self.failed {
            return Err(());
        }
        let status = self.raw.feed(bytes);
        self.finish(status)
    }
    fn fragmented(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.feed(&[*byte]).unwrap();
        }
    }
    fn streamed(&mut self, bytes: &[u8]) -> Result<(), ()> {
        for chunk in bytes.chunks(4096) {
            self.feed(chunk)?;
        }
        Ok(())
    }
}
pub fn metadata_main() {
    for (cap, bytes, events) in [
        (0, META_BYTES, EVENTS),
        (16385, META_BYTES, EVENTS),
        (16384, META_BYTES + 1, EVENTS),
        (16384, META_BYTES, EVENTS + 1),
    ] {
        assert!(unsafe { metadata_new(cap, bytes, events, 0) }.is_null());
    }
    assert!(matches!(
        MetadataOwner::try_new(16384, META_BYTES, EVENTS, 0),
        Err(HostError::Budget)
    ));
    let mut o = MetadataOwner::new();
    o.fragmented(b"\x1b]0;one;two\x1b\\\x1b]7;file://localhost/a%20b\x07");
    assert_eq!(
        o.state.title(),
        "one;two",
        "full semicolon title must survive"
    );
    assert_eq!(o.state.cwd(), Some("/a b"));
    o.feed(b"\x1b]9;body;extra\x07\x1b]777;notify;title;body;extra\x07\x07\x1b[6n\x1bc")
        .unwrap();
    assert_eq!(
        o.last.events[..o.last.frame.event_length]
            .iter()
            .map(|e| e.kind)
            .collect::<Vec<_>>(),
        [4, 4, 1, 2, 3]
    );
    assert_eq!(
        o.state.notices[..3]
            .iter()
            .map(|n| n.source)
            .collect::<Vec<_>>(),
        [9, 777, 1],
        "notification source must remain exact"
    );
    assert_eq!(
        o.state.notice_text(0),
        ("Terminal notification", "body;extra")
    );
    assert_eq!(o.state.notice_text(1), ("title", "body;extra"));
    assert_eq!(o.state.title(), "one;two");
    assert_eq!(o.state.cwd(), Some("/a b"));
    assert_eq!(o.last.replies(), b"\x1b[1;1R");
    o.feed(b"\x1b]0;zero\x07\x1b]2;one;two\x07").unwrap();
    assert_eq!(
        o.records[..o.count]
            .iter()
            .map(|r| (r.source, r.terminator))
            .collect::<Vec<_>>(),
        [(0, 7), (2, 7)]
    );
    o.feed(b"\x1b]2\x07\x1b]777\x07").unwrap();
    assert_eq!(
        o.records[..o.count]
            .iter()
            .map(|r| r.status)
            .collect::<Vec<_>>(),
        [2, 2]
    );
    assert_eq!(o.state.title(), "one;two");
    let sequence = o.state.next;
    o.feed(
        b"\x1b[!p\x1b]2;cancelled\x18\x1b]2;subbed\x1a\x1b]777;notify;;body\x07\x1b]777;bad\x07",
    )
    .unwrap();
    assert_eq!(o.state.title(), "one;two");
    assert_eq!(o.state.next, sequence);
    o.feed(b"\x1b]9;9;/untrusted\x07\x1b]9;7;touch /never-run\x07\x1b]9;4;1;22\x07\x1b]1337;CurrentDir=/other\x07").unwrap();
    assert_eq!(o.state.next, sequence);
    assert_eq!(o.state.cwd(), Some("/a b"));
    assert_eq!(
        &o.records[..o.count]
            .iter()
            .map(|r| (r.source, r.subcommand))
            .collect::<Vec<_>>(),
        &[(9, 9), (9, 7), (9, 4), (1337, 9)]
    );
    for n in [2047, 2048] {
        let text = format!("\x1b]2;{}\x07", "x".repeat(n));
        o.streamed(text.as_bytes()).unwrap();
        assert_eq!(o.state.title(), "x".repeat(256));
    }
    let bidi = format!("\x1b]2;{}{}\x07", "\u{202e}".repeat(400), "x".repeat(256));
    o.fragmented(bidi.as_bytes());
    assert_eq!(o.state.title(), "x".repeat(256));
    o.feed(b"\x1b]2;a\xffb\x07").unwrap();
    assert_eq!(o.state.title(), "a\u{fffd}b");
    o.feed(b"\x1b]7;\x07\x1b]7;https://wrong/path\x07").unwrap();
    assert_eq!(o.state.cwd(), Some("/a b"));
    o.feed(b"\x1b]7;file://host/a%2520%QZ;tail\x07").unwrap();
    assert_eq!(o.state.cwd(), Some("/a%20%QZ;tail"));
    let cwd = format!("\x1b]7;file://host/{}\x07", "%61".repeat(4095));
    o.streamed(cwd.as_bytes()).unwrap();
    assert_eq!(o.state.cwd().unwrap().len(), 4096);
    o.streamed(format!("\x1b]7;file://host/{}\x07", "a".repeat(4096)).as_bytes())
        .unwrap();
    assert_eq!(o.state.cwd(), None);
    o.feed(b"\x1b]7;file://host/%00\x07").unwrap();
    assert_eq!(o.state.cwd(), None);
    for _ in 0..33 {
        o.feed(b"\x07").unwrap();
    }
    assert_eq!(o.state.count, 32);
    assert_eq!(o.state.evicted, 4);
    assert_eq!(o.state.notices[31].sequence, o.state.next);
    let capacity = o.last.capacity_bytes()
        + o.candidate.capacity_bytes()
        + o.state.data.capacity()
        + o.next_state.data.capacity()
        + o.bytes.capacity()
        + o.records.capacity() * size_of::<MetaRecord>();
    println!(
        "metadata: semicolon/title/body, exact source/subcommand, abort, RIS/DECSTR, UTF8/bidi, exact cwd/ring pass; rust_capacity_bytes={capacity}; inline_bytes={}; c_fixed_bytes={}",
        size_of::<MetadataOwner>(),
        o.raw.stats().fixed_host
    );
    o.raw.close();
    // Full-string cap includes command digits plus its first delimiter.
    for n in [16383usize, 16384] {
        let mut o = MetadataOwner::new();
        o.streamed(format!("\x1b]2;{}\x07", "x".repeat(n - 2)).as_bytes())
            .unwrap();
        assert_eq!(o.state.title().len(), 256);
        o.raw.close();
    }
    let mut failed = MetadataOwner::new();
    failed.feed(b"\x1b]2;last-good\x07").unwrap();
    let generation = failed.last.frame.operation_id;
    assert!(
        failed
            .streamed(format!("\x1b]2;{}", "x".repeat(16383)).as_bytes())
            .is_err(),
        "unterminated metadata overflow must fail visibly"
    );
    assert_eq!(unsafe { metadata_failure(failed.ptr.as_ptr()) }, 19);
    assert_eq!(failed.state.title(), "last-good");
    assert!(failed.last.frame.operation_id >= generation);
    let last = failed.last.frame.operation_id;
    assert!(failed.feed(b"\x07\x1bc").is_err());
    assert_eq!(failed.last.frame.operation_id, last);
    failed.raw.close();
    // Small arena fails after a valid callback prefix in the same operation.
    let mut arena = MetadataOwner::try_new(16384, 3, EVENTS, 2 * 1024 * 1024).unwrap();
    arena.feed(b"\x1b]2;ok\x07").unwrap();
    let generation = arena.last.frame.operation_id;
    assert!(
        arena.feed(b"\x07\x1b]2;too-long\x07").is_err(),
        "metadata arena overflow must fence publication"
    );
    assert_eq!(arena.state.title(), "ok");
    assert_eq!(arena.state.next, 0);
    assert_eq!(arena.last.frame.operation_id, generation);
    arena.raw.close();
    // Real capture allocation denial after a valid BEL/title prefix.
    let mut quota = MetadataOwner::new();
    quota.feed(b"\x1b]2;last\x07\x1b]1234;ignored\x07").unwrap();
    let s = quota.raw.stats();
    quota.raw.limits(
        s.managed_current as usize,
        s.engine.general as usize,
        s.engine.pages as usize,
    );
    let generation = quota.last.frame.operation_id;
    assert!(
        quota.feed(b"\x07\x1b]2;new\x07").is_err(),
        "metadata capture allocation denial must fence publication"
    );
    assert!(quota.raw.stats().engine.denials > 0);
    assert_eq!(unsafe { metadata_failure(quota.ptr.as_ptr()) }, 20);
    assert_eq!(quota.state.title(), "last");
    assert_eq!(quota.state.next, 0);
    assert_eq!(quota.last.frame.operation_id, generation);
    quota.raw.close();
    let mut unicode = MetadataOwner::new();
    let notification = format!(
        "\x1b]777;notify;{};{}\x07",
        "😀".repeat(129),
        "😀".repeat(2049)
    );
    unicode.streamed(notification.as_bytes()).unwrap();
    assert_eq!(unicode.state.notices[0].title, 512);
    assert_eq!(unicode.state.notices[0].body, 8192);
    assert_eq!(unicode.state.notice_text(0).0.chars().count(), 128);
    assert_eq!(unicode.state.notice_text(0).1.chars().count(), 2048);
    unicode.raw.close();
    let mut render = MetadataOwner::new();
    render
        .feed(b"\x1b]2;render-last\x07\x1b]9;kept\x07")
        .unwrap();
    let generation = render.last.frame.operation_id;
    assert_eq!(unsafe { integrated_resize(render.raw.ptr(), 240, 120) }, 0);
    let before = render.raw.stats();
    render.raw.limits(
        before.managed_limit as usize,
        before.engine.general as usize,
        ENGINE_LIMIT,
    );
    assert!(
        render.finish(0).is_err(),
        "metadata post-render denial must fence publication"
    );
    assert!(render.raw.stats().engine.denials > before.engine.denials);
    assert_eq!(render.state.title(), "render-last");
    assert_eq!(render.state.next, 1);
    assert_eq!(render.last.frame.operation_id, generation);
    render.raw.close();
    let mut snapshot = MetadataOwner::new();
    snapshot.feed(b"\x1b]2;snapshot-last\x07").unwrap();
    let generation = snapshot.last.frame.operation_id;
    let input = format!(
        "\x1b]2;candidate\x07\x1b[2J\x1b[Ha{}",
        "\u{1d185}".repeat(64)
    );
    assert!(snapshot.feed(input.as_bytes()).is_err());
    assert_eq!(snapshot.state.title(), "snapshot-last");
    assert_eq!(snapshot.last.frame.operation_id, generation);
    snapshot.raw.close();
    let mut event = MetadataOwner::try_new(16384, META_BYTES, 1, 2 * 1024 * 1024).unwrap();
    assert!(event.feed(b"\x07\x1b]2;candidate\x07").is_err());
    assert_eq!(event.state.next, 0);
    assert_eq!(event.state.title(), "");
    event.raw.close();
    let constructor_calls = MetadataOwner::new().raw.stats().engine.calls;
    for deny in 1..=constructor_calls {
        let ptr = NonNull::new(unsafe { metadata_new(16384, META_BYTES, EVENTS, deny as usize) })
            .unwrap();
        let mut raw = std::mem::ManuallyDrop::new(Raw {
            ptr: NonNull::new(unsafe { metadata_integrated(ptr.as_ptr()) }).unwrap(),
            _thread: PhantomData,
        });
        assert!(raw.stats().engine.denials > 0);
        assert_eq!(raw.stats().phase, 2);
        raw.close();
        unsafe { metadata_free(ptr.as_ptr()) };
    }
    let mut history = MetadataOwner::new();
    let input = (0..1005)
        .map(|i| format!("L{i:04}\r\n"))
        .collect::<String>();
    history.streamed(input.as_bytes()).unwrap();
    assert_eq!(history.raw.history(), 1000);
    history.feed(b"\x1b[?1049hALT\x1b]2;alternate\x07").unwrap();
    assert_eq!(history.raw.history(), 0);
    history.feed(b"\x1b[?1049l").unwrap();
    assert_eq!(history.raw.history(), 1000);
    assert_eq!(unsafe { integrated_resize(history.raw.ptr(), 12, 3) }, 0);
    history.finish(0).unwrap();
    let mut cloned_history = 0;
    assert_eq!(
        unsafe { integrated_clone(history.raw.ptr(), &mut cloned_history) },
        0
    );
    history.finish(0).unwrap();
    assert!(cloned_history <= 1000);
    history.raw.close();
    let mut exhaustion = MetadataOwner::new();
    exhaustion.state.next = u64::MAX;
    assert!(exhaustion.feed(b"\x07").is_err());
    assert_eq!(exhaustion.state.next, u64::MAX);
    assert_eq!(exhaustion.state.count, 0);
    exhaustion.raw.close();
    for _ in 0..128 {
        let mut o = MetadataOwner::new();
        o.feed(b"\x1b]2;cycle\x07\x1b]9;body\x07\x1bc").unwrap();
        o.raw.close();
    }
    println!("metadata_success=true");
}

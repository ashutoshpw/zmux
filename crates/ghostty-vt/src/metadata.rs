// SPDX-License-Identifier: MIT
// Promoted original fixed-storage normalization from the reviewed metadata owner.
use crate::owner::{Error, buffer};

const TITLE_BYTES: usize = 1024;
const CWD_BYTES: usize = 4096;
const NOTICE_TITLE: usize = 512;
const NOTICE_BODY: usize = 8192;
const NOTICE_BYTES: usize = NOTICE_TITLE + NOTICE_BODY;
const META_STATE: usize = TITLE_BYTES + CWD_BYTES + 32 * NOTICE_BYTES;
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct MetaRecord {
    pub(crate) source: u32,
    pub(crate) subcommand: u32,
    pub(crate) status: u32,
    pub(crate) terminator: u32,
    pub(crate) offset: u32,
    pub(crate) length: u32,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct Notice {
    pub(crate) sequence: u64,
    pub(crate) source: u32,
    pub(crate) title: usize,
    pub(crate) body: usize,
}
pub(crate) struct MetaState {
    pub(crate) data: Vec<u8>,
    pub(crate) title: usize,
    pub(crate) cwd: Option<usize>,
    pub(crate) notices: [Notice; 32],
    pub(crate) count: usize,
    pub(crate) next: u64,
    pub(crate) evicted: u64,
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
    pub(crate) fn new() -> Result<Self, Error> {
        Ok(Self {
            data: buffer(META_STATE).map_err(|_| Error::Allocation)?,
            title: 0,
            cwd: None,
            notices: [Notice::default(); 32],
            count: 0,
            next: 0,
            evicted: 0,
        })
    }
    pub(crate) fn copy_from(&mut self, last: &Self) {
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
    pub(crate) fn title(&self) -> &str {
        std::str::from_utf8(&self.data[..self.title]).unwrap()
    }
    pub(crate) fn cwd(&self) -> Option<&str> {
        self.cwd
            .map(|n| std::str::from_utf8(&self.data[TITLE_BYTES..TITLE_BYTES + n]).unwrap())
    }
    pub(crate) fn notice_text(&self, i: usize) -> (&str, &str) {
        let n = self.notices[i];
        let start = TITLE_BYTES + CWD_BYTES + i * NOTICE_BYTES;
        (
            std::str::from_utf8(&self.data[start..start + n.title]).unwrap(),
            std::str::from_utf8(&self.data[start + NOTICE_TITLE..start + NOTICE_TITLE + n.body])
                .unwrap(),
        )
    }
    pub(crate) fn notice(&mut self, source: u32, title: &[u8], body: &[u8]) -> Result<(), ()> {
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
    pub(crate) fn set_cwd(&mut self, bytes: &[u8]) {
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
    pub(crate) fn apply(&mut self, record: MetaRecord, bytes: &[u8]) -> Result<(), ()> {
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

use crate::{
    ffi::*,
    metadata::{MetaRecord, MetaState},
};
use std::{ffi::c_void, marker::PhantomData, mem::size_of, ptr::NonNull, rc::Rc};

const CELLS: usize = 120 * 240;
const TEXT: usize = 512 * 1024;
const EVENTS: usize = 1024;
const REPLIES: usize = 4096;
const META_BYTES: usize = 20480;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Allocation,
    Invalid,
    Pending,
    Failed,
    EngineConstruction,
    Unsupported(u32),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "terminal engine {self:?}; explicit session recovery required"
        )
    }
}
impl std::error::Error for Error {}

/// Caller-selected mapping limits, not a total RSS or application memory budget.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub engine_bytes: usize,
    pub general_bytes: usize,
    pub page_bytes: usize,
}

pub(crate) fn buffer<T: Default + Clone>(length: usize) -> Result<Vec<T>, Error> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(length)
        .map_err(|_| Error::Allocation)?;
    buffer.resize(length, T::default());
    Ok(buffer)
}

/// One thread constructs, stages, commits and destroys this owner. No C pointer
/// or callback crosses the safe boundary, and no callback writes to a PTY.
pub struct Engine {
    metadata: NonNull<c_void>,
    integrated: NonNull<c_void>, // Borrowed; metadata_free is the sole destructor.
    frame: RawFrame,
    cells: Vec<RawCell>,
    text: Vec<u8>,
    batch: Batch,
    events: Vec<Event>,
    replies: Vec<u8>,
    records: Vec<MetaRecord>,
    bytes: Vec<u8>,
    state: MetaState,
    next_state: MetaState,
    committed: u64,
    staged: bool,
    failed: bool,
    _thread: PhantomData<Rc<()>>,
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: constructor transferred one Metadata allocation to this owner;
        // its Integrated and every allocator context are destroyed in C order.
        unsafe { metadata_free(self.metadata.as_ptr()) }
    }
}

impl Engine {
    pub fn new(cols: u16, rows: u16, history: usize, limits: Limits) -> Result<Self, Error> {
        if cols == 0
            || cols > 240
            || rows == 0
            || rows > 120
            || history > 1000
            || limits.engine_bytes == 0
            || limits.engine_bytes > 64 * 1024 * 1024
            || limits.general_bytes > limits.engine_bytes
            || limits.page_bytes > limits.engine_bytes
        {
            return Err(Error::Invalid);
        }
        // Allocate all FFI scratch and normalized metadata slots before C creation.
        let cells = buffer(CELLS)?;
        let text = buffer(TEXT)?;
        let events = buffer(EVENTS)?;
        let replies = buffer(REPLIES)?;
        let records = buffer(EVENTS)?;
        let bytes = buffer(META_BYTES)?;
        let state = MetaState::new()?;
        let next_state = MetaState::new()?;
        // SAFETY: validated scalar bounds; C retains no Rust-owned pointer.
        let metadata = NonNull::new(unsafe {
            zmux_vt_new(
                cols,
                rows,
                history,
                limits.engine_bytes,
                limits.general_bytes,
                limits.page_bytes,
            )
        })
        .ok_or(Error::EngineConstruction)?;
        // SAFETY: successful construction owns a nonnull Integrated until free.
        let integrated = NonNull::new(unsafe { metadata_integrated(metadata.as_ptr()) });
        let Some(integrated) = integrated else {
            // SAFETY: still solely owned after failed internal handle validation.
            unsafe { metadata_free(metadata.as_ptr()) };
            return Err(Error::EngineConstruction);
        };
        Ok(Self {
            metadata,
            integrated,
            cells,
            text,
            events,
            replies,
            records,
            bytes,
            state,
            next_state,
            frame: RawFrame::default(),
            batch: Batch::default(),
            committed: 0,
            staged: false,
            failed: false,
            _thread: PhantomData,
        })
    }

    fn ready(&self) -> Result<(), Error> {
        if self.failed {
            Err(Error::Failed)
        } else if self.staged {
            Err(Error::Pending)
        } else {
            Ok(())
        }
    }

    /// Stages at most one worker read quantum. A second mutation is refused until
    /// exact commit. Failure is permanent; bytes are never replayed or fed elsewhere.
    pub fn stage_process(&mut self, bytes: &[u8]) -> Result<u64, Error> {
        self.ready()?;
        if bytes.len() > 4096 {
            return Err(Error::Invalid);
        }
        // SAFETY: bytes live throughout this synchronous call and C copies callbacks.
        let result =
            unsafe { integrated_feed(self.integrated.as_ptr(), bytes.as_ptr(), bytes.len()) };
        self.finish(result)
    }
    pub fn stage_resize(&mut self, cols: u16, rows: u16) -> Result<u64, Error> {
        self.ready()?;
        if cols == 0 || cols > 240 || rows == 0 || rows > 120 {
            return Err(Error::Invalid);
        }
        // SAFETY: live unique owner and checked geometry; no outstanding raw borrow.
        let result = unsafe { integrated_resize(self.integrated.as_ptr(), cols, rows) };
        self.finish(result)
    }
    pub fn stage_scroll(&mut self, lines: i32) -> Result<u64, Error> {
        self.ready()?;
        if !(-1000..=1000).contains(&lines) {
            return Err(Error::Invalid);
        }
        // SAFETY: live unique owner and checked scalar bound.
        let result = unsafe { integrated_scroll(self.integrated.as_ptr(), lines) };
        self.finish(result)
    }

    fn finish(&mut self, result: i32) -> Result<u64, Error> {
        let result = self.export(result);
        if result.is_err() {
            self.abort();
        }
        result
    }
    fn export(&mut self, status: i32) -> Result<u64, Error> {
        if status != 0 {
            return Err(Error::Failed);
        }
        let mut count = 0;
        let mut length = 0;
        // SAFETY: fixed initialized buffers have the advertised exact ABI element
        // sizes/capacities. C does not retain them and writes only during this call.
        let result = unsafe {
            metadata_copy(
                self.metadata.as_ptr(),
                self.records.as_mut_ptr(),
                size_of::<MetaRecord>(),
                EVENTS,
                &mut count,
                self.bytes.as_mut_ptr(),
                META_BYTES,
                &mut length,
            )
        };
        if result != 0 {
            return Err(Error::Failed);
        }
        // SAFETY: same owned initialized-buffer contract. This seals the C owner
        // only after complete render export and the final resource failure check.
        let result = unsafe {
            zmux_vt_export(
                self.metadata.as_ptr(),
                &mut self.batch,
                size_of::<Batch>(),
                &mut self.frame,
                size_of::<RawFrame>(),
                self.cells.as_mut_ptr(),
                size_of::<RawCell>(),
                CELLS,
                self.text.as_mut_ptr(),
                TEXT,
                self.events.as_mut_ptr(),
                size_of::<Event>(),
                EVENTS,
                self.replies.as_mut_ptr(),
                REPLIES,
            )
        };
        if result != 0 {
            return Err(Error::Failed);
        }
        self.validate_frame()?;
        if count > EVENTS
            || length > META_BYTES
            || self.batch.event_length > EVENTS
            || self.batch.reply_length > REPLIES
            || self.committed.checked_add(1) != Some(self.batch.operation_id)
            || self.batch.text_length != self.frame.text_length
        {
            return Err(Error::Invalid);
        }
        self.next_state.copy_from(&self.state);
        let (mut reply, mut metadata, mut bytes) = (0usize, 0usize, 0usize);
        for event in &self.events[..self.batch.event_length] {
            match event.kind {
                1 if event.offset == 0 && event.length == 0 => self
                    .next_state
                    .notice(1, b"Terminal needs attention", b"")
                    .map_err(|_| Error::Invalid)?,
                2 if event.offset as usize == reply => {
                    reply = reply
                        .checked_add(event.length as usize)
                        .ok_or(Error::Invalid)?;
                    if reply > self.batch.reply_length {
                        return Err(Error::Invalid);
                    }
                }
                3 if event.offset == 0 && event.length == 0 => {} // Approved metadata retention on reset.
                4 if event.offset as usize == metadata && event.length == 0 && metadata < count => {
                    let record = self.records[metadata];
                    if record.offset as usize != bytes {
                        return Err(Error::Invalid);
                    }
                    bytes = bytes
                        .checked_add(record.length as usize)
                        .ok_or(Error::Invalid)?;
                    if bytes > length {
                        return Err(Error::Invalid);
                    }
                    self.next_state
                        .apply(record, &self.bytes[record.offset as usize..bytes])
                        .map_err(|_| Error::Invalid)?;
                    metadata += 1;
                }
                _ => return Err(Error::Invalid),
            }
        }
        if reply != self.batch.reply_length || metadata != count || bytes != length {
            return Err(Error::Invalid);
        }
        self.staged = true;
        Ok(self.batch.operation_id)
    }

    fn validate_frame(&self) -> Result<(), Error> {
        let f = &self.frame;
        if f.unsupported != 0 {
            return Err(Error::Unsupported(f.unsupported));
        }
        if f.version != 1
            || f.cells_length > CELLS
            || f.text_length > TEXT
            || f.rows == 0
            || f.rows > 120
            || f.cols == 0
            || f.cols > 240
            || f.cursor_x >= f.cols
            || f.cursor_y >= f.rows
            || [
                f.pending_wrap,
                f.cursor_visible,
                f.application_cursor,
                f.bracketed_paste,
                f.alternate,
            ]
            .iter()
            .any(|v| *v > 1)
            || f.history_rows > 1000
            || f.scroll_len != u64::from(f.rows)
            || f.scroll_total.checked_sub(f.scroll_len) != Some(f.history_rows)
            || f.scroll_offset > f.history_rows
        {
            return Err(Error::Invalid);
        }
        let mut columns = [0u16; 120];
        let mut next_text = 0usize;
        for c in &self.cells[..f.cells_length] {
            let end = (c.text_offset as usize)
                .checked_add(c.text_length as usize)
                .ok_or(Error::Invalid)?;
            if c.row >= f.rows
                || c.column != columns[c.row as usize]
                || !matches!(c.width, 1 | 2)
                || u32::from(c.column) + u32::from(c.width) > u32::from(f.cols)
                || c.text_offset as usize != next_text
                || end > f.text_length
                || end == next_text
                || [c.bold, c.italic, c.underline, c.inverse]
                    .iter()
                    .any(|v| *v > 1)
                || c.foreground.kind > 2
                || c.background.kind > 2
                || std::str::from_utf8(&self.text[next_text..end]).is_err()
            {
                return Err(Error::Invalid);
            }
            columns[c.row as usize] += u16::from(c.width);
            next_text = end;
        }
        if next_text != f.text_length || columns[..f.rows as usize].iter().any(|n| *n != f.cols) {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    /// Validated owned candidate data; readable only for the outstanding batch.
    pub fn frame(&self) -> Result<(&RawFrame, &[RawCell], &[u8]), Error> {
        if !self.staged || self.failed {
            return Err(Error::Invalid);
        }
        Ok((
            &self.frame,
            &self.cells[..self.frame.cells_length],
            &self.text[..self.frame.text_length],
        ))
    }
    pub fn replies(&self) -> Result<&[u8], Error> {
        if !self.staged || self.failed {
            return Err(Error::Invalid);
        }
        Ok(&self.replies[..self.batch.reply_length])
    }
    fn metadata_state(&self) -> &MetaState {
        if self.staged && !self.failed {
            &self.next_state
        } else {
            &self.state
        }
    }
    pub fn title(&self) -> &str {
        self.metadata_state().title()
    }
    pub fn cwd(&self) -> Option<&str> {
        self.metadata_state().cwd()
    }
    pub fn notices(&self) -> impl Iterator<Item = (u64, u32, &str, &str)> {
        let state = self.metadata_state();
        state.notices[..state.count]
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let (title, body) = state.notice_text(i);
                (n.sequence, n.source, title, body)
            })
    }

    /// Caller must first finish every causal reply's physical PTY write, preserving
    /// earlier admitted input order. Zero-reply batches still require this commit.
    /// No allocation or engine access occurs between validated export and commit.
    pub fn commit(&mut self, generation: u64) -> Result<(), Error> {
        if self.failed || !self.staged || generation != self.batch.operation_id {
            return Err(Error::Invalid);
        }
        // SAFETY: exact outstanding generation of this unique sealed C owner.
        if unsafe { metadata_commit(self.metadata.as_ptr(), generation) } != 0 {
            self.abort();
            return Err(Error::Failed);
        }
        std::mem::swap(&mut self.state, &mut self.next_state);
        self.committed = generation;
        self.staged = false;
        Ok(())
    }
    /// Permanently refuse further engine use. Safe to call repeatedly; C abort runs once.
    pub fn abort(&mut self) {
        if !self.failed {
            // SAFETY: live unique owner; abort neither frees nor invokes host callbacks.
            unsafe { integrated_abort(self.integrated.as_ptr()) };
            self.failed = true;
            self.staged = false;
        }
    }
}

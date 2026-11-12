use crate::{
    InvalidSize, Pointer, ProgramOpenRequest, ProgramOpenRequestError, Size, Terminal,
    TerminalError, Viewport, Wheel,
};
use thiserror::Error;
use zmux_program_status::SessionEpoch;

/// Explicit backend selection. Existing runtime callers continue using Terminal;
/// this staged integration seam never changes the default backend implicitly.
#[derive(Clone, Copy, Debug, Default)]
pub enum EngineKind {
    #[default]
    Vt100,
    #[cfg(feature = "ghostty")]
    Ghostty(zmux_ghostty_vt::Limits),
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    Size(#[from] InvalidSize),
    #[error(transparent)]
    Terminal(#[from] TerminalError),
    #[error("terminal has an outstanding staged generation")]
    Pending,
    #[error("terminal generation or operation argument is invalid")]
    Invalid,
    #[error("terminal owner unavailable; explicit session recovery required")]
    Failed,
    #[error("cannot reserve terminal candidate storage")]
    Allocation,
    #[cfg(feature = "ghostty")]
    #[error(transparent)]
    Ghostty(#[from] zmux_ghostty_vt::Error),
}

/// One exact frame/metadata candidate and its finite causal reply bytes.
/// Queue admission is not reply delivery: commit only after complete physical
/// writes, including any previously admitted user bytes, in the sole PTY writer.
#[derive(Debug)]
pub struct EngineBatch {
    generation: u64,
    replies: Vec<u8>,
}

impl EngineBatch {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn replies(&self) -> &[u8] {
        &self.replies
    }
}

// Keep fixed owner storage inline rather than add an infallible heap allocation
// after fallible scratch reservation. This enum lives once on its worker.
#[allow(clippy::large_enum_variant)]
enum Backend {
    Vt100(Terminal),
    #[cfg(feature = "ghostty")]
    Ghostty(zmux_ghostty_vt::Engine),
}

/// Worker-bound terminal owner with explicit staged publication. While staged,
/// callers must not read more PTY output, mutate terminal/PTY geometry, or dequeue
/// new input. On cancellation or write failure, abort; do not replay or fall back.
/// The worker owns input admission, PTY polling and physical writes. Semantic
/// key/paste encoding reads only this idle authoritative owner; application wheel
/// support remains an explicit separate capability at the worker boundary.
pub struct Engine {
    backend: Backend,
    published: Viewport,
    candidate: Option<Viewport>,
    staged_generation: Option<u64>,
    generation: u64,
    failed: bool,
}

impl Engine {
    pub fn encode_key(&mut self, event: crate::KeyEvent) -> Result<Vec<u8>, EngineError> {
        self.ready()?;
        if !event.valid() {
            return Err(EngineError::Invalid);
        }
        let result = match &mut self.backend {
            Backend::Vt100(terminal) => Ok(terminal.encode_key(event)),
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(terminal) => terminal
                .encode_key(event.key as u32, event.modifiers, event.action as u8)
                .map_err(EngineError::from),
        };
        if result.is_err() {
            self.abort();
        }
        result
    }
    pub fn encode_paste(&mut self, text: &str) -> Result<Vec<u8>, EngineError> {
        self.ready()?;
        if text.is_empty() || text.len() > crate::PASTE_BYTES {
            return Err(EngineError::Invalid);
        }
        let result = match &mut self.backend {
            Backend::Vt100(terminal) => Ok(terminal.encode_paste(text)),
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(terminal) => terminal
                .encode_paste(text.as_bytes())
                .map_err(EngineError::from),
        };
        if result.is_err() {
            self.abort();
        }
        result
    }
    /// Construct and export the initial frame on its worker before PTY spawn.
    /// Ghostty mapping limits are explicit caller policy, with no production default.
    pub fn new(size: Size, kind: EngineKind) -> Result<Self, EngineError> {
        Self::new_with_open_request_nonce(size, kind, None)
    }

    /// Construct an engine with an optional session-bound opener nonce. Ghostty
    /// currently has no OSC opener metadata support and ignores this value.
    pub fn new_with_open_request_nonce(
        size: Size,
        kind: EngineKind,
        expected_open_nonce: Option<[u8; 16]>,
    ) -> Result<Self, EngineError> {
        Self::new_with_session_epoch(size, kind, expected_open_nonce, SessionEpoch(0))
    }

    /// Construct an engine whose program-status owner belongs to one worker epoch.
    pub fn new_with_session_epoch(
        size: Size,
        kind: EngineKind,
        expected_open_nonce: Option<[u8; 16]>,
        session_epoch: SessionEpoch,
    ) -> Result<Self, EngineError> {
        size.validate()?;
        let (backend, published, generation) = match kind {
            EngineKind::Vt100 => {
                let terminal = Terminal::new_with_open_request_nonce_and_epoch(
                    size,
                    expected_open_nonce,
                    session_epoch,
                )?;
                let published = terminal.try_viewport()?;
                (Backend::Vt100(terminal), published, 0)
            }
            #[cfg(feature = "ghostty")]
            EngineKind::Ghostty(limits) => {
                let mut terminal = zmux_ghostty_vt::Engine::new_with_session_epoch(
                    size.cols,
                    size.rows,
                    crate::MAX_SCROLLBACK_LINES,
                    limits,
                    session_epoch,
                )?;
                let generation = terminal.stage_process(&[])?;
                let published = ghostty_viewport(&terminal, generation)?;
                if !terminal.replies()?.is_empty() {
                    return Err(EngineError::Invalid);
                }
                terminal.commit(generation)?;
                (Backend::Ghostty(terminal), published, generation)
            }
        };
        Ok(Self {
            backend,
            published,
            candidate: None,
            staged_generation: None,
            generation,
            failed: false,
        })
    }

    fn ready(&self) -> Result<(), EngineError> {
        if self.failed {
            Err(EngineError::Failed)
        } else if self.staged_generation.is_some() {
            Err(EngineError::Pending)
        } else {
            Ok(())
        }
    }

    pub fn viewport(&self) -> &Viewport {
        &self.published
    }

    /// Export the latest committed parser state at the session frame cadence.
    /// Default vt100 PTY reads can commit without repeatedly copying every cell.
    pub fn refresh_viewport(&mut self) -> Result<(), EngineError> {
        self.ready()?;
        match &self.backend {
            Backend::Vt100(terminal) => {
                let mut viewport = terminal.try_viewport()?;
                viewport.viewport_generation = self.generation;
                self.published = viewport;
            }
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(_) => {}
        }
        Ok(())
    }

    /// Drain one-shot events from the selected parser. Ghostty currently has no OSC metadata
    /// callback; its link export is limited to visible plain HTTP(S) text.
    pub fn take_open_requests(
        &mut self,
    ) -> Result<Vec<ProgramOpenRequest>, ProgramOpenRequestError> {
        match &mut self.backend {
            Backend::Vt100(terminal) => terminal.take_open_requests(),
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(_) => Ok(Vec::new()),
        }
    }

    /// At most one bounded worker read quantum; successful zero-reply stages
    /// still require exact commit before the candidate becomes observable.
    pub fn stage_process(&mut self, bytes: &[u8]) -> Result<EngineBatch, EngineError> {
        self.ready()?;
        if bytes.len() > 4096 {
            return Err(EngineError::Invalid);
        }
        let result = match &mut self.backend {
            Backend::Vt100(terminal) => terminal.process(bytes).map_err(EngineError::from),
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(terminal) => terminal
                .stage_process(bytes)
                .map(|_| ())
                .map_err(EngineError::from),
        };
        self.finish(result)
    }

    /// Session workers commit output per bounded PTY read, then export one full
    /// viewport at the existing frame cadence. Ghostty keeps its exact export
    /// boundary because that owner requires a sealed render candidate.
    pub fn stage_process_deferred(&mut self, bytes: &[u8]) -> Result<EngineBatch, EngineError> {
        self.ready()?;
        if bytes.len() > 4096 {
            return Err(EngineError::Invalid);
        }
        #[cfg(feature = "ghostty")]
        if let Backend::Ghostty(terminal) = &mut self.backend {
            let result = terminal
                .stage_process(bytes)
                .map(|_| ())
                .map_err(EngineError::from);
            return self.finish(result);
        }
        let result = match &mut self.backend {
            Backend::Vt100(terminal) => terminal.process(bytes).map_err(EngineError::from),
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(_) => unreachable!(),
        };
        self.finish_deferred(result)
    }

    pub fn stage_scroll_deferred(&mut self, lines: i32) -> Result<EngineBatch, EngineError> {
        self.ready()?;
        if !(-1000..=1000).contains(&lines) {
            return Err(EngineError::Invalid);
        }
        #[cfg(feature = "ghostty")]
        if let Backend::Ghostty(terminal) = &mut self.backend {
            let result = terminal
                .stage_scroll(lines)
                .map(|_| ())
                .map_err(EngineError::from);
            return self.finish(result);
        }
        let result = match &mut self.backend {
            Backend::Vt100(terminal) => terminal.scroll(lines).map_err(EngineError::from),
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(_) => unreachable!(),
        };
        self.finish_deferred(result)
    }

    pub fn pointer_deferred(
        &mut self,
        event: Pointer,
    ) -> Result<(Vec<u8>, EngineBatch), EngineError> {
        self.ready()?;
        let bytes = match &mut self.backend {
            Backend::Vt100(terminal) => terminal.pointer(event),
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(_) => return Err(EngineError::Invalid),
        };
        let batch = self.finish_deferred(Ok(()))?;
        Ok((bytes, batch))
    }

    pub fn wheel_deferred(&mut self, event: Wheel) -> Result<(Vec<u8>, EngineBatch), EngineError> {
        self.ready()?;
        #[cfg(feature = "ghostty")]
        if event.history && matches!(&self.backend, Backend::Ghostty(_)) {
            let batch = self.stage_scroll(event.lines.clamp(-1000, 1000))?;
            return Ok((Vec::new(), batch));
        }
        match &mut self.backend {
            Backend::Vt100(terminal) => {
                let bytes = terminal.wheel(event);
                let batch = self.finish_deferred(Ok(()))?;
                Ok((bytes, batch))
            }
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(_) => Err(EngineError::Invalid),
        }
    }

    /// Stage the observed attached-child exit after its final PTY output drain.
    /// Done/error records remain; working/blocked records retire in this batch.
    pub fn stage_attached_child_exited(&mut self) -> Result<EngineBatch, EngineError> {
        self.ready()?;
        let result = match &mut self.backend {
            Backend::Vt100(terminal) => terminal.attached_child_exited().map_err(EngineError::from),
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(terminal) => terminal
                .stage_process(&[])
                .and_then(|_| terminal.attached_child_exited())
                .map(|_| ())
                .map_err(EngineError::from),
        };
        self.finish(result)
    }

    pub fn terminal_closed_viewport(&mut self) -> Result<Viewport, EngineError> {
        let status = match &mut self.backend {
            Backend::Vt100(terminal) => terminal
                .terminal_closed_status_snapshot()
                .map_err(EngineError::from)?,
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(terminal) => terminal
                .terminal_closed_status_snapshot()
                .map_err(EngineError::from)?,
        };
        let mut viewport = self.published.clone();
        viewport.program_status = status;
        Ok(viewport)
    }

    pub fn pointer(&mut self, event: Pointer) -> Result<(Vec<u8>, EngineBatch), EngineError> {
        self.ready()?;
        let bytes = match &mut self.backend {
            Backend::Vt100(terminal) => terminal.pointer(event),
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(_) => return Err(EngineError::Invalid),
        };
        let batch = self.finish(Ok(()))?;
        Ok((bytes, batch))
    }

    pub fn wheel(&mut self, event: Wheel) -> Result<(Vec<u8>, EngineBatch), EngineError> {
        self.ready()?;
        #[cfg(feature = "ghostty")]
        if event.history && matches!(&self.backend, Backend::Ghostty(_)) {
            let batch = self.stage_scroll(event.lines.clamp(-1000, 1000))?;
            return Ok((Vec::new(), batch));
        }
        match &mut self.backend {
            Backend::Vt100(terminal) => {
                let bytes = terminal.wheel(event);
                let batch = self.finish(Ok(()))?;
                Ok((bytes, batch))
            }
            #[cfg(feature = "ghostty")]
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(_) => Err(EngineError::Invalid),
        }
    }

    /// Stage engine geometry first, then let the worker apply kernel PTY geometry.
    /// A kernel resize failure must abort the session and retain its old published
    /// frame: kernel and engine geometry are not rollback-atomic. Commit only after
    /// kernel success and complete causal reply writes.
    pub fn stage_resize(&mut self, size: Size) -> Result<EngineBatch, EngineError> {
        self.ready()?;
        size.validate()?;
        let result = match &mut self.backend {
            Backend::Vt100(terminal) => terminal.resize(size).map_err(EngineError::from),
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(terminal) => terminal
                .stage_resize(size.cols, size.rows)
                .map(|_| ())
                .map_err(EngineError::from),
        };
        self.finish(result)
    }

    pub fn stage_scroll(&mut self, lines: i32) -> Result<EngineBatch, EngineError> {
        self.ready()?;
        if !(-1000..=1000).contains(&lines) {
            return Err(EngineError::Invalid);
        }
        let result = match &mut self.backend {
            Backend::Vt100(terminal) => terminal.scroll(lines).map_err(EngineError::from),
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(terminal) => terminal
                .stage_scroll(lines)
                .map(|_| ())
                .map_err(EngineError::from),
        };
        self.finish(result)
    }

    fn finish(&mut self, result: Result<(), EngineError>) -> Result<EngineBatch, EngineError> {
        let result = result.and_then(|()| self.export());
        if result.is_err() {
            self.abort();
        }
        result
    }

    fn finish_deferred(
        &mut self,
        result: Result<(), EngineError>,
    ) -> Result<EngineBatch, EngineError> {
        let result = result.and_then(|()| match &mut self.backend {
            Backend::Vt100(terminal) => {
                let generation = self.generation.checked_add(1).ok_or(EngineError::Invalid)?;
                let replies = terminal.take_replies()?;
                self.candidate = None;
                self.staged_generation = Some(generation);
                Ok(EngineBatch {
                    generation,
                    replies,
                })
            }
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(_) => Err(EngineError::Invalid),
        });
        if result.is_err() {
            self.abort();
        }
        result
    }

    fn export(&mut self) -> Result<EngineBatch, EngineError> {
        let generation = self.generation.checked_add(1).ok_or(EngineError::Invalid)?;
        let (candidate, replies) = match &mut self.backend {
            Backend::Vt100(terminal) => (terminal.try_viewport()?, terminal.take_replies()?),
            #[cfg(feature = "ghostty")]
            Backend::Ghostty(terminal) => {
                let mut replies = Vec::new();
                replies
                    .try_reserve_exact(terminal.replies()?.len())
                    .map_err(|_| EngineError::Allocation)?;
                replies.extend_from_slice(terminal.replies()?);
                (ghostty_viewport(terminal, generation)?, replies)
            }
        };
        let mut candidate = candidate;
        candidate.viewport_generation = generation;
        self.candidate = Some(candidate);
        self.staged_generation = Some(generation);
        Ok(EngineBatch {
            generation,
            replies,
        })
    }

    pub fn commit(&mut self, generation: u64) -> Result<(), EngineError> {
        if self.failed
            || self.staged_generation != Some(generation)
            || self.generation.checked_add(1) != Some(generation)
        {
            return Err(EngineError::Invalid);
        }
        #[cfg(feature = "ghostty")]
        if let Backend::Ghostty(terminal) = &mut self.backend
            && let Err(error) = terminal.commit(generation)
        {
            self.abort();
            return Err(error.into());
        }
        // Move only after the same exact nonallocating engine commit succeeds.
        if let Some(candidate) = self.candidate.take() {
            self.published = candidate;
        }
        self.generation = generation;
        self.staged_generation = None;
        Ok(())
    }

    /// Abort once after cancellation, deadline or any partial/failed reply write.
    /// The previous viewport/metadata/notices remain available; no later commit,
    /// mutation, transparent fallback or replay can succeed.
    pub fn abort(&mut self) {
        if !self.failed {
            #[cfg(feature = "ghostty")]
            if let Backend::Ghostty(terminal) = &mut self.backend {
                terminal.abort();
            }
            self.candidate = None;
            self.staged_generation = None;
            self.failed = true;
        }
    }
}

#[cfg(feature = "ghostty")]
fn owned(text: &str) -> Result<String, EngineError> {
    let mut result = String::new();
    result
        .try_reserve_exact(text.len())
        .map_err(|_| EngineError::Allocation)?;
    result.push_str(text);
    Ok(result)
}

#[cfg(feature = "ghostty")]
fn ghostty_viewport(
    engine: &zmux_ghostty_vt::Engine,
    generation: u64,
) -> Result<Viewport, EngineError> {
    use crate::{Color, Run, Style, TerminalNotice};
    fn color(c: zmux_ghostty_vt::RawColor) -> Color {
        match c.kind {
            0 => Color::Default,
            1 => Color::Indexed(c.index),
            _ => Color::Rgb([c.r, c.g, c.b]),
        }
    }
    let (f, cells, text) = engine.frame()?;
    let mut lines: Vec<Vec<Run>> = Vec::new();
    lines
        .try_reserve_exact(f.rows as usize)
        .map_err(|_| EngineError::Allocation)?;
    for _ in 0..f.rows {
        lines.push(Vec::new());
    }
    for c in cells {
        // Safe bridge validated exact UTF-8 ranges, cell order, styles and geometry.
        let value = std::str::from_utf8(
            &text[c.text_offset as usize..(c.text_offset + c.text_length) as usize],
        )
        .map_err(|_| EngineError::Invalid)?;
        let style = Style {
            foreground: color(c.foreground),
            background: color(c.background),
            bold: c.bold != 0,
            italic: c.italic != 0,
            underline: c.underline != 0,
            inverse: c.inverse != 0,
        };
        let row = &mut lines[c.row as usize];
        if value.is_ascii()
            && let Some(last) = row.last_mut()
            && last.text.is_ascii()
            && last.style == style
            && last.column + last.width == c.column
        {
            last.text
                .try_reserve_exact(value.len())
                .map_err(|_| EngineError::Allocation)?;
            last.text.push_str(value);
            last.width += u16::from(c.width);
        } else {
            row.try_reserve(1).map_err(|_| EngineError::Allocation)?;
            row.push(Run {
                column: c.column,
                width: u16::from(c.width),
                text: owned(value)?,
                style,
            });
        }
    }
    let mut contents = String::new();
    contents
        .try_reserve_exact(text.len() + f.rows as usize)
        .map_err(|_| EngineError::Allocation)?;
    for (i, runs) in lines.iter().enumerate() {
        if i != 0 {
            contents.push('\n');
        }
        let start = contents.len();
        for run in runs {
            contents.push_str(&run.text);
        }
        let end = contents.trim_end_matches(' ').len().max(start);
        contents.truncate(end);
    }
    contents.truncate(contents.trim_end_matches('\n').len());
    let mut notices = Vec::new();
    notices
        .try_reserve_exact(32)
        .map_err(|_| EngineError::Allocation)?;
    for (sequence, source, title, body) in engine.notices() {
        notices.push(TerminalNotice {
            sequence,
            source: owned(match source {
                1 => "bell",
                9 => "osc9",
                777 => "osc777",
                778 => "osc777-inbox",
                _ => return Err(EngineError::Invalid),
            })?,
            title: owned(title)?,
            body: owned(body)?,
        });
    }
    Ok(Viewport {
        viewport_generation: generation,
        links: crate::plain_links_from_runs(&lines),
        notices,
        title: owned(engine.title())?,
        working_directory: engine.cwd().map(owned).transpose()?,
        size: Size {
            rows: f.rows,
            cols: f.cols,
        },
        contents,
        cursor_row: f.cursor_y,
        cursor_col: if f.pending_wrap != 0 {
            f.cols
        } else {
            f.cursor_x
        },
        lines,
        // The optional exporter has no authoritative per-row wrap/copy extents.
        // Preserve physical-row copying; pending cursor wrap is not row metadata.
        row_copy: None,
        // This engine does not implement the semantic pointer contract.
        pointer_tracking: None,
        application_cursor: f.application_cursor != 0,
        bracketed_paste: f.bracketed_paste != 0,
        hide_cursor: f.cursor_visible == 0,
        scrollback_offset: (f.history_rows - f.scroll_offset) as usize,
        scrollback_lines: f.history_rows as usize,
        alternate_screen: f.alternate != 0,
        program_status: engine.program_status_snapshot()?,
    })
}

//! Headless parser feasibility adapter, not a production renderer or full terminal engine.

mod pointer;
pub use pointer::{Pointer, PointerAction, PointerButton};
mod engine;
mod input;
pub use engine::{Engine, EngineBatch, EngineError, EngineKind};
pub use input::{ENCODED_PASTE_BYTES, Key, KeyAction, KeyEvent, PASTE_BYTES};
#[cfg(feature = "ghostty")]
pub use zmux_ghostty_vt::Limits as GhosttyLimits;

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use subtle::ConstantTimeEq as _;

pub const MAX_PROGRAM_OPEN_REQUESTS: usize = 8;
pub const MAX_PROGRAM_OPEN_URL_BYTES: usize = 8_192;
const MAX_PROGRAM_OPEN_URL_BASE64_BYTES: usize = 10_923;
const MAX_LINK_SPANS: usize = 512;
const MAX_LINK_SPAN_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb([u8; 3]),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Style {
    pub foreground: Color,
    pub background: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Run {
    pub column: u16,
    pub width: u16,
    pub text: String,
    pub style: Style,
}

/// A visible terminal link. Columns are zero-based, with an exclusive end.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LinkSpan {
    pub row: u16,
    pub start_column: u16,
    pub end_column: u16,
    pub url: String,
}

/// A parsed explicit opener event. The nonce and URL exist only in the
/// session-owned event queue; this type is intentionally not serializable or
/// printable through `Debug`.
pub struct ProgramOpenRequest {
    nonce: [u8; 16],
    url: String,
}

#[derive(Debug, Error)]
pub enum ProgramOpenRequestError {
    #[error("terminal opener request queue exceeded its bound")]
    QueueOverflow,
}

impl ProgramOpenRequest {
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn matches_nonce(&self, expected: &[u8; 16]) -> bool {
        bool::from(self.nonce.ct_eq(expected))
    }
}
use thiserror::Error;

pub const MAX_ROWS: u16 = 120;
pub const MAX_COLS: u16 = 240;
pub const MAX_SCROLLBACK_LINES: usize = 1_000;
pub const MAX_CONTROL_STRING_BYTES: usize = 16_384;

#[derive(Debug, Error)]
pub enum TerminalError {
    #[error(
        "terminal control string exceeded {MAX_CONTROL_STRING_BYTES} bytes; session recovery required"
    )]
    ControlStringTooLong,
    #[error("terminal viewport generation is exhausted; session recovery required")]
    ViewportGenerationExhausted,
    #[error("terminal owner failed; session recovery required")]
    Failed,
    #[error(transparent)]
    InvalidSize(#[from] InvalidSize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Size {
    pub rows: u16,
    pub cols: u16,
}

#[derive(Debug, Error)]
#[error("terminal size must be 1–{MAX_ROWS} rows and 1–{MAX_COLS} columns")]
pub struct InvalidSize;

impl Size {
    pub fn validate(self) -> Result<Self, InvalidSize> {
        if self.rows == 0 || self.cols == 0 || self.rows > MAX_ROWS || self.cols > MAX_COLS {
            Err(InvalidSize)
        } else {
            Ok(self)
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct TerminalNotice {
    pub sequence: u64,
    pub source: String,
    pub title: String,
    pub body: String,
}

/// Copy boundaries for one visible physical row, from the same parser snapshot
/// as its drawing runs. Written spaces count; empty padding does not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RowCopy {
    pub wraps_to_next: bool,
    pub written_end: u16,
}

#[derive(Clone, Debug, Serialize)]
pub struct Viewport {
    pub viewport_generation: u64,
    pub links: Vec<LinkSpan>,
    pub notices: Vec<TerminalNotice>,
    pub title: String,
    pub working_directory: Option<String>,
    pub size: Size,
    pub contents: String,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub lines: Vec<Vec<Run>>,
    pub row_copy: Option<Vec<RowCopy>>,
    pub pointer_tracking: Option<PointerTracking>,
    pub application_cursor: bool,
    pub bracketed_paste: bool,
    pub hide_cursor: bool,
    pub scrollback_offset: usize,
    pub scrollback_lines: usize,
    pub alternate_screen: bool,
}

/// A native routing hint; report encoding still uses the worker's current mode.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerTracking {
    None,
    Press,
    PressRelease,
    ButtonMotion,
    AnyMotion,
}

#[derive(Default)]
enum ControlMode {
    #[default]
    Ground,
    Escape,
    String(usize),
}

/// Pointer coordinates are zero-based grid cells; history bypasses application tracking.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wheel {
    pub lines: i32,
    pub row: u16,
    pub col: u16,
    pub modifiers: u8,
    pub history: bool,
}

/// One worker owns construction, mutation, export and destruction. This contract
/// stays independent of the backend so thread-bound C owners need no unsafe Send.
pub struct Terminal {
    parser: vt100::Parser<Metadata>,
    size: Size,
    control: ControlMode,
    failed: bool,
    scrollback_lines: usize,
    viewport_generation: u64,
    _worker_owned: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[derive(Default)]
struct Metadata {
    title: String,
    notices: Vec<TerminalNotice>,
    next_notice: u64,
    working_directory: Option<String>,
    expected_open_nonce: Option<[u8; 16]>,
    open_requests: VecDeque<ProgramOpenRequest>,
    open_requests_overflowed: bool,
}

fn display_text(bytes: &[u8], limit: usize) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(limit)
        .collect()
}

impl Metadata {
    fn open_request(&mut self, nonce: &[u8], encoded_url: &[u8]) {
        let Some(expected_nonce) = self.expected_open_nonce else {
            return;
        };
        let Some(nonce) = decode_open_nonce(nonce) else {
            return;
        };
        if !bool::from(nonce.ct_eq(&expected_nonce)) {
            return;
        }
        if self.open_requests_overflowed {
            return;
        }
        let Some(bytes) = decode_base64url(encoded_url) else {
            return;
        };
        let Ok(url) = String::from_utf8(bytes) else {
            return;
        };
        if self.open_requests.len() == MAX_PROGRAM_OPEN_REQUESTS {
            self.open_requests.clear();
            self.open_requests_overflowed = true;
            return;
        }
        self.open_requests
            .push_back(ProgramOpenRequest { nonce, url });
    }

    fn notice(&mut self, source: &str, title: &[u8], body: &[u8]) {
        let title = display_text(title, 128);
        if title.is_empty() {
            return;
        }
        self.next_notice = self.next_notice.saturating_add(1);
        if self.notices.len() == 32 {
            self.notices.remove(0);
        }
        self.notices.push(TerminalNotice {
            sequence: self.next_notice,
            source: source.into(),
            title,
            body: display_text(body, 2048),
        });
    }
}
impl vt100::Callbacks for Metadata {
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.notice("bell", b"Terminal needs attention", b"");
    }
    fn visual_bell(&mut self, _: &mut vt100::Screen) {
        self.notice("bell", b"Terminal needs attention", b"");
    }
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = display_text(title, 256);
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        match params {
            [b"9", message] => self.notice("osc9", b"Terminal notification", message),
            [b"777", b"notify", title, body] => self.notice("osc777", title, body),
            [b"777", b"zmux-open", b"v1", nonce, encoded_url] => {
                self.open_request(nonce, encoded_url)
            }
            _ => {}
        }
        if let [b"7", value] = params {
            let Ok(uri) = std::str::from_utf8(value) else {
                return;
            };
            let Some(authority_path) = uri.strip_prefix("file://") else {
                return;
            };
            let Some(start) = authority_path.find('/') else {
                return;
            };
            let path = &authority_path.as_bytes()[start..];
            let mut decoded = Vec::with_capacity(path.len());
            let mut i = 0;
            while i < path.len() {
                if path[i] == b'%'
                    && i + 2 < path.len()
                    && let (Some(hi), Some(lo)) = (
                        (path[i + 1] as char).to_digit(16),
                        (path[i + 2] as char).to_digit(16),
                    )
                {
                    decoded.push((hi * 16 + lo) as u8);
                    i += 3;
                } else {
                    decoded.push(path[i]);
                    i += 1;
                }
            }
            // Preserve a path exactly or reject it. The session coordinator may use this
            // local shell-integration metadata as a cwd descriptor; never interpret it as input.
            self.working_directory = String::from_utf8(decoded).ok().filter(|path| {
                path.starts_with('/') && path.len() <= 4096 && !path.chars().any(char::is_control)
            });
        }
    }
}

impl Terminal {
    pub fn new(size: Size) -> Result<Self, InvalidSize> {
        Self::new_with_open_request_nonce(size, None)
    }

    /// Construct a terminal that accepts explicit opener requests only for the
    /// private nonce bound to its owning session. With no nonce, opener OSCs are ignored.
    pub fn new_with_open_request_nonce(
        size: Size,
        expected_open_nonce: Option<[u8; 16]>,
    ) -> Result<Self, InvalidSize> {
        let size = size.validate()?;
        let metadata = Metadata {
            expected_open_nonce,
            ..Metadata::default()
        };
        Ok(Self {
            parser: vt100::Parser::new_with_callbacks(
                size.rows,
                size.cols,
                MAX_SCROLLBACK_LINES,
                metadata,
            ),
            size,
            control: ControlMode::Ground,
            failed: false,
            scrollback_lines: 0,
            viewport_generation: 0,
            _worker_owned: std::marker::PhantomData,
        })
    }

    pub fn process(&mut self, bytes: &[u8]) -> Result<(), TerminalError> {
        if self.failed {
            return Err(TerminalError::Failed);
        }
        // An original conservative resource guard, not a second escape-sequence parser.
        // ESC introduces control strings; UTF-8 continuation bytes are never treated as C1.
        for &byte in bytes {
            self.control = match (&self.control, byte) {
                (_, 0x1b) => ControlMode::Escape,
                (_, 0x18 | 0x1a) => ControlMode::Ground,
                (ControlMode::Escape, b']' | b'P' | b'_' | b'^' | b'X') => ControlMode::String(0),
                (ControlMode::Escape, 0x00..=0x1f) => ControlMode::Escape,
                (ControlMode::String(_), 0x07) => ControlMode::Ground,
                (ControlMode::String(count), _) => {
                    if *count == MAX_CONTROL_STRING_BYTES {
                        self.failed = true;
                        return Err(TerminalError::ControlStringTooLong);
                    }
                    ControlMode::String(count + 1)
                }
                _ => ControlMode::Ground,
            };
        }
        self.parser.process(bytes);
        self.measure_history();
        self.advance_viewport_generation()?;
        Ok(())
    }

    fn advance_viewport_generation(&mut self) -> Result<(), TerminalError> {
        let Some(generation) = self.viewport_generation.checked_add(1) else {
            self.failed = true;
            return Err(TerminalError::ViewportGenerationExhausted);
        };
        self.viewport_generation = generation;
        Ok(())
    }

    /// Drain private one-shot requests for the owning session worker. These values never cross
    /// the serialized viewport boundary.
    pub fn take_open_requests(
        &mut self,
    ) -> Result<Vec<ProgramOpenRequest>, ProgramOpenRequestError> {
        let metadata = self.parser.callbacks_mut();
        if std::mem::take(&mut metadata.open_requests_overflowed) {
            metadata.open_requests.clear();
            return Err(ProgramOpenRequestError::QueueOverflow);
        }
        Ok(metadata.open_requests.drain(..).collect())
    }

    pub fn resize(&mut self, size: Size) -> Result<(), TerminalError> {
        if self.failed {
            return Err(TerminalError::Failed);
        }
        let size = size.validate()?;
        self.parser.screen_mut().set_size(size.rows, size.cols);
        self.size = size;
        self.measure_history();
        self.advance_viewport_generation()
    }

    // Probe the parser's bounded extent without copying stored history. Restore the
    // current anchor before publishing; live parsing remains worker-owned.
    fn measure_history(&mut self) {
        let screen = self.parser.screen_mut();
        let offset = screen.scrollback();
        screen.set_scrollback(MAX_SCROLLBACK_LINES);
        self.scrollback_lines = screen.scrollback();
        screen.set_scrollback(offset);
    }

    /// Positive lines move toward older output; zero returns to the live screen.
    pub fn scroll(&mut self, lines: i32) -> Result<(), TerminalError> {
        if self.failed {
            return Err(TerminalError::Failed);
        }
        let screen = self.parser.screen_mut();
        let prior = screen.scrollback();
        let offset = if lines == 0 {
            0
        } else {
            screen
                .scrollback()
                .saturating_add_signed(lines.clamp(-1000, 1000) as isize)
        };
        screen.set_scrollback(offset);
        if screen.scrollback() != prior {
            self.advance_viewport_generation()?;
        }
        Ok(())
    }

    /// Resolve wheel routing against the authoritative parser mode, not a stale UI
    /// frame. Return raw bytes: legacy mouse encoding is not necessarily UTF-8.
    pub fn wheel(&mut self, event: Wheel) -> Vec<u8> {
        let count = event.lines.unsigned_abs().min(32) as usize;
        if count == 0 {
            return Vec::new();
        }
        let screen = self.parser.screen();
        if !event.history && screen.mouse_protocol_mode() != vt100::MouseProtocolMode::None {
            let code = (if event.lines > 0 { 64 } else { 65 }) + (event.modifiers & 28);
            let report = self.mouse_report(code, event.row, event.col, false);
            if self.scroll(0).is_err() {
                return Vec::new();
            }
            report.repeat(count)
        } else if !event.history && screen.alternate_screen() {
            let report = [
                27,
                if screen.application_cursor() {
                    b'O'
                } else {
                    b'['
                },
                if event.lines > 0 { b'A' } else { b'B' },
            ];
            report.repeat(count)
        } else {
            let _ = self.scroll(event.lines);
            Vec::new()
        }
    }

    fn runs(&self) -> (Vec<Vec<Run>>, Vec<RowCopy>) {
        (0..self.size.rows)
            .map(|row| {
                let mut runs: Vec<Run> = Vec::new();
                let mut written_end = 0;
                for col in 0..self.size.cols {
                    // Scrollback rows retain their original width after a resize. Within the
                    // current geometry, missing cells in those rows are blank cells.
                    let cell = self.parser.screen().cell(row, col);
                    if cell.is_some_and(vt100::Cell::is_wide_continuation) {
                        continue;
                    }
                    let text = cell.map_or(" ", |cell| {
                        if cell.contents().is_empty() {
                            " "
                        } else {
                            cell.contents()
                        }
                    });
                    let width = cell.map_or(1, |cell| if cell.is_wide() { 2 } else { 1 });
                    if cell.is_some_and(vt100::Cell::has_contents) {
                        written_end = (col + width).min(self.size.cols);
                    }
                    let style = match cell {
                        Some(cell) => Style {
                            foreground: color(cell.fgcolor()),
                            background: color(cell.bgcolor()),
                            bold: cell.bold(),
                            italic: cell.italic(),
                            underline: cell.underline(),
                            inverse: cell.inverse(),
                        },
                        None => Style {
                            foreground: Color::Default,
                            background: Color::Default,
                            bold: false,
                            italic: false,
                            underline: false,
                            inverse: false,
                        },
                    };
                    // ASCII runs share a draw call; isolate Unicode glyphs at their actual grid origin.
                    if text.is_ascii()
                        && let Some(last) = runs.last_mut()
                        && last.text.is_ascii()
                        && last.style == style
                        && last.column + last.width == col
                    {
                        last.text.push_str(text);
                        last.width += width;
                    } else {
                        runs.push(Run {
                            column: col,
                            width,
                            text: text.into(),
                            style,
                        });
                    }
                }
                (
                    runs,
                    RowCopy {
                        wraps_to_next: self.parser.screen().row_wrapped(row),
                        written_end,
                    },
                )
            })
            .unzip()
    }

    fn links(&self) -> Vec<LinkSpan> {
        let screen = self.parser.screen();
        let mut links = Vec::new();
        let mut retained_bytes = 0;

        // Preserve explicit OSC 8 targets exactly. Only HTTP(S) destinations enter the native
        // click surface; the desktop still applies the authoritative URL policy at admission.
        for row in 0..self.size.rows {
            let mut active: Option<(u16, u16, String)> = None;
            for column in 0..self.size.cols {
                let cell = screen.cell(row, column);
                if cell.is_some_and(vt100::Cell::is_wide_continuation) {
                    continue;
                }
                let target = screen
                    .cell_hyperlink(row, column)
                    .filter(|url| has_http_scheme(url));
                let width = cell.map_or(1, |cell| if cell.is_wide() { 2 } else { 1 });
                match (active.as_mut(), target) {
                    (Some((_, end, current)), Some(url)) if current == url && *end == column => {
                        *end = column.saturating_add(width).min(self.size.cols);
                    }
                    (Some(_), _) => {
                        push_active_link(&mut links, &mut retained_bytes, row, active.take());
                        if let Some(url) = target {
                            active = Some((
                                column,
                                column.saturating_add(width).min(self.size.cols),
                                url.to_owned(),
                            ));
                        }
                    }
                    (None, Some(url)) => {
                        active = Some((
                            column,
                            column.saturating_add(width).min(self.size.cols),
                            url.to_owned(),
                        ));
                    }
                    (None, None) => {}
                }
            }
            push_active_link(&mut links, &mut retained_bytes, row, active.take());
            if links.len() >= MAX_LINK_SPANS || retained_bytes >= MAX_LINK_SPAN_BYTES {
                return links;
            }
        }

        // Plain URL recognition follows each logical line across terminal autowrap, while each
        // returned span remains in physical-row cell coordinates for precise hit testing.
        let mut logical_line = Vec::new();
        for row in 0..self.size.rows {
            if row > 0 && !screen.row_wrapped(row - 1) {
                scan_plain_links(&logical_line, &mut links, &mut retained_bytes);
                logical_line.clear();
            }
            let written_end = (0..self.size.cols)
                .rev()
                .find(|column| {
                    screen
                        .cell(row, *column)
                        .is_some_and(vt100::Cell::has_contents)
                })
                .map_or(0, |column| column + 1);
            for column in 0..written_end {
                let Some(cell) = screen.cell(row, column) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let width = if cell.is_wide() { 2 } else { 1 };
                let explicit = screen.cell_hyperlink(row, column).is_some();
                let text = if cell.contents().is_empty() {
                    " "
                } else {
                    cell.contents()
                };
                for character in text.chars() {
                    logical_line.push(MappedChar {
                        character,
                        row,
                        column,
                        width,
                        explicit,
                    });
                }
            }
            if !screen.row_wrapped(row) {
                scan_plain_links(&logical_line, &mut links, &mut retained_bytes);
                logical_line.clear();
            }
            if links.len() >= MAX_LINK_SPANS || retained_bytes >= MAX_LINK_SPAN_BYTES {
                return links;
            }
        }
        if !logical_line.is_empty() {
            scan_plain_links(&logical_line, &mut links, &mut retained_bytes);
        }
        links
    }

    /// Only the visible screen crosses the bridge, never a clone of complete scrollback.
    pub fn viewport(&self) -> Viewport {
        let (cursor_row, cursor_col) = self.parser.screen().cursor_position();
        let (lines, row_copy) = self.runs();
        Viewport {
            viewport_generation: self.viewport_generation,
            links: self.links(),
            notices: self.parser.callbacks().notices.clone(),
            title: self.parser.callbacks().title.clone(),
            working_directory: self.parser.callbacks().working_directory.clone(),
            size: self.size,
            contents: self.parser.screen().contents(),
            cursor_row,
            cursor_col,
            lines,
            row_copy: Some(row_copy),
            pointer_tracking: Some(match self.parser.screen().mouse_protocol_mode() {
                vt100::MouseProtocolMode::None => PointerTracking::None,
                vt100::MouseProtocolMode::Press => PointerTracking::Press,
                vt100::MouseProtocolMode::PressRelease => PointerTracking::PressRelease,
                vt100::MouseProtocolMode::ButtonMotion => PointerTracking::ButtonMotion,
                vt100::MouseProtocolMode::AnyMotion => PointerTracking::AnyMotion,
            }),
            application_cursor: self.parser.screen().application_cursor(),
            bracketed_paste: self.parser.screen().bracketed_paste(),
            hide_cursor: self.parser.screen().hide_cursor(),
            scrollback_offset: self.parser.screen().scrollback(),
            scrollback_lines: self.scrollback_lines,
            alternate_screen: self.parser.screen().alternate_screen(),
        }
    }
}

#[cfg(feature = "ghostty")]
pub(crate) fn plain_links_from_runs(lines: &[Vec<Run>]) -> Vec<LinkSpan> {
    let mut links = Vec::new();
    let mut retained_bytes = 0;
    for (row, runs) in lines.iter().enumerate() {
        let Ok(row) = u16::try_from(row) else {
            break;
        };
        let mut mapped = Vec::new();
        let mut next_column = 0;
        for run in runs {
            while next_column < run.column {
                mapped.push(MappedChar {
                    character: ' ',
                    row,
                    column: next_column,
                    width: 1,
                    explicit: false,
                });
                next_column += 1;
            }
            if run.text.is_ascii() {
                for (offset, character) in run.text.chars().enumerate() {
                    let Ok(offset) = u16::try_from(offset) else {
                        break;
                    };
                    mapped.push(MappedChar {
                        character,
                        row,
                        column: run.column.saturating_add(offset),
                        width: 1,
                        explicit: false,
                    });
                }
            } else {
                for character in run.text.chars() {
                    mapped.push(MappedChar {
                        character,
                        row,
                        column: run.column,
                        width: run.width,
                        explicit: false,
                    });
                }
            }
            next_column = run.column.saturating_add(run.width);
        }
        scan_plain_links(&mapped, &mut links, &mut retained_bytes);
        if links.len() >= MAX_LINK_SPANS || retained_bytes >= MAX_LINK_SPAN_BYTES {
            break;
        }
    }
    links
}

fn color(value: vt100::Color) -> Color {
    match value {
        vt100::Color::Default => Color::Default,
        vt100::Color::Idx(index) => Color::Indexed(index),
        vt100::Color::Rgb(r, g, b) => Color::Rgb([r, g, b]),
    }
}

#[derive(Clone, Copy)]
struct MappedChar {
    character: char,
    row: u16,
    column: u16,
    width: u16,
    explicit: bool,
}

fn push_active_link(
    links: &mut Vec<LinkSpan>,
    retained_bytes: &mut usize,
    row: u16,
    active: Option<(u16, u16, String)>,
) {
    let Some((start_column, end_column, url)) = active else {
        return;
    };
    if links.len() >= MAX_LINK_SPANS
        || retained_bytes.saturating_add(url.len()) > MAX_LINK_SPAN_BYTES
    {
        return;
    }
    push_link(links, retained_bytes, row, start_column, end_column, &url);
}

fn has_http_scheme(url: &str) -> bool {
    url.get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
        || url
            .get(..8)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

fn decode_open_nonce(value: &[u8]) -> Option<[u8; 16]> {
    if value.len() != 32 {
        return None;
    }
    let mut nonce = [0; 16];
    for (index, pair) in value.chunks_exact(2).enumerate() {
        let high = lower_hex(pair[0])?;
        let low = lower_hex(pair[1])?;
        nonce[index] = (high << 4) | low;
    }
    Some(nonce)
}

fn lower_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn decode_base64url(value: &[u8]) -> Option<Vec<u8>> {
    if value.is_empty() || value.len() > MAX_PROGRAM_OPEN_URL_BASE64_BYTES || value.len() % 4 == 1 {
        return None;
    }
    let remainder = value.len() % 4;
    let decoded_length = (value.len() / 4) * 3
        + match remainder {
            0 => 0,
            2 => 1,
            3 => 2,
            _ => return None,
        };
    if decoded_length > MAX_PROGRAM_OPEN_URL_BYTES {
        return None;
    }
    let mut decoded = Vec::new();
    decoded.try_reserve_exact(decoded_length).ok()?;
    let mut offset = 0;
    while value.len() - offset >= 4 {
        let a = base64url_value(value[offset])?;
        let b = base64url_value(value[offset + 1])?;
        let c = base64url_value(value[offset + 2])?;
        let d = base64url_value(value[offset + 3])?;
        decoded.extend_from_slice(&[(a << 2) | (b >> 4), (b << 4) | (c >> 2), (c << 6) | d]);
        offset += 4;
    }
    match value.len() - offset {
        0 => {}
        2 => {
            let a = base64url_value(value[offset])?;
            let b = base64url_value(value[offset + 1])?;
            if b & 0x0f != 0 {
                return None;
            }
            decoded.push((a << 2) | (b >> 4));
        }
        3 => {
            let a = base64url_value(value[offset])?;
            let b = base64url_value(value[offset + 1])?;
            let c = base64url_value(value[offset + 2])?;
            if c & 0x03 != 0 {
                return None;
            }
            decoded.extend_from_slice(&[(a << 2) | (b >> 4), (b << 4) | (c >> 2)]);
        }
        _ => return None,
    }
    (decoded.len() == decoded_length).then_some(decoded)
}

fn base64url_value(value: u8) -> Option<u8> {
    match value {
        b'A'..=b'Z' => Some(value - b'A'),
        b'a'..=b'z' => Some(value - b'a' + 26),
        b'0'..=b'9' => Some(value - b'0' + 52),
        b'-' => Some(62),
        b'_' => Some(63),
        _ => None,
    }
}

fn scheme_length(chars: &[MappedChar], start: usize) -> Option<usize> {
    fn matches(chars: &[MappedChar], start: usize, pattern: &str) -> bool {
        chars
            .get(start..start + pattern.len())
            .is_some_and(|actual| {
                actual
                    .iter()
                    .zip(pattern.chars())
                    .all(|(actual, expected)| actual.character.eq_ignore_ascii_case(&expected))
            })
    }
    if matches(chars, start, "https://") {
        Some(8)
    } else if matches(chars, start, "http://") {
        Some(7)
    } else {
        None
    }
}

fn url_start_boundary(chars: &[MappedChar], start: usize) -> bool {
    start == 0
        || !chars[start - 1].character.is_ascii_alphanumeric()
            && !matches!(chars[start - 1].character, '+' | '-' | '.')
}

fn is_url_delimiter(character: char) -> bool {
    character.is_whitespace()
        || character.is_control()
        || matches!(character, '<' | '>' | '"' | '\'' | '`')
}

fn is_trailing_url_punctuation(character: char) -> bool {
    matches!(character, '.' | ',' | ';' | ':' | '!' | '?')
}

fn wrapper_delimiter(character: char) -> Option<(usize, bool)> {
    match character {
        '(' => Some((0, true)),
        ')' => Some((0, false)),
        '[' => Some((1, true)),
        ']' => Some((1, false)),
        '{' => Some((2, true)),
        '}' => Some((2, false)),
        _ => None,
    }
}

fn trimmed_plain_url_end(chars: &[MappedChar], start: usize, raw_end: usize) -> usize {
    let mut open_delimiters = [0; 3];
    let mut trim_start = raw_end;
    let mut trimming_suffix = false;

    // Keep paired path delimiters, while treating unmatched closing wrappers and the existing
    // terminal punctuation set as removable suffix characters.
    for (offset, mapped) in chars[start..raw_end].iter().enumerate() {
        let position = start + offset;
        let unmatched_closer = match wrapper_delimiter(mapped.character) {
            Some((kind, true)) => {
                open_delimiters[kind] += 1;
                false
            }
            Some((kind, false)) if open_delimiters[kind] > 0 => {
                open_delimiters[kind] -= 1;
                false
            }
            Some((_, false)) => true,
            None => false,
        };
        if is_trailing_url_punctuation(mapped.character) || unmatched_closer {
            if !trimming_suffix {
                trim_start = position;
            }
            trimming_suffix = true;
        } else {
            trim_start = position + 1;
            trimming_suffix = false;
        }
    }

    trim_start
}

fn scan_plain_links(chars: &[MappedChar], links: &mut Vec<LinkSpan>, retained_bytes: &mut usize) {
    let mut index = 0;
    while index < chars.len()
        && links.len() < MAX_LINK_SPANS
        && *retained_bytes < MAX_LINK_SPAN_BYTES
    {
        let Some(scheme_length) = scheme_length(chars, index) else {
            index += 1;
            continue;
        };
        if !url_start_boundary(chars, index) {
            index += scheme_length;
            continue;
        }
        let mut end = index + scheme_length;
        while end < chars.len() && !is_url_delimiter(chars[end].character) {
            end += 1;
        }
        let raw_end = end;
        end = trimmed_plain_url_end(chars, index, raw_end);
        if end <= index + scheme_length || chars[index..end].iter().any(|mapped| mapped.explicit) {
            index = raw_end.max(index + scheme_length);
            continue;
        }
        let byte_length = chars[index..end]
            .iter()
            .map(|mapped| mapped.character.len_utf8())
            .sum::<usize>();
        if byte_length > MAX_PROGRAM_OPEN_URL_BYTES {
            index = raw_end.max(index + scheme_length);
            continue;
        }
        let mut url = String::with_capacity(byte_length);
        for mapped in &chars[index..end] {
            url.push(mapped.character);
        }
        if !has_http_scheme(&url) {
            index = raw_end.max(index + scheme_length);
            continue;
        }
        push_mapped_spans(&chars[index..end], url, links, retained_bytes);
        index = raw_end.max(index + scheme_length);
    }
}

fn push_mapped_spans(
    mapped: &[MappedChar],
    url: String,
    links: &mut Vec<LinkSpan>,
    retained_bytes: &mut usize,
) {
    let mut active: Option<(u16, u16, u16)> = None;
    for cell in mapped {
        let cell_end = cell.column.saturating_add(cell.width);
        match active.as_mut() {
            Some((row, _, end)) if *row == cell.row && cell.column <= *end => {
                *end = (*end).max(cell_end);
            }
            Some(_) => {
                if let Some((row, start_column, end_column)) = active.take() {
                    push_link(links, retained_bytes, row, start_column, end_column, &url);
                }
                active = Some((cell.row, cell.column, cell_end));
            }
            None => active = Some((cell.row, cell.column, cell_end)),
        }
    }
    if let Some((row, start_column, end_column)) = active {
        push_link(links, retained_bytes, row, start_column, end_column, &url);
    }
}

fn push_link(
    links: &mut Vec<LinkSpan>,
    retained_bytes: &mut usize,
    row: u16,
    start_column: u16,
    end_column: u16,
    url: &str,
) {
    if links.len() >= MAX_LINK_SPANS
        || retained_bytes.saturating_add(url.len()) > MAX_LINK_SPAN_BYTES
    {
        return;
    }
    links.push(LinkSpan {
        row,
        start_column,
        end_column,
        url: url.to_owned(),
    });
    *retained_bytes += url.len();
}

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

#[derive(Clone, Debug, Serialize)]
pub struct Run {
    pub column: u16,
    pub width: u16,
    pub text: String,
    pub style: Style,
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
    pub notices: Vec<TerminalNotice>,
    pub title: String,
    pub working_directory: Option<String>,
    pub size: Size,
    pub contents: String,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub lines: Vec<Vec<Run>>,
    pub row_copy: Option<Vec<RowCopy>>,
    pub application_cursor: bool,
    pub bracketed_paste: bool,
    pub hide_cursor: bool,
    pub scrollback_offset: usize,
    pub scrollback_lines: usize,
    pub alternate_screen: bool,
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
    _worker_owned: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[derive(Default)]
struct Metadata {
    title: String,
    notices: Vec<TerminalNotice>,
    next_notice: u64,
    working_directory: Option<String>,
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
        let size = size.validate()?;
        Ok(Self {
            parser: vt100::Parser::new_with_callbacks(
                size.rows,
                size.cols,
                MAX_SCROLLBACK_LINES,
                Metadata::default(),
            ),
            size,
            control: ControlMode::Ground,
            failed: false,
            scrollback_lines: 0,
            _worker_owned: std::marker::PhantomData,
        })
    }

    pub fn process(&mut self, bytes: &[u8]) -> Result<(), TerminalError> {
        if self.failed {
            return Err(TerminalError::ControlStringTooLong);
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
        Ok(())
    }

    pub fn resize(&mut self, size: Size) -> Result<(), InvalidSize> {
        let size = size.validate()?;
        self.parser.screen_mut().set_size(size.rows, size.cols);
        self.size = size;
        self.measure_history();
        Ok(())
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
    pub fn scroll(&mut self, lines: i32) {
        let screen = self.parser.screen_mut();
        let offset = if lines == 0 {
            0
        } else {
            screen
                .scrollback()
                .saturating_add_signed(lines.clamp(-1000, 1000) as isize)
        };
        screen.set_scrollback(offset);
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
            self.scroll(0);
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
            self.scroll(event.lines);
            Vec::new()
        }
    }

    fn runs(&self) -> (Vec<Vec<Run>>, Vec<RowCopy>) {
        (0..self.size.rows)
            .map(|row| {
                let mut runs: Vec<Run> = Vec::new();
                let mut written_end = 0;
                for col in 0..self.size.cols {
                    let cell = self
                        .parser
                        .screen()
                        .cell(row, col)
                        .expect("validated screen geometry");
                    if cell.is_wide_continuation() {
                        continue;
                    }
                    let text = if cell.contents().is_empty() {
                        " "
                    } else {
                        cell.contents()
                    };
                    let width = if cell.is_wide() { 2 } else { 1 };
                    if cell.has_contents() {
                        written_end = (col + width).min(self.size.cols);
                    }
                    let style = Style {
                        foreground: color(cell.fgcolor()),
                        background: color(cell.bgcolor()),
                        bold: cell.bold(),
                        italic: cell.italic(),
                        underline: cell.underline(),
                        inverse: cell.inverse(),
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

    /// Only the visible screen crosses the bridge, never a clone of complete scrollback.
    pub fn viewport(&self) -> Viewport {
        let (cursor_row, cursor_col) = self.parser.screen().cursor_position();
        let (lines, row_copy) = self.runs();
        Viewport {
            notices: self.parser.callbacks().notices.clone(),
            title: self.parser.callbacks().title.clone(),
            working_directory: self.parser.callbacks().working_directory.clone(),
            size: self.size,
            contents: self.parser.screen().contents(),
            cursor_row,
            cursor_col,
            lines,
            row_copy: Some(row_copy),
            application_cursor: self.parser.screen().application_cursor(),
            bracketed_paste: self.parser.screen().bracketed_paste(),
            hide_cursor: self.parser.screen().hide_cursor(),
            scrollback_offset: self.parser.screen().scrollback(),
            scrollback_lines: self.scrollback_lines,
            alternate_screen: self.parser.screen().alternate_screen(),
        }
    }
}

fn color(value: vt100::Color) -> Color {
    match value {
        vt100::Color::Default => Color::Default,
        vt100::Color::Idx(index) => Color::Indexed(index),
        vt100::Color::Rgb(r, g, b) => Color::Rgb([r, g, b]),
    }
}

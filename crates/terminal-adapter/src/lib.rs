//! Headless parser feasibility adapter, not a production renderer or full terminal engine.

use serde::Serialize;

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
pub struct Viewport {
    pub title: String,
    pub working_directory: Option<String>,
    pub size: Size,
    pub contents: String,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub lines: Vec<Vec<Run>>,
    pub application_cursor: bool,
    pub bracketed_paste: bool,
    pub hide_cursor: bool,
}

#[derive(Default)]
enum ControlMode {
    #[default]
    Ground,
    Escape,
    String(usize),
}

pub struct Terminal {
    parser: vt100::Parser<Metadata>,
    size: Size,
    control: ControlMode,
    failed: bool,
}

#[derive(Default)]
struct Metadata {
    title: String,
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

impl vt100::Callbacks for Metadata {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = display_text(title, 256);
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
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
            // This is display metadata only: never open a path or execute shell content.
            self.working_directory = Some(display_text(&decoded, 2048));
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
        Ok(())
    }

    pub fn resize(&mut self, size: Size) -> Result<(), InvalidSize> {
        let size = size.validate()?;
        self.parser.screen_mut().set_size(size.rows, size.cols);
        self.size = size;
        Ok(())
    }

    fn runs(&self) -> Vec<Vec<Run>> {
        (0..self.size.rows)
            .map(|row| {
                let mut runs: Vec<Run> = Vec::new();
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
                runs
            })
            .collect()
    }

    /// Only the visible screen crosses the bridge, never a clone of complete scrollback.
    pub fn viewport(&self) -> Viewport {
        let (cursor_row, cursor_col) = self.parser.screen().cursor_position();
        Viewport {
            title: self.parser.callbacks().title.clone(),
            working_directory: self.parser.callbacks().working_directory.clone(),
            size: self.size,
            contents: self.parser.screen().contents(),
            cursor_row,
            cursor_col,
            lines: self.runs(),
            application_cursor: self.parser.screen().application_cursor(),
            bracketed_paste: self.parser.screen().bracketed_paste(),
            hide_cursor: self.parser.screen().hide_cursor(),
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

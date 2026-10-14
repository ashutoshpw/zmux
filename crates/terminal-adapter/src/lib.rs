//! Headless parser feasibility adapter, not a production renderer or full terminal engine.

use serde::Serialize;
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
    pub size: Size,
    pub contents: String,
    pub cursor_row: u16,
    pub cursor_col: u16,
}

#[derive(Default)]
enum ControlMode {
    #[default]
    Ground,
    Escape,
    String(usize),
}

pub struct Terminal {
    parser: vt100::Parser,
    size: Size,
    control: ControlMode,
    failed: bool,
}

impl Terminal {
    pub fn new(size: Size) -> Result<Self, InvalidSize> {
        let size = size.validate()?;
        Ok(Self {
            parser: vt100::Parser::new(size.rows, size.cols, MAX_SCROLLBACK_LINES),
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

    /// Only the visible screen crosses the bridge, never a clone of complete scrollback.
    pub fn viewport(&self) -> Viewport {
        let (cursor_row, cursor_col) = self.parser.screen().cursor_position();
        Viewport {
            size: self.size,
            contents: self.parser.screen().contents(),
            cursor_row,
            cursor_col,
        }
    }
}

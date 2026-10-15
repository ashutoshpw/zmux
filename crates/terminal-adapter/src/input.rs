//! Semantic input is encoded by the terminal owner, never a rendered frame.
use crate::Terminal;
use serde::Deserialize;

pub const PASTE_BYTES: usize = 16_384;
pub const ENCODED_PASTE_BYTES: usize = PASTE_BYTES + 12;

/// Stable original host/FFI key IDs; these are not upstream enum discriminants.
#[derive(Clone, Copy, Debug, Deserialize)]
#[repr(u32)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    ArrowLeft = 1,
    ArrowRight,
    ArrowDown,
    ArrowUp,
    Home,
    End,
    Delete,
    PageUp,
    PageDown,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Enter,
    NumpadEnter,
    Backspace,
    Escape,
    Tab,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum KeyAction {
    Release = 0,
    #[default]
    Press = 1,
    Repeat = 2,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyEvent {
    pub key: Key,
    #[serde(default)]
    pub action: KeyAction,
    /// Shift=1, Option/Alt=2, Control=4. Command shortcuts stay with the host.
    #[serde(default)]
    pub modifiers: u8,
    /// The old native path interpreted ASCII Control text before Enter, Tab,
    /// Backspace and Escape. Preserve that host text only in the default backend;
    /// the candidate uses its actual key protocol encoder instead.
    #[serde(default)]
    pub legacy_control: Option<u8>,
}
impl KeyEvent {
    pub fn valid(self) -> bool {
        self.modifiers & !7 == 0
            && self.legacy_control.is_none_or(|byte| {
                byte <= 31
                    && self.modifiers & 4 != 0
                    && matches!(
                        self.key,
                        Key::Enter | Key::NumpadEnter | Key::Backspace | Key::Escape | Key::Tab
                    )
            })
    }
}

impl Terminal {
    pub fn encode_key(&self, event: KeyEvent) -> Vec<u8> {
        if event.action == KeyAction::Release {
            return Vec::new();
        }
        if let Some(byte) = event.legacy_control {
            return vec![byte];
        }
        let modifier = 1 + event.modifiers;
        let cursor = match event.key {
            Key::ArrowLeft => Some("D"),
            Key::ArrowRight => Some("C"),
            Key::ArrowDown => Some("B"),
            Key::ArrowUp => Some("A"),
            Key::Home => Some("H"),
            Key::End => Some("F"),
            _ => None,
        };
        if let Some(final_) = cursor {
            return if modifier > 1 {
                format!("\x1b[1;{modifier}{final_}")
            } else {
                format!(
                    "\x1b{}{final_}",
                    if self.parser.screen().application_cursor() {
                        "O"
                    } else {
                        "["
                    }
                )
            }
            .into_bytes();
        }
        let function = match event.key {
            Key::F1 => Some("P"),
            Key::F2 => Some("Q"),
            Key::F3 => Some("R"),
            Key::F4 => Some("S"),
            _ => None,
        };
        if let Some(final_) = function {
            return if modifier > 1 {
                format!("\x1b[1;{modifier}{final_}")
            } else {
                format!("\x1bO{final_}")
            }
            .into_bytes();
        }
        let tilde = match event.key {
            Key::Delete => Some(3),
            Key::PageUp => Some(5),
            Key::PageDown => Some(6),
            Key::F5 => Some(15),
            Key::F6 => Some(17),
            Key::F7 => Some(18),
            Key::F8 => Some(19),
            Key::F9 => Some(20),
            Key::F10 => Some(21),
            Key::F11 => Some(23),
            Key::F12 => Some(24),
            _ => None,
        };
        if let Some(number) = tilde {
            return if modifier > 1 {
                format!("\x1b[{number};{modifier}~")
            } else {
                format!("\x1b[{number}~")
            }
            .into_bytes();
        }
        match event.key {
            Key::Enter | Key::NumpadEnter => b"\r".to_vec(),
            Key::Backspace => b"\x7f".to_vec(),
            Key::Escape => b"\x1b".to_vec(),
            Key::Tab if event.modifiers & 1 != 0 => b"\x1b[Z".to_vec(),
            Key::Tab => b"\t".to_vec(),
            _ => unreachable!("all semantic special keys handled"),
        }
    }

    /// Preserve the existing default host paste transformations, using live modes.
    /// The optional candidate's pinned encoder additionally strips unsafe controls.
    pub fn encode_paste(&self, text: &str) -> Vec<u8> {
        if self.parser.screen().bracketed_paste() {
            format!("\x1b[200~{text}\x1b[201~").into_bytes()
        } else {
            text.replace("\r\n", "\n").replace('\n', "\r").into_bytes()
        }
    }
}

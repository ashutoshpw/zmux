//! Semantic pointer reports encoded only against the worker-owned parser.
use crate::Terminal;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerAction {
    Press,
    Release,
    Motion,
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerButton {
    Left,
    Middle,
    Right,
    None,
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pointer {
    pub action: PointerAction,
    pub button: PointerButton,
    pub row: u16,
    pub col: u16,
    pub modifiers: u8,
}
impl Terminal {
    pub fn pointer(&mut self, event: Pointer) -> Vec<u8> {
        use vt100::MouseProtocolMode as Mode;
        let mode = self.parser.screen().mouse_protocol_mode();
        let button = match event.button {
            PointerButton::Left => 0,
            PointerButton::Middle => 1,
            PointerButton::Right => 2,
            PointerButton::None => 3,
        };
        let release = matches!(event.action, PointerAction::Release);
        let motion = matches!(event.action, PointerAction::Motion);
        if mode == Mode::None
            || (button == 3 && !motion)
            || (release && mode == Mode::Press)
            || (motion && mode != Mode::AnyMotion && (mode != Mode::ButtonMotion || button == 3))
        {
            return Vec::new();
        }
        let modifiers = if mode == Mode::Press {
            0
        } else {
            event.modifiers & 28
        };
        let code = button + modifiers + if motion { 32 } else { 0 };
        let report = self.mouse_report(code, event.row, event.col, release);
        if self.scroll(0).is_err() {
            return Vec::new();
        }
        report
    }
    pub(crate) fn mouse_report(&self, code: u8, row: u16, col: u16, release: bool) -> Vec<u8> {
        let x = col.min(self.size.cols - 1);
        let y = row.min(self.size.rows - 1);
        if self.parser.screen().mouse_protocol_encoding() == vt100::MouseProtocolEncoding::Sgr {
            return format!(
                "\x1b[<{code};{};{}{}",
                x + 1,
                y + 1,
                if release { 'm' } else { 'M' }
            )
            .into_bytes();
        }
        // Legacy release identifies no button; SGR retains the actual button.
        let code = if release { (code & 28) | 3 } else { code };
        match self.parser.screen().mouse_protocol_encoding() {
            vt100::MouseProtocolEncoding::Default => vec![
                27,
                b'[',
                b'M',
                code + 32,
                x.min(222) as u8 + 33,
                y.min(222) as u8 + 33,
            ],
            vt100::MouseProtocolEncoding::Utf8 => {
                let mut text = String::from("\x1b[M");
                for scalar in [u32::from(code + 32), u32::from(x + 33), u32::from(y + 33)] {
                    text.push(char::from_u32(scalar).expect("bounded mouse coordinate"));
                }
                text.into_bytes()
            }
            vt100::MouseProtocolEncoding::Sgr => unreachable!("handled above"),
        }
    }
}

//! A reply of the terminal that comes after the query for its background
//! gave up (see [`crate::theme::query_background`]) reaches the input as
//! keys: crossterm reads `ESC ] 11 ; rgb:… ESC \` as Alt-`]`, the characters
//! of the reply, and Alt-`\` (or Ctrl-g for a BEL). Those keys are dropped so
//! that they do not count, move or open the command line.

use super::*;

/// The longest reply swallowed: `11;rgba:ffff/ffff/ffff/ffff` and some.
const LONGEST: usize = 48;

impl Session {
    /// Whether `key` belongs to a late reply of the terminal and is dropped.
    /// A key that cannot be part of one ends the reply and is a key again.
    pub(super) fn swallow_late_reply(&mut self, key: &KeyEvent) -> bool {
        let plain = key.modifiers - KeyModifiers::SHIFT == KeyModifiers::NONE;
        match (key.code, self.late_reply) {
            (KeyCode::Char(']'), _) if key.modifiers == KeyModifiers::ALT => {
                self.late_reply = Some(0);
                true
            }
            (KeyCode::Char('\\'), Some(_)) if key.modifiers == KeyModifiers::ALT => {
                self.late_reply = None;
                true
            }
            (KeyCode::Char('g'), Some(_)) if key.modifiers == KeyModifiers::CONTROL => {
                self.late_reply = None;
                true
            }
            (KeyCode::Char(c), Some(n))
                if plain && n < LONGEST && (c.is_ascii_alphanumeric() || ";:/?".contains(c)) =>
            {
                self.late_reply = Some(n + 1);
                true
            }
            _ => {
                self.late_reply = None;
                false
            }
        }
    }
}

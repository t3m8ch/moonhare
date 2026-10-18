//! A reply of the terminal that comes after the query for its background
//! gave up (see [`crate::theme::query_background`]) reaches the input as
//! keys: crossterm reads `ESC ] 11 ; rgb:… ESC \` as Alt-`]`, the characters
//! of the reply, and Alt-`\` (or Ctrl-g for a BEL). Those keys are held back
//! while they spell such a reply and dropped once it ends.
//!
//! The Reviewer types the same keys too: tmux sends Esc and a quick `]` as
//! Alt-`]`. So keys are held only while a reply may still come (the query
//! gave up, and for [`WINDOW`] after), only while they match the reply
//! exactly, and only while they come in a burst, as a reply does; otherwise
//! the held keys are given back in the order they came.

use std::time::Duration;

use super::*;

/// How long after the session opens a late reply is looked for.
const WINDOW: Duration = Duration::from_secs(3);
/// The longest pause between two keys of one reply.
const GAP: Duration = Duration::from_millis(100);
/// The longest reply held: `11;rgba:ffff/ffff/ffff/ffff` and some.
const LONGEST: usize = 48;
/// How every reply starts after `ESC ]`.
const PREFIX: &str = "11;rgb";

#[derive(Default)]
pub(super) struct LateReply {
    /// Until when a reply may start; `None` when none is expected.
    until: Option<Duration>,
    /// The keys held so far, Alt-`]` first.
    held: Vec<KeyEvent>,
    /// When the last of them came.
    last: Duration,
}

/// Where a key leaves the reply being held.
enum Fit {
    /// One more key of it.
    More,
    /// Its end: drop it all.
    End,
    /// Not a reply after all.
    Not,
}

impl LateReply {
    fn fit(&self, key: &KeyEvent) -> Fit {
        let at = self.held.len() - 1;
        let plain = key.modifiers - KeyModifiers::SHIFT == KeyModifiers::NONE;
        match key.code {
            KeyCode::Char('\\') if key.modifiers == KeyModifiers::ALT && at > PREFIX.len() => {
                Fit::End
            }
            KeyCode::Char('g') if key.modifiers == KeyModifiers::CONTROL && at > PREFIX.len() => {
                Fit::End
            }
            KeyCode::Char(c) if plain && at < LONGEST => {
                let fits = match PREFIX.as_bytes().get(at) {
                    Some(&want) => c == want as char,
                    None => c.is_ascii_hexdigit() || c == ':' || c == '/',
                };
                if fits {
                    Fit::More
                } else {
                    Fit::Not
                }
            }
            _ => Fit::Not,
        }
    }
}

impl Session {
    /// The query for the terminal background gave up: its reply may still
    /// come, as keys, for a while.
    pub fn expect_late_reply(&mut self) {
        self.late_reply.until = Some(self.clock() + WINDOW);
    }

    /// The keys to act on now that `key` came: none while a reply is held,
    /// the held keys and `key` when it turns out to be none.
    pub(super) fn through_late_reply(&mut self, key: KeyEvent) -> Vec<KeyEvent> {
        let now = self.clock();
        let mut out = Vec::new();
        let reply = &mut self.late_reply;
        if !reply.held.is_empty() {
            if now.saturating_sub(reply.last) <= GAP {
                match reply.fit(&key) {
                    Fit::More => {
                        reply.held.push(key);
                        reply.last = now;
                        return out;
                    }
                    Fit::End => {
                        reply.held.clear();
                        return out;
                    }
                    Fit::Not => {}
                }
            }
            out.append(&mut reply.held);
        }
        let open = reply.until.is_some_and(|until| now < until);
        if !open {
            reply.until = None;
        }
        if open && key.code == KeyCode::Char(']') && key.modifiers == KeyModifiers::ALT {
            reply.held.push(key);
            reply.last = now;
        } else {
            out.push(key);
        }
        out
    }

    /// Give the held keys back once the pause after them is too long for a
    /// reply. Whether there were any.
    pub(super) fn late_reply_elapse(&mut self) -> bool {
        let now = self.clock();
        let reply = &mut self.late_reply;
        if reply.held.is_empty() || now.saturating_sub(reply.last) <= GAP {
            return false;
        }
        for key in std::mem::take(&mut reply.held) {
            self.dispatch(Event::Key(key));
        }
        true
    }

    /// How long until held keys are given back, if any are held.
    pub(super) fn late_reply_due(&self) -> Option<Duration> {
        let reply = &self.late_reply;
        (!reply.held.is_empty())
            .then(|| (reply.last + GAP + Duration::from_millis(1)).saturating_sub(self.clock()))
    }
}

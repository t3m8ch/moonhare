//! Moving between windows: `Ctrl-w` + `h/j/k/l/w`, `Tab`, and the
//! `Ctrl-h/j/k/l` aliases. Plain `hjkl` never change the window.

use ratatui::crossterm::event::{KeyCode, KeyModifiers};

use super::*;
use crate::hints::CTRL_W;

impl Session {
    /// Handle a window-switching key; returns whether it was one.
    pub(super) fn window_key(&mut self, code: KeyCode, mods: KeyModifiers) -> bool {
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        if self.pending == Some(CTRL_W) {
            self.pending = None;
            self.count = None;
            if let KeyCode::Char(c) = code {
                if !self.sidebar_key(c) {
                    self.go_window(c);
                }
            }
            return true;
        }
        if self.pending.is_some() {
            return false;
        }
        match code {
            KeyCode::Char('w') if ctrl => self.pending = Some(CTRL_W),
            KeyCode::Char(c @ ('h' | 'j' | 'k' | 'l')) if ctrl => self.go_window(c),
            KeyCode::Tab => self.go_window('w'),
            KeyCode::BackTab => self.go_window('W'),
            _ => return false,
        }
        true
    }

    /// The window that has the focus: the commit list only while the sidebar
    /// is shown.
    pub(super) fn window(&self) -> Focus {
        if self.sidebar_visible() {
            self.focus
        } else {
            Focus::Diff
        }
    }

    /// Windows are the commit list and the file list (the left column, in
    /// that order) and the diff. `w` and `W` go round them, forward and back.
    fn go_window(&mut self, direction: char) {
        if !self.sidebar_visible() {
            self.focus = Focus::Diff;
            return;
        }
        self.focus = match (direction, self.focus) {
            ('w', Focus::Diff) | ('W', Focus::Files) => Focus::Commits,
            ('w', Focus::Commits) | ('W', Focus::Diff) => Focus::Files,
            ('w', Focus::Files) | ('W', Focus::Commits) => Focus::Diff,
            ('h' | 'k', Focus::Diff | Focus::Files) => Focus::Commits,
            ('j', Focus::Commits) => Focus::Files,
            ('l', _) => Focus::Diff,
            (_, focus) => focus,
        };
    }
}

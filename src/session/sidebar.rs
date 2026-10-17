//! The width of the sidebar, changed during the session: `Ctrl-w <` / `>` /
//! `=`, `:sidebar [N]` and dragging the border with the mouse.

use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use super::*;

/// The sidebar never gets narrower than this: it stays readable.
const MIN_WIDTH: u16 = 16;
/// The columns left to the diff at least.
const DIFF_MIN_WIDTH: u16 = 40;
/// Columns moved by one `Ctrl-w <` / `>`.
const STEP: u16 = 4;

impl Session {
    /// The chosen width kept inside the bounds of the current terminal.
    pub(super) fn custom_sidebar_width(&self, chosen: u16) -> u16 {
        let max = self.size.0.saturating_sub(DIFF_MIN_WIDTH).max(MIN_WIDTH);
        chosen.clamp(MIN_WIDTH, max)
    }

    /// The width the sidebar has now, before it is clamped to the terminal.
    fn current_sidebar_width(&self) -> u16 {
        self.sidebar_custom
            .map_or(SIDEBAR_WIDTH, |w| self.custom_sidebar_width(w))
    }

    fn set_sidebar_width(&mut self, width: Option<u16>) {
        self.sidebar_custom = width.map(|w| self.custom_sidebar_width(w));
        self.drop_mouse_selection();
    }

    /// `Ctrl-w` + `<`, `>` or `=`; returns whether the key was one of them.
    pub(super) fn sidebar_key(&mut self, c: char) -> bool {
        let width = self.current_sidebar_width();
        match c {
            '<' => self.set_sidebar_width(Some(width.saturating_sub(STEP))),
            '>' => self.set_sidebar_width(Some(width + STEP)),
            '=' => self.set_sidebar_width(None),
            _ => return false,
        }
        true
    }

    /// `:sidebar N` sets the width, `:sidebar` (or `reset`) restores it.
    pub(super) fn sidebar_command(&mut self, arg: &str) {
        match arg {
            "" | "reset" => self.set_sidebar_width(None),
            n => match n.parse::<u16>() {
                Ok(width) => self.set_sidebar_width(Some(width)),
                Err(_) => self.message = Some("Usage: :sidebar [N | reset]".into()),
            },
        }
    }

    /// Grabbing the border between the sidebar and the diff and moving it;
    /// returns whether the event was part of that.
    pub(super) fn sidebar_mouse(&mut self, mouse: MouseEvent) -> bool {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let border = self.sidebar_width().saturating_sub(1);
                let grabbed = self.sidebar_visible()
                    && !self.picker
                    && mouse.column == border
                    && mouse.row < self.sidebar_area().height;
                self.mouse.resizing = grabbed;
                grabbed
            }
            MouseEventKind::Drag(MouseButton::Left) if self.mouse.resizing => {
                self.set_sidebar_width(Some(mouse.column.saturating_add(1)));
                true
            }
            MouseEventKind::Up(MouseButton::Left) if self.mouse.resizing => {
                self.mouse.resizing = false;
                true
            }
            _ => false,
        }
    }
}

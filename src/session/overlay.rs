//! The which-key window: a box above the bottom rows listing what can follow
//! the pending prefix.

use ratatui::crossterm::event::KeyCode;

use super::*;
use crate::hints;

impl Session {
    /// The prefix whose which-key window is open: a prefix waited long enough.
    pub(super) fn which_key_prefix(&self) -> Option<char> {
        self.pending
            .filter(|_| self.pending_for >= hints::WHICH_KEY_DELAY)
    }

    /// The last screen row: the command line, or hints for the mode and window.
    pub(super) fn render_hints(&self, buf: &mut Buffer) {
        let y = self.size.1 - 1;
        let width = self.size.0 as usize;
        if let Mode::Command(cmd) = &self.mode {
            buf.set_stringn(0, y, format!(":{cmd}█"), width, Style::new().fg(FG));
            return;
        }
        let context = match (self.pending, self.window(), self.commit_anchor) {
            (Some(prefix), ..) => hints::Context::Pending(prefix),
            (None, Focus::Commits, Some(_)) => hints::Context::CommitsVisual,
            (None, Focus::Commits, None) => hints::Context::Commits,
            (None, Focus::Diff, _) => hints::Context::Diff,
        };
        buf.set_stringn(
            0,
            y,
            format!(" {}", hints::hint_line(context)),
            width,
            Style::new().fg(DIM),
        );
    }

    /// Keys while the help is open: `j/k` scroll, `Esc`, `q` and `?` close.
    pub(super) fn help_key(&mut self, code: KeyCode) {
        let Some(offset) = self.help else { return };
        let last = hints::help_lines().len().saturating_sub(1);
        match code {
            KeyCode::Esc | KeyCode::Char('q' | '?') => self.help = None,
            KeyCode::Char('j') | KeyCode::Down => self.help = Some((offset + 1).min(last)),
            KeyCode::Char('k') | KeyCode::Up => self.help = Some(offset.saturating_sub(1)),
            _ => {}
        }
    }

    pub(super) fn render_help(&self, buf: &mut Buffer, offset: usize) {
        let area = Rect::new(
            1,
            0,
            self.size.0.saturating_sub(2),
            self.size.1.saturating_sub(BOTTOM_ROWS),
        );
        for y in area.y..area.y + area.height {
            buf.set_string(
                area.x,
                y,
                " ".repeat(area.width as usize),
                Style::new().fg(FG),
            );
        }
        draw_box(buf, area, " Справка (j/k прокрутка, Esc закрыть) ");
        let rows = area.height.saturating_sub(2) as usize;
        for (i, line) in hints::help_lines()
            .iter()
            .skip(offset)
            .take(rows)
            .enumerate()
        {
            buf.set_stringn(
                area.x + 2,
                area.y + 1 + i as u16,
                line,
                area.width.saturating_sub(4) as usize,
                Style::new().fg(FG),
            );
        }
    }

    pub(super) fn render_which_key(&self, buf: &mut Buffer, prefix: char) {
        let (title, rows) = hints::which_key(prefix);
        let lines: Vec<String> = rows.iter().map(|(k, l)| format!(" {k:<3} {l} ")).collect();
        let title = format!(" {title} ");
        let inner = lines
            .iter()
            .map(|l| l.chars().count())
            .chain([title.chars().count()])
            .max()
            .unwrap_or(0);
        let width = (inner as u16 + 2).min(self.size.0);
        let height = (lines.len() as u16 + 2).min(self.size.1.saturating_sub(BOTTOM_ROWS));
        let area = Rect::new(
            self.size.0 - width,
            self.size.1 - BOTTOM_ROWS - height,
            width,
            height,
        );
        for y in area.y..area.y + area.height {
            buf.set_string(area.x, y, " ".repeat(width as usize), Style::new().fg(FG));
        }
        draw_box(buf, area, &title);
        for (i, line) in lines
            .iter()
            .take(height.saturating_sub(2) as usize)
            .enumerate()
        {
            buf.set_stringn(
                area.x + 1,
                area.y + 1 + i as u16,
                line,
                width.saturating_sub(2) as usize,
                Style::new().fg(FG),
            );
        }
    }
}

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
            buf.set_stringn(
                0,
                y,
                format!(":{cmd}█"),
                width,
                Style::new().fg(self.theme().fg),
            );
            return;
        }
        if let Mode::Search(query) = &self.mode {
            buf.set_stringn(
                0,
                y,
                format!("/{query}█"),
                width,
                Style::new().fg(self.theme().fg),
            );
            return;
        }
        let context = match (self.pending, self.window(), self.commit_anchor) {
            _ if self.diagram_open() => hints::Context::Diagram,
            _ if self.explain_open() => hints::Context::Explanation,
            (Some(' '), ..) if self.file_picker.is_some() => hints::Context::PickerSpace,
            (Some(prefix), ..) => hints::Context::Pending(prefix),
            (None, ..) if self.file_picker.is_some() => hints::Context::FilePicker,
            (None, Focus::Commits, Some(_)) => hints::Context::CommitsVisual,
            (None, Focus::Commits, None) => hints::Context::Commits,
            (None, Focus::Files, _) => hints::Context::Files,
            (None, Focus::Diff, _) => hints::Context::Diff,
        };
        let mut line = format!(" {}", hints::hint_line(context));
        // The file panel names the Group under its cursor first.
        if matches!(context, hints::Context::Files | hints::Context::FilePicker) {
            if let Some(group) = self.group_hint() {
                let group = fit(&group, width / 2, false);
                line = format!(" {group}  Space i: описание целиком  │{line}");
            }
        }
        buf.set_stringn(0, y, line, width, Style::new().fg(self.theme().dim));
    }

    /// The help wrapped to the width of its box.
    pub(super) fn help_lines(&self) -> Vec<String> {
        let width = scroll::text_cols(self.full_overlay_area());
        (hints::help_lines().iter())
            .flat_map(|line| super::comments::wrap(line, width))
            .collect()
    }

    /// Keys while the help is open: the scrolling keys, `Esc`, `q` and `?`
    /// close.
    pub(super) fn help_key(&mut self, key: KeyEvent) {
        let Some(mut scroll) = self.help else { return };
        let (total, rows) = (
            self.help_lines().len(),
            scroll::text_rows(self.full_overlay_area()),
        );
        if scroll.key(key, total, rows) {
            self.help = Some(scroll);
        } else if matches!(key.code, KeyCode::Esc | KeyCode::Char('q' | '?')) {
            self.help = None;
        }
    }

    /// The box of the full-screen overlays (help, prompt): all rows above the
    /// bottom ones, edge to edge.
    pub(super) fn full_overlay_area(&self) -> Rect {
        Rect::new(0, 0, self.size.0, self.size.1.saturating_sub(BOTTOM_ROWS))
    }

    pub(super) fn render_help(&self, buf: &mut Buffer, scroll: &scroll::Scroll) {
        let area = self.full_overlay_area();
        clear_area(self.theme(), buf, area);
        draw_box(
            self.theme(),
            buf,
            area,
            " Справка (j/k, Ctrl-d/u, gg/G, колесо; Esc закрыть) ",
        );
        scroll::render_lines(self.theme(), buf, area, &self.help_lines(), scroll);
    }

    pub(super) fn render_which_key(&self, buf: &mut Buffer, prefix: char) {
        let (title, mut rows) = hints::which_key(prefix);
        if prefix == ' ' && self.file_picker.is_some() {
            rows.retain(|(key, _)| hints::PICKER_SPACE.contains(key));
        }
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
        clear_area(self.theme(), buf, area);
        draw_box(self.theme(), buf, area, &title);
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
                Style::new().fg(self.theme().fg),
            );
        }
    }
}

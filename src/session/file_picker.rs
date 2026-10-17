//! The `Space f` file picker: the file list in a box over the screen, for
//! reaching a file when the sidebar is hidden.

use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use super::*;

impl Session {
    pub(super) fn open_file_picker(&mut self) {
        self.file_picker = Some(self.selected);
    }

    /// Keys of the open file picker.
    pub(super) fn file_picker_key(&mut self, key: KeyEvent, cursor: usize) {
        let last = self.files.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc => self.file_picker = None,
            KeyCode::Enter => {
                self.file_picker = None;
                self.select(cursor);
            }
            KeyCode::Char('j') | KeyCode::Down => self.file_picker = Some((cursor + 1).min(last)),
            KeyCode::Char('k') | KeyCode::Up => {
                self.file_picker = Some(cursor.saturating_sub(1));
            }
            _ => {}
        }
    }

    /// Where the picker box is.
    pub(super) fn file_picker_area(&self) -> Rect {
        let width = self.size.0.saturating_sub(4).min(72);
        let height = (self.files.len() as u16 + 4).min(self.size.1.saturating_sub(BOTTOM_ROWS + 1));
        Rect::new(
            (self.size.0 - width) / 2,
            (self.size.1 - BOTTOM_ROWS - height) / 2,
            width,
            height,
        )
    }

    /// Clicks and the wheel in the open picker; a click opens the file.
    pub(super) fn file_picker_mouse(&mut self, mouse: MouseEvent) -> bool {
        let Some(cursor) = self.file_picker else {
            return false;
        };
        let area = self.file_picker_area();
        let capacity = (area.height as usize).saturating_sub(4);
        match mouse.kind {
            MouseEventKind::ScrollDown => {
                let last = self.files.len().saturating_sub(1);
                self.file_picker = Some((cursor + 1).min(last));
            }
            MouseEventKind::ScrollUp => self.file_picker = Some(cursor.saturating_sub(1)),
            MouseEventKind::Down(MouseButton::Left) => {
                let offset = (cursor + 1).saturating_sub(capacity.max(1));
                let row = (mouse.row as usize).checked_sub(area.y as usize + 1);
                let inside =
                    (area.x + 1..(area.x + area.width).saturating_sub(1)).contains(&mouse.column);
                if let Some(row) = row.filter(|&r| r < capacity && inside) {
                    if offset + row < self.files.len() {
                        self.file_picker = None;
                        self.select(offset + row);
                    }
                }
            }
            _ => {}
        }
        true
    }

    pub(super) fn render_file_picker(&self, buf: &mut Buffer, cursor: usize) {
        let area = self.file_picker_area();
        let width = area.width;
        for y in area.y..area.y + area.height {
            buf.set_string(area.x, y, " ".repeat(width as usize), Style::new().fg(FG));
        }
        draw_box(buf, area, " Pick a file ");
        let inner_w = area.width.saturating_sub(2) as usize;
        let capacity = (area.height as usize).saturating_sub(4);
        let offset = (cursor + 1).saturating_sub(capacity.max(1));
        for (row, (i, file)) in self
            .files
            .iter()
            .enumerate()
            .skip(offset)
            .take(capacity)
            .enumerate()
        {
            let at = i == cursor;
            let mut style = Style::new().fg(FG);
            if at {
                style = style.bg(Color::Indexed(238)).add_modifier(Modifier::BOLD);
            }
            let line = format!(
                "{} {} {}",
                if at { '>' } else { ' ' },
                file.status.letter(),
                fit(&file.path, inner_w.saturating_sub(4), true)
            );
            buf.set_stringn(
                area.x + 1,
                area.y + 1 + row as u16,
                pad(&line, inner_w),
                inner_w,
                style,
            );
        }
        let hint = "j/k move  Enter open  Esc close";
        buf.set_stringn(
            area.x + 1,
            area.y + area.height - 2,
            fit(hint, inner_w, false),
            inner_w,
            Style::new().fg(DIM),
        );
    }
}

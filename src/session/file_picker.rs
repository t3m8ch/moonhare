//! The `Space f` file picker: the tree of the file panel in a wide box over
//! the screen, for reaching a file when the sidebar is hidden.

use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use super::file_panel::{At, Row};
use super::*;

/// The picker never gets narrower than this, unless the terminal is.
const MIN_WIDTH: u16 = 60;
/// Nor wider than this: longer rows are hard to follow.
const MAX_WIDTH: u16 = 200;
/// Rows of the box outside the tree: borders and the hint row.
const CHROME_ROWS: u16 = 3;

impl Session {
    pub(super) fn open_file_picker(&mut self) {
        self.file_picker = Some(At::File(self.selected));
    }

    /// Where the picker box is: about nine tenths of the screen.
    pub(super) fn file_picker_area(&self) -> Rect {
        let (w, h) = (self.size.0, self.size.1.saturating_sub(BOTTOM_ROWS));
        let width = (w * 9 / 10)
            .max(MIN_WIDTH)
            .min(MAX_WIDTH)
            .min(w.saturating_sub(2));
        let height = (h * 9 / 10).max(h.min(10)).min(h);
        Rect::new((w - width) / 2, (h - height) / 2, width, height)
    }

    /// Rows of the tree the picker shows at once.
    pub(super) fn picker_capacity(&self) -> usize {
        self.file_picker_area().height.saturating_sub(CHROME_ROWS + 1) as usize
    }

    pub(super) fn picker_rows(&self) -> Vec<Row> {
        self.tree_rows((self.file_picker_area().width as usize).saturating_sub(3))
    }

    fn picker_offset(&self, rows: &[Row], at: &At) -> usize {
        let cursor = self.picker_cursor(rows, at).unwrap_or(0);
        file_panel::offset_for(cursor, self.picker_capacity())
    }

    /// Clicks and the wheel in the open picker; a click opens a file or
    /// folds a directory.
    pub(super) fn file_picker_mouse(&mut self, mouse: MouseEvent) -> bool {
        let Some(at) = self.file_picker.clone() else {
            return false;
        };
        let area = self.file_picker_area();
        match mouse.kind {
            MouseEventKind::ScrollDown => self.picker_wheel(true),
            MouseEventKind::ScrollUp => self.picker_wheel(false),
            MouseEventKind::Down(MouseButton::Left) => {
                let rows = self.picker_rows();
                let offset = self.picker_offset(&rows, &at);
                let row = (mouse.row as usize).checked_sub(area.y as usize + 1);
                let inside =
                    (area.x + 1..(area.x + area.width).saturating_sub(1)).contains(&mouse.column);
                if let Some(row) = row.filter(|&r| r < self.picker_capacity() && inside) {
                    if let Some(row) = rows.get(offset + row) {
                        self.click_picker_row(row);
                    }
                }
            }
            _ => {}
        }
        true
    }

    pub(super) fn render_file_picker(&self, buf: &mut Buffer, at: &At) {
        let area = self.file_picker_area();
        clear_area(buf, area);
        draw_box(buf, area, " Pick a file ");
        let inner_w = area.width.saturating_sub(2) as usize;
        let rows = self.picker_rows();
        let cursor = self.picker_cursor(&rows, at);
        self.render_tree_rows(
            buf,
            area.x + 1,
            area.y + 1,
            inner_w,
            &rows,
            self.picker_offset(&rows, at),
            self.picker_capacity(),
            cursor,
        );
        let hint = "j/k move  h/l fold  Enter open  Space i Group  Esc close";
        buf.set_stringn(
            area.x + 1,
            area.y + area.height - 2,
            fit(hint, inner_w, false),
            inner_w,
            Style::new().fg(DIM),
        );
    }
}

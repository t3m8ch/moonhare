//! The mouse over an overlay (help, Summary, Explanation, diagram, Prompt):
//! the wheel scrolls it, a drag, a double click and a triple click select
//! text, which is copied when the button is released.

use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use super::copy::{text_width, word_span};
use super::mouse::SELECTION_BG;
use super::*;

/// Text selected in an overlay, in screen cells.
#[derive(Debug, Clone, Copy)]
pub(super) struct TextSelection {
    rect: Rect,
    anchor: (u16, u16),
    head: (u16, u16),
    /// A double or triple click picked it: copy even a single cell.
    span: bool,
}

impl TextSelection {
    /// The selected cells as ((first column, row), (last column, row)).
    fn ends(&self) -> ((u16, u16), (u16, u16)) {
        let key = |p: (u16, u16)| (p.1, p.0);
        if key(self.anchor) <= key(self.head) {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

impl Session {
    /// The area inside the border of the overlay that is on top, if any.
    fn overlay_text_rect(&self) -> Option<Rect> {
        let full = self.full_overlay_area();
        let area = if let Some((area, ..)) = self.editor_popup() {
            area
        } else if self.prompt_view.is_some() || self.help.is_some() {
            full
        } else if self.group_info_open()
            || self.explain_open()
            || self.diagram_open()
            || self.summary_open()
        {
            self.overlay_area()
        } else {
            return None;
        };
        Some(Rect::new(
            area.x + 2,
            area.y + 1,
            area.width.saturating_sub(4),
            area.height.saturating_sub(2),
        ))
    }

    /// Whether an overlay covers the windows.
    pub(super) fn overlay_open(&self) -> bool {
        self.overlay_text_rect().is_some()
    }

    /// The rows of the rect as they are drawn now, cell by cell.
    fn overlay_cells(&self, rect: Rect) -> Vec<Vec<String>> {
        let mut buf = Buffer::empty(Rect::new(0, 0, self.size.0, self.size.1));
        self.render_base(&mut buf);
        (rect.y..rect.y + rect.height)
            .map(|y| {
                let mut row: Vec<String> = (rect.x..rect.x + rect.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect();
                // The cells after a wide character belong to it.
                for i in 0..row.len() {
                    let width = unicode_width::UnicodeWidthStr::width(row[i].as_str());
                    for later in row.iter_mut().skip(i + 1).take(width.saturating_sub(1)) {
                        later.clear();
                    }
                }
                row
            })
            .collect()
    }

    /// The text between the ends of the selection, rows trimmed on the right.
    fn selected_overlay_text(&self, selection: TextSelection) -> String {
        let rect = selection.rect;
        let cells = self.overlay_cells(rect);
        let ((x0, y0), (x1, y1)) = selection.ends();
        (y0..=y1)
            .map(|y| {
                let row = &cells[(y - rect.y) as usize];
                let from = if y == y0 { (x0 - rect.x) as usize } else { 0 };
                let to = if y == y1 {
                    (x1 - rect.x) as usize + 1
                } else {
                    row.len()
                };
                row[from.min(row.len())..to.min(row.len())]
                    .concat()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The mouse event, while an overlay is open; the overlay takes all of them.
    pub(super) fn overlay_mouse(&mut self, mouse: MouseEvent) -> bool {
        let Some(rect) = self.overlay_text_rect() else {
            return false;
        };
        let clamp = |x: u16, y: u16| {
            (
                x.clamp(rect.x, rect.x + rect.width.saturating_sub(1)),
                y.clamp(rect.y, rect.y + rect.height.saturating_sub(1)),
            )
        };
        let inside = rect.contains((mouse.column, mouse.row).into());
        match mouse.kind {
            MouseEventKind::ScrollDown => self.scroll_overlay(true),
            MouseEventKind::ScrollUp => self.scroll_overlay(false),
            MouseEventKind::Down(MouseButton::Left) => {
                self.mouse.text = None;
                self.mouse.text_press = inside;
                if inside {
                    let at = (mouse.column, mouse.row);
                    let count = self.count_click(mouse.column, mouse.row);
                    if count >= 2 {
                        self.select_overlay_span(rect, at, count == 2);
                    } else {
                        self.mouse.text = Some(TextSelection {
                            rect,
                            anchor: at,
                            head: at,
                            span: false,
                        });
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.mouse.text_press => {
                if let Some(selection) = &mut self.mouse.text {
                    selection.head = clamp(mouse.column, mouse.row);
                }
            }
            MouseEventKind::Up(MouseButton::Left) if self.mouse.text_press => {
                self.mouse.text_press = false;
                // A plain click selects nothing.
                let current = self.overlay_text_rect();
                if let Some(selection) = (self.mouse.text)
                    .filter(|s| (s.span || s.anchor != s.head) && Some(s.rect) == current)
                {
                    let text = self.selected_overlay_text(selection);
                    self.copy_to_clipboard(&text, "the selected text");
                } else {
                    self.mouse.text = None;
                }
            }
            _ => {}
        }
        true
    }

    /// Select the word at `at`, or its whole row.
    fn select_overlay_span(&mut self, rect: Rect, at: (u16, u16), word: bool) {
        let cells = self.overlay_cells(rect);
        let row = cells[(at.1 - rect.y) as usize].concat();
        let (from, to) = if word {
            match word_span(&row, (at.0 - rect.x) as usize) {
                Some(span) => span,
                None => return,
            }
        } else {
            (0, text_width(row.trim_end()))
        };
        if to <= from {
            return;
        }
        self.mouse.text = Some(TextSelection {
            rect,
            anchor: (rect.x + from as u16, at.1),
            head: (rect.x + to as u16 - 1, at.1),
            span: true,
        });
    }

    /// The wheel: a few lines, in the overlay that is on top.
    fn scroll_overlay(&mut self, down: bool) {
        // Selected cells would stay where they are and hold other text.
        if !self.mouse.text_press {
            self.mouse.text = None;
        }
        let (total, rows) = (self.overlay_total(), scroll::text_rows(self.overlay_box()));
        if let Some(scroll) = self.overlay_scroll() {
            scroll.wheel(down, total, rows);
        }
    }

    /// The box and the wrapped text of the overlay that is on top, whatever it
    /// is: what scrolling needs to know.
    fn overlay_box(&self) -> Rect {
        if let Some((area, ..)) = self.editor_popup() {
            area
        } else if self.prompt_view.is_some() || self.help.is_some() {
            self.full_overlay_area()
        } else {
            self.overlay_area()
        }
    }

    /// The rows of text of the overlay that is on top.
    fn overlay_total(&self) -> usize {
        if let Some((_, _, lines)) = self.editor_popup() {
            lines.len()
        } else if self.prompt_view.is_some() {
            self.prompt_lines().len()
        } else if self.help.is_some() {
            self.help_lines().len()
        } else if self.group_info_open() {
            self.group_info_lines().len()
        } else if self.explain_open() {
            self.explain_lines().len()
        } else if self.diagram_open() {
            self.open_lines().len()
        } else if self.summary_open() {
            self.summary_view().1.len()
        } else {
            0
        }
    }

    /// The scroll of the overlay that is on top.
    fn overlay_scroll(&mut self) -> Option<&mut scroll::Scroll> {
        if self.editor.is_some() {
            self.editor_scroll_mut()
        } else if self.prompt_view.is_some() {
            self.prompt_view.as_mut().map(|(_, scroll)| scroll)
        } else if self.help.is_some() {
            self.help.as_mut()
        } else if self.group_info_open() {
            self.group_info_scroll_mut()
        } else if self.explain_open() {
            self.explain_scroll_mut()
        } else if self.diagram_open() {
            self.diagram_scroll_mut()
        } else {
            self.summary_scroll_mut()
        }
    }

    /// Highlight the text selected in an overlay.
    pub(super) fn render_overlay_selection(&self, buf: &mut Buffer) {
        let Some(selection) = self.mouse.text else {
            return;
        };
        if self.overlay_text_rect() != Some(selection.rect) {
            return;
        }
        let rect = selection.rect;
        let cells = self.overlay_cells(rect);
        let ((x0, y0), (x1, y1)) = selection.ends();
        for y in y0..=y1 {
            let row = &cells[(y - rect.y) as usize];
            let text_end = rect.x as usize + text_width(row.concat().trim_end());
            let from = if y == y0 { x0 } else { rect.x } as usize;
            let to = if y == y1 { x1 as usize + 1 } else { text_end };
            for x in from..to.min(text_end) {
                buf[(x as u16, y)].set_bg(SELECTION_BG);
            }
        }
    }
}

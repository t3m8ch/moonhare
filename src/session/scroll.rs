//! Scrolling of the text inside an overlay: the keys, the wheel and the
//! position shown on the border are the same in every overlay.
//!
//! An overlay keeps a [`Scroll`], builds its wrapped lines for the width of its
//! box, hands keys to [`Scroll::key`] and draws with [`render_lines`]. The
//! offset is only clamped when it is used, so a resize cannot leave it
//! out of range.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Line;

use super::*;

/// Rows the wheel scrolls per notch.
pub(super) const WHEEL_STEP: usize = 3;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Scroll {
    top: usize,
    /// `g` was typed and waits for a second one.
    g_typed: bool,
}

impl Scroll {
    /// Scrolled to the end: the last page, however long the text becomes.
    pub(super) fn at_end() -> Scroll {
        Scroll {
            top: usize::MAX,
            g_typed: false,
        }
    }

    /// The first row shown, for `total` rows of text in a box of `rows`.
    pub(super) fn top(&self, total: usize, rows: usize) -> usize {
        self.top.min(total.saturating_sub(rows))
    }

    /// Scroll to the end.
    pub(super) fn jump_to_end(&mut self) {
        self.top = usize::MAX;
    }

    /// A scrolling key; returns whether it was one.
    pub(super) fn key(&mut self, key: KeyEvent, total: usize, rows: usize) -> bool {
        let g_typed = std::mem::take(&mut self.g_typed);
        let last = total.saturating_sub(rows);
        let top = self.top.min(last);
        let half = (rows / 2).max(1);
        let page = rows.saturating_sub(1).max(1);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let target = match (key.code, ctrl) {
            (KeyCode::Char('j') | KeyCode::Down, false) => top + 1,
            (KeyCode::Char('k') | KeyCode::Up, false) => top.saturating_sub(1),
            (KeyCode::Char('d'), true) => top + half,
            (KeyCode::Char('u'), true) => top.saturating_sub(half),
            (KeyCode::Char('f'), true) | (KeyCode::PageDown, _) => top + page,
            (KeyCode::Char('b'), true) | (KeyCode::PageUp, _) => top.saturating_sub(page),
            (KeyCode::Char('G'), false) | (KeyCode::End, _) => last,
            (KeyCode::Home, _) => 0,
            (KeyCode::Char('g'), false) if g_typed => 0,
            (KeyCode::Char('g'), false) => {
                self.g_typed = true;
                top
            }
            _ => return false,
        };
        self.top = target.min(last);
        true
    }

    /// One notch of the wheel.
    pub(super) fn wheel(&mut self, down: bool, total: usize, rows: usize) {
        let last = total.saturating_sub(rows);
        let top = self.top.min(last);
        self.top = if down {
            (top + WHEEL_STEP).min(last)
        } else {
            top.saturating_sub(WHEEL_STEP)
        };
        self.g_typed = false;
    }
}

/// The rows of text a box of `area` has room for.
pub(super) fn text_rows(area: Rect) -> usize {
    area.height.saturating_sub(2) as usize
}

/// The width of text a box of `area` has room for.
pub(super) fn text_cols(area: Rect) -> usize {
    area.width.saturating_sub(4) as usize
}

/// Draw the part of `lines` that `scroll` shows inside the border of `area`,
/// and the position (`last row shown/rows in all`) on the bottom border when
/// there is more than a page.
pub(super) fn render_lines(buf: &mut Buffer, area: Rect, lines: &[String], scroll: &Scroll) {
    let lines: Vec<Line> = lines.iter().map(|l| Line::raw(l.as_str())).collect();
    render_styled(buf, area, &lines, scroll);
}

/// [`render_lines`] for lines that carry their own styles; what has no
/// colour of its own is drawn in the text colour.
pub(super) fn render_styled(buf: &mut Buffer, area: Rect, lines: &[Line], scroll: &Scroll) {
    let rows = text_rows(area);
    let width = text_cols(area);
    let top = scroll.top(lines.len(), rows);
    for (i, line) in lines.iter().skip(top).take(rows).enumerate() {
        let (x, y) = (area.x + 2, area.y + 1 + i as u16);
        let mut left = width;
        let mut col = x;
        for span in &line.spans {
            if left == 0 {
                break;
            }
            let style = Style::new().fg(FG).patch(span.style);
            let (end, _) = buf.set_stringn(col, y, span.content.as_ref(), left, style);
            left = left.saturating_sub((end - col) as usize);
            col = end;
        }
    }
    if lines.len() > rows && area.height >= 2 {
        let shown = (top + rows).min(lines.len());
        let label = format!(" {shown}/{} ", lines.len());
        let len = label.chars().count() as u16;
        if area.width >= len + 4 {
            buf.set_stringn(
                area.x + area.width - 2 - len,
                area.y + area.height - 1,
                &label,
                len as usize,
                Style::new().fg(DIM),
            );
        }
    }
}

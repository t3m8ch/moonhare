//! Moving over a picture (a drawn Diagram) that is wider and taller than the
//! box it is shown in: keys and the wheel move the view along both axes,
//! markers on the edges tell where the picture goes on and the border names
//! the columns and rows in sight.
//!
//! Like [`super::scroll::Scroll`], the offsets are clamped when used, so a
//! resize or a new picture cannot leave them out of range.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthChar;

use super::scroll::WHEEL_STEP;
use super::*;

/// Columns a sideways notch of the wheel moves.
pub(super) const WHEEL_COLUMNS: usize = 2 * WHEEL_STEP;

/// The size of a picture or of the box it is shown in: columns and rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Size {
    pub cols: usize,
    pub rows: usize,
}

impl Size {
    pub(super) fn of(lines: &[String]) -> Size {
        Size {
            cols: lines.iter().map(|l| text_width(l)).max().unwrap_or(0),
            rows: lines.len(),
        }
    }

    fn of_rect(rect: Rect) -> Size {
        Size {
            cols: rect.width as usize,
            rows: rect.height as usize,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Pan {
    /// The first column shown; `None` until the Reviewer moves sideways: the
    /// view starts where the top of the picture is in sight.
    left: Option<usize>,
    top: usize,
    /// `g` was typed and waits for a second one.
    g_typed: bool,
}

impl Pan {
    /// The view at `left`, `top`.
    pub(super) fn at(left: usize, top: usize) -> Pan {
        Pan {
            left: Some(left),
            top,
            g_typed: false,
        }
    }

    /// The first column shown of `lines` in a box of `view`.
    pub(super) fn left(&self, lines: &[String], view: Size) -> usize {
        let last = Size::of(lines).cols.saturating_sub(view.cols);
        self.left.unwrap_or_else(|| start(lines, view)).min(last)
    }

    /// The first row shown of `lines` in a box of `view`.
    pub(super) fn top(&self, lines: &[String], view: Size) -> usize {
        self.top.min(lines.len().saturating_sub(view.rows))
    }

    /// A moving key; returns whether it was one.
    pub(super) fn key(&mut self, key: KeyEvent, lines: &[String], view: Size) -> bool {
        let g_typed = std::mem::take(&mut self.g_typed);
        let size = Size::of(lines);
        let (last_col, last_row) = (
            size.cols.saturating_sub(view.cols),
            size.rows.saturating_sub(view.rows),
        );
        let (left, top) = (self.left(lines, view), self.top(lines, view));
        let half_cols = (view.cols / 2).max(1);
        let half_rows = (view.rows / 2).max(1);
        let page = view.rows.saturating_sub(1).max(1);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let (left, top) = match (key.code, ctrl) {
            (KeyCode::Char('h') | KeyCode::Left, false) => (left.saturating_sub(1), top),
            (KeyCode::Char('l') | KeyCode::Right, false) => (left + 1, top),
            (KeyCode::Char('j') | KeyCode::Down, false) => (left, top + 1),
            (KeyCode::Char('k') | KeyCode::Up, false) => (left, top.saturating_sub(1)),
            (KeyCode::Char('H'), false) => (left.saturating_sub(half_cols), top),
            (KeyCode::Char('L'), false) => (left + half_cols, top),
            (KeyCode::Char('d'), true) => (left, top + half_rows),
            (KeyCode::Char('u'), true) => (left, top.saturating_sub(half_rows)),
            (KeyCode::Char('f'), true) | (KeyCode::PageDown, _) => (left, top + page),
            (KeyCode::Char('b'), true) | (KeyCode::PageUp, _) => (left, top.saturating_sub(page)),
            (KeyCode::Char('0'), false) => (0, top),
            (KeyCode::Char('$'), false) => (last_col, top),
            (KeyCode::Char('G'), false) | (KeyCode::End, _) => (left, last_row),
            (KeyCode::Home, _) => (left, 0),
            (KeyCode::Char('g'), false) if g_typed => (left, 0),
            (KeyCode::Char('g'), false) => {
                self.g_typed = true;
                (left, top)
            }
            _ => return false,
        };
        self.left = Some(left.min(last_col));
        self.top = top.min(last_row);
        true
    }

    /// One notch of the wheel, down or up, or sideways when `sideways`.
    pub(super) fn wheel(&mut self, forward: bool, sideways: bool, lines: &[String], view: Size) {
        self.g_typed = false;
        let size = Size::of(lines);
        let (left, top) = (self.left(lines, view), self.top(lines, view));
        let step = |at: usize, by: usize, last: usize| {
            if forward {
                (at + by).min(last)
            } else {
                at.saturating_sub(by)
            }
        };
        if sideways {
            self.left = Some(step(
                left,
                WHEEL_COLUMNS,
                size.cols.saturating_sub(view.cols),
            ));
        } else {
            self.left = Some(left);
            self.top = step(top, WHEEL_STEP, size.rows.saturating_sub(view.rows));
        }
    }
}

/// Where the view of `lines` starts: at the left edge, unless nothing of the
/// top of the picture (its first row with something drawn, a block centred
/// over a wide picture) is in sight from there; then far enough right to show
/// it.
fn start(lines: &[String], view: Size) -> usize {
    let Some(first) = lines.iter().find(|l| !l.trim().is_empty()) else {
        return 0;
    };
    let from = text_width(&first[..first.len() - first.trim_start().len()]);
    let to = text_width(first.trim_end());
    if from < view.cols {
        0
    } else if to - from <= view.cols {
        to - view.cols
    } else {
        from
    }
}

fn text_width(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

/// `cols` cells of `line` from column `left`; a wide character cut in half
/// becomes a space.
pub(super) fn slice(line: &str, left: usize, cols: usize) -> String {
    let mut out = String::new();
    let (mut at, mut used) = (0, 0);
    for c in line.chars() {
        let w = c.width().unwrap_or(0);
        if at + w <= left {
            at += w;
            continue;
        }
        if used >= cols {
            break;
        }
        if at < left {
            // The character starts before the view.
            out.extend(std::iter::repeat_n(' ', at + w - left));
            used += at + w - left;
        } else if used + w > cols {
            out.extend(std::iter::repeat_n(' ', cols - used));
            break;
        } else {
            out.push(c);
            used += w;
        }
        at += w;
    }
    out
}

/// What [`render`] draws `lines` in.
pub(super) struct Place {
    /// Where the picture goes.
    pub text: Rect,
    /// The box whose edges carry the markers: the border of an overlay, or
    /// the text itself when there is none.
    pub edges: Rect,
    /// The row the position is written on, right-aligned, and how far from
    /// its edges.
    pub label: Option<(Rect, u16)>,
}

/// Draw the part of `lines` that `pan` shows, the markers of the sides the
/// picture goes on past, and its position when it does not fit.
pub(super) fn render(theme: &Theme, buf: &mut Buffer, place: &Place, lines: &[String], pan: &Pan) {
    let view = Size::of_rect(place.text);
    let (left, top) = (pan.left(lines, view), pan.top(lines, view));
    let style = Style::new().fg(theme.fg);
    for (i, line) in lines.iter().skip(top).take(view.rows).enumerate() {
        let shown = slice(line, left, view.cols);
        buf.set_stringn(
            place.text.x,
            place.text.y + i as u16,
            &shown,
            view.cols,
            style,
        );
    }
    let size = Size::of(lines);
    if size.cols <= view.cols && size.rows <= view.rows {
        return;
    }
    let edges = place.edges;
    let marker = Style::new().fg(theme.accent);
    let (mid_x, mid_y) = (edges.x + edges.width / 2, edges.y + edges.height / 2);
    let (right, bottom) = (
        edges.x + edges.width.saturating_sub(1),
        edges.y + edges.height.saturating_sub(1),
    );
    if let Some((row, margin)) = place.label {
        let label = position(left, top, view, size);
        let len = label.chars().count() as u16;
        if row.width >= len + 2 * margin {
            buf.set_stringn(
                row.x + row.width - margin - len,
                row.y,
                &label,
                len as usize,
                Style::new().fg(theme.dim),
            );
        }
    }
    if left > 0 {
        buf.set_string(edges.x, mid_y, "◀", marker);
    }
    if left + view.cols < size.cols {
        buf.set_string(right, mid_y, "▶", marker);
    }
    if top > 0 {
        buf.set_string(mid_x, edges.y, "▲", marker);
    }
    if top + view.rows < size.rows {
        buf.set_string(mid_x, bottom, "▼", marker);
    }
}

/// ` колонки 120–240 / 380 · строки 1–40 / 90 `.
fn position(left: usize, top: usize, view: Size, size: Size) -> String {
    let span = |from: usize, shown: usize, all: usize| {
        format!("{}–{} / {all}", from + 1, (from + shown).min(all))
    };
    format!(
        " колонки {} · строки {} ",
        span(left, view.cols, size.cols),
        span(top, view.rows, size.rows)
    )
}

/// The view of `to` whose centre is where the centre of `pan`'s view of
/// `from` is, in shares of the pictures' sizes: a picture laid out again
/// keeps the place the Reviewer was looking at in sight.
pub(super) fn recentre(pan: &Pan, from: &[String], to: &[String], view: Size) -> Pan {
    let (old, new) = (Size::of(from), Size::of(to));
    let along = |first: usize, shown: usize, old: usize, new: usize| {
        let centre = (first as f64 + shown.min(old) as f64 / 2.0) / old.max(1) as f64;
        (centre * new as f64 - shown as f64 / 2.0).max(0.0).round() as usize
    };
    Pan::at(
        along(pan.left(from, view), view.cols, old.cols, new.cols),
        along(pan.top(from, view), view.rows, old.rows, new.rows),
    )
}

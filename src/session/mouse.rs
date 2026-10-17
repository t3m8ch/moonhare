//! The mouse in the diff: clicks put the cursor on a line, a drag selects
//! code and releasing the button copies it.

use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use super::copy::{cut, format, CopyLine, Mark};
use super::*;

/// A place in the diff: a view item and a cell column in its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Pos {
    item: usize,
    col: usize,
}

/// Code selected with the mouse.
#[derive(Debug, Clone, Copy)]
pub(super) struct Selection {
    anchor: Pos,
    head: Pos,
    /// Side-by-side: the half the selection lives in.
    half: Option<Side>,
}

/// The button held down.
#[derive(Debug, Clone, Copy)]
pub(super) struct Press {
    anchor: Pos,
    half: Option<Side>,
}

#[derive(Default)]
pub(super) struct Mouse {
    press: Option<Press>,
    selection: Option<Selection>,
}

impl Session {
    /// The mouse event, when it is one of the diff's; returns whether it was.
    pub(super) fn diff_mouse(&mut self, mouse: MouseEvent) -> bool {
        let area = self.diff_area();
        let inside = area.contains((mouse.column, mouse.row).into()) && !self.picker;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if inside => {
                let Some(item) = self.item_at_row(mouse.row) else {
                    return false;
                };
                self.focus = Focus::Diff;
                self.vp.cursor = item;
                self.mouse.selection = None;
                let half = self.half_at(mouse.column);
                let anchor = self.pos_at(item, mouse.column, half);
                self.mouse.press = Some(Press { anchor, half });
                true
            }
            MouseEventKind::Drag(MouseButton::Left) if self.mouse.press.is_some() => {
                if let (Some(press), Some(item)) =
                    (self.mouse.press, self.item_at_or_above(mouse.row))
                {
                    self.mouse.selection = Some(Selection {
                        anchor: press.anchor,
                        head: self.pos_at(item, mouse.column, press.half),
                        half: press.half,
                    });
                }
                true
            }
            MouseEventKind::Up(MouseButton::Left) if self.mouse.press.take().is_some() => {
                if let Some(selection) = self.mouse.selection {
                    let text = format(&self.selected_lines(selection));
                    self.copy_to_clipboard(&text, "the selected code");
                }
                true
            }
            _ => false,
        }
    }

    /// The view item drawn on screen row `row` of the diff.
    fn item_at_row(&self, row: u16) -> Option<usize> {
        let top = self.diff_area().y + 1;
        let layout = self.comment_layout();
        self.visible_items()
            .find(|&item| layout.y(top, self.vp.top, item) == row)
    }

    /// The last visible item that starts at or above `row`: a row of a
    /// Comment box belongs to the line it hangs under.
    fn item_at_or_above(&self, row: u16) -> Option<usize> {
        let top = self.diff_area().y + 1;
        let layout = self.comment_layout();
        self.visible_items()
            .take_while(|&item| layout.y(top, self.vp.top, item) <= row)
            .last()
    }

    fn visible_items(&self) -> impl Iterator<Item = usize> {
        let height = self.viewport_height();
        self.vp.top..self.line_count().min(self.vp.top + height)
    }

    fn digits(&self) -> usize {
        match &self.loaded {
            Some(Ok(l)) => digits(l.diff.old_lines.len().max(l.diff.new_lines.len())),
            _ => 1,
        }
    }

    /// Side-by-side: the half that column `x` is in.
    fn half_at(&self, x: u16) -> Option<Side> {
        if self.view_layout != DiffLayout::SideBySide {
            return None;
        }
        let area = self.diff_area();
        let half = (area.width as usize).saturating_sub(1) / 2;
        Some(if (x as usize) <= area.x as usize + half {
            Side::Old
        } else {
            Side::New
        })
    }

    /// The place under column `x` in the line of `item`; side-by-side
    /// columns count in the given half, whichever half `x` is over.
    fn pos_at(&self, item: usize, x: u16, half: Option<Side>) -> Pos {
        let area = self.diff_area();
        let width = area.width as usize;
        let side_width = width.saturating_sub(1) / 2;
        // Where the code of the half starts and ends on screen.
        let (code_x, end) = match half {
            None => (area.x as usize + 2 * self.digits() + 4, usize::MAX),
            Some(Side::Old) => (
                area.x as usize + self.digits() + 1,
                area.x as usize + side_width,
            ),
            Some(Side::New) => (
                area.x as usize + side_width + 1 + self.digits() + 1,
                area.x as usize + width,
            ),
        };
        let col = if (x as usize) >= end {
            usize::MAX / 2
        } else {
            (x as usize).saturating_sub(code_x) + self.hscroll
        };
        Pos { item, col }
    }

    /// The lines a selection covers, each cut to the selected columns.
    fn selected_lines(&self, selection: Selection) -> Vec<CopyLine> {
        let (from, to) = if selection.anchor <= selection.head {
            (selection.anchor, selection.head)
        } else {
            (selection.head, selection.anchor)
        };
        let Some(Ok(loaded)) = &self.loaded else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for item in from.item..=to.item {
            let Some(&Item::Line(entry)) = self.view.items.get(item) else {
                continue;
            };
            let start = if item == from.item { from.col } else { 0 };
            let end = if item == to.item { to.col + 1 } else { usize::MAX };
            let (side, index, mark) = match selection.half {
                None => {
                    let line = self.inline[entry];
                    match line.kind {
                        InlineKind::Context => (Side::New, line.new.unwrap(), Mark::Context),
                        InlineKind::Removed => (Side::Old, line.old.unwrap(), Mark::Removed),
                        InlineKind::Added => (Side::New, line.new.unwrap(), Mark::Added),
                    }
                }
                Some(side) => {
                    let row = loaded.diff.rows[entry];
                    let (mine, other) = match side {
                        Side::Old => (row.old, row.new),
                        Side::New => (row.new, row.old),
                    };
                    let Some(index) = mine else { continue };
                    let mark = match (loaded.line_changed(side, index, other), side) {
                        (false, _) => Mark::Context,
                        (true, Side::Old) => Mark::Removed,
                        (true, Side::New) => Mark::Added,
                    };
                    (side, index, mark)
                }
            };
            out.push(CopyLine {
                mark,
                side,
                line: index,
                text: cut(&loaded.lines(side)[index], start, end),
            });
        }
        out
    }
}

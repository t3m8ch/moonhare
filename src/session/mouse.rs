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

/// A click, to tell double and triple clicks.
#[derive(Debug, Clone, Copy)]
struct Click {
    at: (u16, u16),
    time: std::time::Duration,
    count: u8,
}

const SELECTION_BG: Color = Color::Indexed(24);

/// Clicks closer in time than this chain into a double or triple click.
const MULTI_CLICK: std::time::Duration = std::time::Duration::from_millis(400);

#[derive(Default)]
pub(super) struct Mouse {
    /// `:set nomouse`: the terminal keeps the mouse.
    disabled: bool,
    /// Time since the session opened, as the events and ticks report it.
    clock: std::time::Duration,
    last_click: Option<Click>,
    press: Option<Press>,
    pub(super) selection: Option<Selection>,
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
                match self.count_click(mouse.column, mouse.row) {
                    2 => self.select_span(anchor, half, true),
                    3 => self.select_span(anchor, half, false),
                    _ => {}
                }
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
                    let text = format(&self.selected_lines(selection), &[]);
                    self.copy_to_clipboard(&text, "the selected code");
                }
                true
            }
            _ => false,
        }
    }

    /// The wheel over the commit list or the file list; returns whether it was.
    pub(super) fn list_mouse(&mut self, mouse: MouseEvent) -> bool {
        let down = match mouse.kind {
            MouseEventKind::ScrollDown => true,
            MouseEventKind::ScrollUp => false,
            _ => return false,
        };
        let at = (mouse.column, mouse.row).into();
        if self.sidebar_visible() && !self.picker && self.commits_area().contains(at) {
            self.focus = Focus::Commits;
            let key = if down { 'j' } else { 'k' };
            self.commits_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
            return true;
        }
        if self.sidebar_visible() && !self.picker && self.files_area().contains(at) {
            let last = self.files.len().saturating_sub(1);
            let to = if down {
                (self.selected + 1).min(last)
            } else {
                self.selected.saturating_sub(1)
            };
            self.select(to);
            return true;
        }
        false
    }

    /// A click on screen row `row` of the commit list drawn in `inner`
    /// (below the border): the cursor goes to the commit, and the focus too.
    pub(super) fn click_commit_list(&mut self, inner: Rect, row: u16) {
        let frame = self.range_frame_in(inner.width, inner.height).len();
        let capacity = (inner.height as usize).saturating_sub(1 + frame);
        // Row 0 is the pinned "Весь Range" row, the Range Comments hang under it.
        let Some(row) = (row as usize).checked_sub(inner.y as usize) else {
            return;
        };
        if row <= capacity + frame {
            self.focus = Focus::Commits;
            if row <= frame {
                self.commit_cursor = 0;
            } else {
                // A Round header is not a commit: the click only focuses.
                let at = self.commit_offset(capacity) + row - 1 - frame;
                if let Some(rounds::ListRow::Commit(i)) = self.list_rows().get(at) {
                    self.commit_cursor = i + 1;
                }
            }
        }
    }

    /// Clicks and the wheel in the open commit picker; returns whether the
    /// picker took the event.
    pub(super) fn picker_mouse(&mut self, mouse: MouseEvent) -> bool {
        if !self.picker {
            return false;
        }
        let inner = self.picker_inner();
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left)
                if inner.contains((mouse.column, mouse.row).into()) =>
            {
                self.click_commit_list(inner, mouse.row);
            }
            MouseEventKind::ScrollDown => {
                self.commits_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
            }
            MouseEventKind::ScrollUp => {
                self.commits_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
            }
            _ => {}
        }
        true
    }

    /// Whether the terminal should hand mouse events over (`:set mouse`).
    pub fn mouse_enabled(&self) -> bool {
        !self.mouse.disabled
    }

    /// `:set mouse` and `:set nomouse`.
    pub(super) fn set_command(&mut self, option: &str) {
        match option {
            "mouse" => self.mouse.disabled = false,
            "nomouse" => {
                self.mouse.disabled = true;
                self.mouse.press = None;
                self.mouse.selection = None;
            }
            other => self.message = Some(format!("Unknown option: {other}")),
        }
    }

    /// Time passes: double clicks are told apart by it.
    pub(super) fn mouse_time(&mut self, elapsed: std::time::Duration) {
        self.mouse.clock += elapsed;
    }

    /// Register a click and say how many in a row it is: 1, 2 or 3.
    fn count_click(&mut self, column: u16, row: u16) -> u8 {
        let now = self.mouse.clock;
        let count = match self.mouse.last_click {
            Some(c) if c.at == (column, row) && now - c.time <= MULTI_CLICK => c.count % 3 + 1,
            _ => 1,
        };
        self.mouse.last_click = Some(Click {
            at: (column, row),
            time: now,
            count,
        });
        count
    }

    /// Select the word under `at`, or its whole line.
    fn select_span(&mut self, at: Pos, half: Option<Side>, word: bool) {
        let Some(line) = self.copy_line_at(at.item, half) else {
            return;
        };
        let (from, to) = if word {
            match word_span(&line.text, at.col) {
                Some(span) => span,
                None => return,
            }
        } else {
            (0, usize::MAX / 2)
        };
        self.mouse.selection = Some(Selection {
            anchor: Pos { col: from, ..at },
            head: Pos {
                col: to.saturating_sub(1),
                ..at
            },
            half,
        });
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

    /// Screen columns where the code of the half (or of the inline view)
    /// starts, and where it ends.
    fn code_columns(&self, half: Option<Side>) -> (usize, usize) {
        let area = self.diff_area();
        let width = area.width as usize;
        let side_width = width.saturating_sub(1) / 2;
        match half {
            None => (area.x as usize + 2 * self.digits() + 4, usize::MAX),
            Some(Side::Old) => (
                area.x as usize + self.digits() + 1,
                area.x as usize + side_width,
            ),
            Some(Side::New) => (
                area.x as usize + side_width + 1 + self.digits() + 1,
                area.x as usize + width,
            ),
        }
    }

    /// Drop the selection; returns whether there was one.
    pub(super) fn clear_mouse_selection(&mut self) -> bool {
        self.mouse.selection.take().is_some()
    }

    /// The place under column `x` in the line of `item`; side-by-side
    /// columns count in the given half, whichever half `x` is over.
    fn pos_at(&self, item: usize, x: u16, half: Option<Side>) -> Pos {
        let (code_x, end) = self.code_columns(half);
        let col = if (x as usize) >= end {
            usize::MAX / 2
        } else {
            (x as usize).saturating_sub(code_x) + self.hscroll
        };
        Pos { item, col }
    }

    /// The cell columns selected on each item the selection touches.
    fn selected_spans(selection: Selection) -> Vec<(usize, usize, usize)> {
        let (from, to) = if selection.anchor <= selection.head {
            (selection.anchor, selection.head)
        } else {
            (selection.head, selection.anchor)
        };
        (from.item..=to.item)
            .map(|item| {
                let start = if item == from.item { from.col } else { 0 };
                let end = if item == to.item { to.col + 1 } else { usize::MAX };
                (item, start, end)
            })
            .collect()
    }

    /// The lines a selection covers, each cut to the selected columns.
    pub(super) fn selected_lines(&self, selection: Selection) -> Vec<CopyLine> {
        Self::selected_spans(selection)
            .into_iter()
            .filter_map(|(item, start, end)| {
                let mut line = self.copy_line_at(item, selection.half)?;
                line.text = cut(&line.text, start, end);
                Some(line)
            })
            .collect()
    }

    /// Highlight the code the mouse selected.
    pub(super) fn render_mouse_selection(&self, buf: &mut Buffer, top: u16, height: usize) {
        let Some(selection) = self.mouse.selection else {
            return;
        };
        let area = self.diff_area();
        let layout = self.comment_layout();
        let bottom = top + height as u16;
        for (item, start, end) in Self::selected_spans(selection) {
            if !self.visible_items().any(|i| i == item) {
                continue;
            }
            let y = layout.y(top, self.vp.top, item);
            if y >= bottom {
                continue;
            }
            let Some(line) = self.copy_line_at(item, selection.half) else {
                continue;
            };
            let (code_x, limit) = self.code_columns(selection.half);
            let text_end = line.text.chars().map(|c| if c == '\t' { TAB_WIDTH } else { 1 }).sum();
            let (start, end) = (start.max(self.hscroll), end.min(text_end));
            for col in start..end {
                let x = code_x + col - self.hscroll;
                if x >= limit || x >= (area.x + area.width) as usize {
                    break;
                }
                buf[(x as u16, y)].set_bg(SELECTION_BG);
            }
        }
    }

    /// The lines a view item stands for when copied whole: one in the inline
    /// layout, and in side-by-side the removed line and the added one of a
    /// changed row, as the inline layout would show them.
    pub(super) fn whole_lines(&self, item: usize) -> Vec<CopyLine> {
        let Some(Ok(loaded)) = &self.loaded else {
            return Vec::new();
        };
        if self.view_layout == DiffLayout::Inline {
            return self.copy_line_at(item, None).into_iter().collect();
        }
        let Some(&Item::Line(entry)) = self.view.items.get(item) else {
            return Vec::new();
        };
        let row = loaded.diff.rows[entry];
        let line = |side: Side, index: usize, mark: Mark| CopyLine {
            mark,
            side,
            line: index,
            text: loaded.lines(side)[index].clone(),
        };
        let old_changed = row.old.is_some_and(|l| loaded.line_changed(Side::Old, l, row.new));
        let new_changed = row.new.is_some_and(|l| loaded.line_changed(Side::New, l, row.old));
        if !old_changed && !new_changed {
            let (side, index) = match (row.new, row.old) {
                (Some(n), _) => (Side::New, n),
                (None, Some(o)) => (Side::Old, o),
                _ => return Vec::new(),
            };
            return vec![line(side, index, Mark::Context)];
        }
        let mut out = Vec::new();
        if let Some(o) = row.old {
            out.push(line(Side::Old, o, Mark::Removed));
        }
        if let Some(n) = row.new {
            out.push(line(Side::New, n, Mark::Added));
        }
        out
    }

    /// The whole line of `item` as a selection in `half` takes it.
    fn copy_line_at(&self, item: usize, half: Option<Side>) -> Option<CopyLine> {
        let Some(Ok(loaded)) = &self.loaded else {
            return None;
        };
        let &Item::Line(entry) = self.view.items.get(item)? else {
            return None;
        };
        let (side, index, mark) = match half {
            None => {
                let line = self.inline[entry];
                match line.kind {
                    InlineKind::Context => (Side::New, line.new?, Mark::Context),
                    InlineKind::Removed => (Side::Old, line.old?, Mark::Removed),
                    InlineKind::Added => (Side::New, line.new?, Mark::Added),
                }
            }
            Some(side) => {
                let row = loaded.diff.rows[entry];
                let (mine, other) = match side {
                    Side::Old => (row.old, row.new),
                    Side::New => (row.new, row.old),
                };
                let index = mine?;
                let mark = match (loaded.line_changed(side, index, other), side) {
                    (false, _) => Mark::Context,
                    (true, Side::Old) => Mark::Removed,
                    (true, Side::New) => Mark::Added,
                };
                (side, index, mark)
            }
        };
        Some(CopyLine {
            mark,
            side,
            line: index,
            text: loaded.lines(side)[index].clone(),
        })
    }
}

/// The cell columns `from..to` of the word at cell column `col` of `text`:
/// a run of letters, digits and `_`, of blanks, or one other character.
fn word_span(text: &str, col: usize) -> Option<(usize, usize)> {
    let mut cells = Vec::new();
    let mut cell = 0;
    for c in text.chars() {
        let width = if c == '\t' { TAB_WIDTH } else { 1 };
        cells.push((c, cell, cell + width));
        cell += width;
    }
    let at = cells.iter().position(|&(_, from, to)| from <= col && col < to)?;
    let class = |c: char| match c {
        c if c.is_alphanumeric() || c == '_' => 0,
        c if c.is_whitespace() => 1,
        _ => 2,
    };
    let kind = class(cells[at].0);
    if kind == 2 {
        return Some((cells[at].1, cells[at].2));
    }
    let mut first = at;
    while first > 0 && class(cells[first - 1].0) == kind {
        first -= 1;
    }
    let mut last = at;
    while last + 1 < cells.len() && class(cells[last + 1].0) == kind {
        last += 1;
    }
    Some((cells[first].1, cells[last].2))
}

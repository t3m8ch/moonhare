// PROTOTYPE 4 mouse handling: clicks, wheel, and text selection that copies on release
// through a real OSC 52 sequence (like opencode / Claude Code).
//
// Rules the user asked for:
//  - only code text is ever selected: never line numbers, gutter markers, +/- signs,
//    borders, Comment boxes, other panes, or the opposite column in side-by-side;
//  - releasing the button copies (OSC 52 + toast); double-click = word, triple-click = line;
//  - Esc or a click elsewhere clears the selection.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::*;

fn inside(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x.saturating_add(r.width) && y >= r.y && y < r.y.saturating_add(r.height)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// [start, end) of the word (or the run of same-class characters) around `ch`.
fn word_bounds(text: &[char], ch: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }
    let ch = ch.min(text.len() - 1);
    let class = |c: char| if is_word(c) { 0 } else if c.is_whitespace() { 1 } else { 2 };
    let k = class(text[ch]);
    let mut s = ch;
    while s > 0 && class(text[s - 1]) == k {
        s -= 1;
    }
    let mut e = ch + 1;
    while e < text.len() && class(text[e]) == k {
        e += 1;
    }
    (s, e)
}

impl App {
    pub fn on_mouse(&mut self, ev: MouseEvent) {
        if !self.mouse_on || self.too_small() {
            return;
        }
        self.toast = None;
        let (x, y) = (ev.column, ev.row);
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => self.mouse_down(x, y),
            MouseEventKind::Drag(MouseButton::Left) => self.mouse_drag(x, y),
            MouseEventKind::Up(MouseButton::Left) => self.mouse_up(),
            MouseEventKind::ScrollUp => self.mouse_wheel(x, y, -1),
            MouseEventKind::ScrollDown => self.mouse_wheel(x, y, 1),
            _ => {}
        }
    }

    fn click_count(&mut self, x: u16, y: u16) -> u8 {
        let now = Instant::now();
        let c = match self.last_click {
            Some((t, lx, ly, c)) if now.saturating_duration_since(t) < Duration::from_millis(500) && lx == x && ly == y => {
                if c >= 3 {
                    1
                } else {
                    c + 1
                }
            }
            _ => 1,
        };
        self.last_click = Some((now, x, y, c));
        c
    }

    fn mouse_down(&mut self, x: u16, y: u16) {
        let count = self.click_count(x, y);
        self.dragging = false;
        self.drag_moved = false;
        self.multi_click = false;
        self.drag_overlay = false;
        self.drag_start = None;
        self.drag_start_o = None;
        if self.overlay.is_some() {
            self.overlay_down(x, y, count);
            return;
        }
        if !matches!(self.mode, Mode::Normal | Mode::Visual) {
            return; // never lose a Comment draft or a half-typed command to a stray click
        }
        self.chord = None;
        self.whichkey = false;
        self.dsel = None;
        if self.mode == Mode::Visual {
            self.diff_anchor = None;
            self.commit_anchor = None;
            self.mode = Mode::Normal;
        }
        if let Some(inner) = self.hits.diff.as_ref().map(|g| g.inner) {
            if inside(inner, x, y) {
                self.diff_down(x, y, count);
                return;
            }
        }
        if let Some(r) = self.hits.range_chip {
            if inside(r, x, y) {
                self.on_range = true;
                self.open_overlay(Overlay::Commits);
                return;
            }
        }
        if let Some(idx) = self.hits.commit_rows.iter().find(|(r, _)| inside(*r, x, y)).map(|(_, i)| *i) {
            self.focus = Pane::Commits;
            self.set_commit_cursor(idx);
            if count == 2 {
                self.commit_anchor = None;
                self.apply_commit_scope();
            }
            return;
        }
        if let Some(ix) = self.hits.list_rows.iter().find(|(r, _)| inside(*r, x, y)).map(|(_, i)| *i) {
            self.focus = Pane::Files;
            let items = self.list_items();
            match items.get(ix).copied() {
                Some(ListItem::Group(g)) => {
                    self.list_goto(ix);
                    self.list_toggle_fold(g);
                }
                Some(ListItem::File(_)) => {
                    self.list_goto(ix);
                    if count == 2 {
                        self.focus = Pane::Diff;
                    }
                }
                None => {}
            }
            return;
        }
        if let Some(p) = self.hits.panes.iter().find(|(r, _)| inside(*r, x, y)).map(|(_, p)| *p) {
            self.focus = p;
        }
    }

    fn diff_down(&mut self, x: u16, y: u16, count: u8) {
        self.focus = Pane::Diff;
        let Some(geo) = self.hits.diff.as_ref() else { return };
        let row = (y - geo.inner.y) as usize;
        let Some(meta) = geo.lines.get(row).cloned() else { return };
        let Some(src) = meta.src else { return };
        let rel_x = x - geo.inner.x;
        self.diff_cursor = src;
        // a click on the gutter / line numbers only places the cursor: it never starts a selection
        let Some(seg) = meta.segs.iter().find(|s| rel_x >= s.x0 && rel_x < s.x0 + s.w).cloned() else { return };
        let ch = seg.skip + (rel_x - seg.x0) as usize;
        let pos = DPos { src, sub: meta.sub, ch };
        self.drag_start = Some((pos, seg.col));
        self.dragging = true;
        if count >= 2 {
            let (s, e) = if count == 2 { word_bounds(&seg.text, ch) } else { (0, seg.text.len()) };
            if e > s {
                self.dsel = Some(DiffSel { a: DPos { ch: s, ..pos }, b: DPos { ch: e - 1, ..pos }, col: seg.col });
                self.multi_click = true;
            }
        }
    }

    /// Position (row, char) in column `col` for a pointer position; the pointer may be
    /// outside the diff area or outside the column: it is clamped, never leaves the column.
    fn diff_pos_at(&self, x: u16, y: u16, col: u8) -> Option<DPos> {
        let geo = self.hits.diff.as_ref()?;
        if geo.lines.is_empty() {
            return None;
        }
        let last = geo.inner.y + geo.inner.height.saturating_sub(1);
        let yy = y.clamp(geo.inner.y, last);
        let mut idx = ((yy - geo.inner.y) as usize).min(geo.lines.len() - 1);
        // comment boxes and "⋯ unchanged" lines carry no row: use the nearest row above, else below
        if geo.lines[idx].src.is_none() {
            idx = (0..=idx).rev().find(|i| geo.lines[*i].src.is_some()).or_else(|| (idx..geo.lines.len()).find(|i| geo.lines[*i].src.is_some()))?;
        }
        let meta = &geo.lines[idx];
        let rel_x = x.saturating_sub(geo.inner.x);
        let ch = match meta.segs.iter().find(|s| s.col == col) {
            Some(s) if rel_x >= s.x0 => s.skip + (rel_x - s.x0) as usize,
            _ => 0,
        };
        Some(DPos { src: meta.src?, sub: meta.sub, ch })
    }

    fn mouse_drag(&mut self, x: u16, y: u16) {
        if self.drag_overlay {
            self.overlay_drag(x, y);
            return;
        }
        let Some((start, col)) = self.drag_start else { return };
        let Some(end) = self.diff_pos_at(x, y, col) else { return };
        if end.key() != start.key() {
            self.drag_moved = true;
        }
        if self.drag_moved {
            self.dsel = Some(DiffSel { a: start, b: end, col });
        }
    }

    fn mouse_up(&mut self) {
        if self.drag_overlay {
            self.drag_overlay = false;
            if let Some(((l1, c1), (l2, c2))) = self.osel {
                if (l1, c1) != (l2, c2) || self.multi_click {
                    let text = self.selected_overlay_text();
                    if !text.is_empty() {
                        self.emit_clip(&text);
                        self.toast = Some(format!("Скопировано {} симв.", text.chars().count()));
                    }
                }
            }
            return;
        }
        let was = self.drag_start.take().is_some();
        self.dragging = false;
        if was && self.dsel.is_some() && (self.drag_moved || self.multi_click) {
            self.copy_mouse_selection(false);
        }
    }

    fn mouse_wheel(&mut self, x: u16, y: u16, d: isize) {
        if let Some(o) = self.overlay {
            match o {
                Overlay::Commits => self.commit_move(d),
                Overlay::Files => {
                    let n = self.visible_files().len();
                    self.overlay_cursor = (self.overlay_cursor as isize + d).clamp(0, n.saturating_sub(1) as isize) as usize;
                }
                _ => self.overlay_scroll = (self.overlay_scroll as isize + 3 * d).max(0) as usize,
            }
            return;
        }
        if let Some(inner) = self.hits.diff.as_ref().map(|g| g.inner) {
            if inside(inner, x, y) {
                self.diff_move(3 * d);
                return;
            }
        }
        if let Some(p) = self.hits.panes.iter().find(|(r, _)| inside(*r, x, y)).map(|(_, p)| *p) {
            match p {
                Pane::Files => self.list_move(d),
                Pane::Commits => self.commit_move(d),
                Pane::Diff => self.diff_move(3 * d),
            }
        }
    }

    // ---------- overlays ----------

    fn overlay_down(&mut self, x: u16, y: u16, count: u8) {
        let Some(bx) = self.hits.overlay_box else { return };
        if !inside(bx, x, y) {
            self.close_overlay();
            return;
        }
        match self.overlay {
            Some(Overlay::Commits) => {
                if let Some(idx) = self.hits.picker_rows.iter().find(|(r, _)| inside(*r, x, y)).map(|(_, i)| *i) {
                    self.set_commit_cursor(idx);
                    self.commit_anchor = None;
                    self.apply_commit_scope();
                    self.close_overlay();
                }
            }
            Some(Overlay::Files) => {
                if let Some(pos) = self.hits.picker_rows.iter().find(|(r, _)| inside(*r, x, y)).map(|(_, i)| *i) {
                    self.close_overlay();
                    self.select_file(pos);
                }
            }
            Some(_) => {
                self.osel = None;
                let Some(geo) = self.hits.overlay.as_ref() else { return };
                if !inside(geo.inner, x, y) {
                    return;
                }
                let line = geo.scroll + (y - geo.inner.y) as usize;
                let Some(text) = geo.texts.get(line) else { return };
                let chars: Vec<char> = text.chars().collect();
                let ch = (x - geo.inner.x) as usize;
                self.drag_overlay = true;
                self.drag_start_o = Some((line, ch));
                if count >= 2 {
                    let (s, e) = if count == 2 { word_bounds(&chars, ch) } else { (0, chars.len()) };
                    if e > s {
                        self.osel = Some(((line, s), (line, e - 1)));
                        self.multi_click = true;
                    }
                }
            }
            None => {}
        }
    }

    fn overlay_drag(&mut self, x: u16, y: u16) {
        let Some(start) = self.drag_start_o else { return };
        let Some(geo) = self.hits.overlay.as_ref() else { return };
        if geo.texts.is_empty() {
            return;
        }
        let last = geo.inner.y + geo.inner.height.saturating_sub(1);
        let yy = y.clamp(geo.inner.y, last);
        let line = (geo.scroll + (yy - geo.inner.y) as usize).min(geo.texts.len() - 1);
        let ch = x.saturating_sub(geo.inner.x) as usize;
        if (line, ch) != start {
            self.drag_moved = true;
        }
        if self.drag_moved {
            self.osel = Some((start, (line, ch)));
        }
    }

    pub fn selected_overlay_text(&self) -> String {
        let (Some(((l1, c1), (l2, c2))), Some(geo)) = (self.osel, self.hits.overlay.as_ref()) else { return String::new() };
        let ((lo_l, lo_c), (hi_l, hi_c)) = if (l1, c1) <= (l2, c2) { ((l1, c1), (l2, c2)) } else { ((l2, c2), (l1, c1)) };
        let mut out = Vec::new();
        for l in lo_l..=hi_l {
            let Some(t) = geo.texts.get(l) else { break };
            let chars: Vec<char> = t.chars().collect();
            let from = if l == lo_l { lo_c.min(chars.len()) } else { 0 };
            let to = if l == hi_l { (hi_c + 1).min(chars.len()) } else { chars.len() };
            out.push(if from < to { chars[from..to].iter().collect::<String>() } else { String::new() });
        }
        out.join("\n")
    }

    /// Copy the active mouse selection again (`<Space>y` also inserts the anchored Comments).
    /// A plain copy: it never changes a Comment's state and never closes a Round.
    pub fn copy_mouse_selection(&mut self, with_comments: bool) {
        let (parts, as_diff, fi) = self.selected_parts();
        if parts.is_empty() {
            self.toast = Some("нечего копировать: выделение пусто".into());
            return;
        }
        let (text, quoted) = self.render_copy(fi, &parts, as_diff, with_comments);
        if text.is_empty() {
            return;
        }
        self.emit_clip(&text);
        self.toast = Some(self.copy_toast(&text, as_diff, with_comments, quoted));
    }

    /// `<Space>y`: the active mouse selection if there is one, else the VISUAL selection.
    pub fn copy_with_comments(&mut self) {
        if self.dsel.is_some() {
            self.copy_mouse_selection(true);
        } else {
            self.copy_visual(true);
        }
    }

    /// The selected code as (mark, text, source row) parts, char-precise on the first and last
    /// line. Side-by-side stays inside one column (plain code of that side). Inline (Q46):
    /// no removed lines → plain new code; no added lines → plain old code; both → a diff with
    /// ' ', '+', '-' prefixes. Returns (parts, as_diff, file).
    pub fn selected_parts(&self) -> (Vec<(char, String, usize)>, bool, usize) {
        let (Some(sel), Some(geo)) = (self.dsel, self.hits.diff.as_ref()) else { return (Vec::new(), false, 0) };
        let lines = crate::ui::diff_lines(self, geo.fi, geo.width, geo.inline);
        let (lo, hi) = sel.norm();
        let mut parts: Vec<(char, String, usize)> = Vec::new();
        for dl in &lines {
            let Some(src) = dl.src else { continue };
            let key = (src, dl.sub);
            if key < (lo.src, lo.sub) || key > (hi.src, hi.sub) {
                continue;
            }
            let Some(seg) = dl.segs.iter().find(|s| s.col == sel.col) else { continue };
            let len = seg.text.len();
            let from = if key == (lo.src, lo.sub) { lo.ch.min(len) } else { 0 };
            let to = if key == (hi.src, hi.sub) { (hi.ch + 1).min(len) } else { len };
            let piece: String = if from < to { seg.text[from..to].iter().collect() } else { String::new() };
            parts.push((seg.mark, piece, src));
        }
        let as_diff = sel.col == 2 && parts.iter().any(|p| p.0 == '+') && parts.iter().any(|p| p.0 == '-');
        (parts, as_diff, geo.fi)
    }
}

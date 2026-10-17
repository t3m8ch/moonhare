//! Comments in the session: the editor, the boxes drawn under lines and the
//! keys that create them.

use std::collections::HashMap;

use super::*;
use crate::comments::{Comment, State, Target};

const COMMENT_COLOR: Color = Color::Indexed(179);
const DRAFT_COLOR: Color = Color::Indexed(215);
/// Columns the boxes are indented by.
const INDENT: usize = 5;

/// The Comment being written: a draft the Reviewer has not saved yet.
pub(super) struct Editor {
    target: Target,
    text: String,
    insert: bool,
    /// Asking whether to throw a non-empty draft away.
    confirm: bool,
}

/// One drawn row of a Comment box.
#[derive(Clone)]
pub(super) struct CRow {
    pub(super) text: String,
    pub(super) style: Style,
}

/// The Comment rows of the displayed file, by the view item they hang under.
pub(super) struct CommentLayout {
    /// File Comments: a frame above the first line.
    header: Vec<CRow>,
    /// The first view item on screen.
    top: usize,
    under: HashMap<usize, Vec<CRow>>,
}

impl CommentLayout {
    /// Screen row of view item `item` when the pane shows items from `top`
    /// and the first one is on row `first`.
    pub(super) fn y(&self, first: u16, top: usize, item: usize) -> u16 {
        let extra: usize = (top..item)
            .filter_map(|i| self.under.get(&i))
            .map(Vec::len)
            .sum();
        first + (self.header_rows(top) + item - top + extra) as u16
    }

    /// Rows the file frame takes: it scrolls away with the first line.
    fn header_rows(&self, top: usize) -> usize {
        if top == 0 {
            self.header.len()
        } else {
            0
        }
    }

    pub(super) fn draw_header(&self, buf: &mut Buffer, x: u16, first: u16, bottom: u16) {
        // Only while the first line is the top one.
        if self.header_rows(self.top) == 0 {
            return;
        }
        for (k, row) in self.header.iter().enumerate() {
            let y = first + k as u16;
            if y >= bottom {
                return;
            }
            buf.set_stringn(
                x,
                y,
                &row.text,
                buf.area.width.saturating_sub(x) as usize,
                row.style,
            );
        }
    }

    pub(super) fn draw(&self, buf: &mut Buffer, x: u16, y: u16, bottom: u16, item: usize) {
        for (k, row) in self.under.get(&item).into_iter().flatten().enumerate() {
            let y = y + k as u16;
            if y >= bottom {
                return;
            }
            buf.set_stringn(
                x,
                y,
                &row.text,
                buf.area.width.saturating_sub(x) as usize,
                row.style,
            );
        }
    }
}

/// `text` cut into lines of at most `width` characters.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for line in text.split('\n') {
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            out.push(String::new());
        }
        out.extend(chars.chunks(width).map(|c| c.iter().collect::<String>()));
    }
    out
}

fn comment_box(
    width: usize,
    header: &str,
    body: &str,
    footer: Option<&str>,
    style: Style,
) -> Vec<CRow> {
    let pad = " ".repeat(INDENT);
    let row = |text: String| CRow { text, style };
    let mut rows = vec![row(format!("{pad}┌ {header}"))];
    for line in wrap(body, width.saturating_sub(INDENT + 3)) {
        rows.push(row(format!("{pad}│ {line}")));
    }
    if let Some(footer) = footer {
        rows.push(CRow {
            text: format!("{pad}└ {footer}"),
            style: Style::new().fg(DIM),
        });
    }
    rows
}

fn lines_label(side: Side, start: usize, end: usize) -> String {
    let side = match side {
        Side::Old => "old",
        Side::New => "new",
    };
    if start == end {
        format!("{side} {}", start + 1)
    } else {
        format!("{side} {}-{}", start + 1, end + 1)
    }
}

impl Session {
    /// Index of the view item showing `line` of `side`, if it is on screen.
    fn item_of_line(&self, side: Side, line: usize) -> Option<usize> {
        self.view.items.iter().position(|&item| match item {
            Item::Line(e) => {
                let (old, new) = self.entry_lines(e);
                match side {
                    Side::Old => old == Some(line),
                    Side::New => new == Some(line),
                }
            }
            Item::Fold { .. } => false,
        })
    }

    /// Where a line Comment of the displayed file is in its code now.
    pub(super) fn place_of(&self, c: &Comment) -> Option<(Side, usize, usize)> {
        let Target::Lines {
            path,
            side,
            start,
            end,
            anchor,
        } = &c.target
        else {
            return None;
        };
        let (Some(file), Some(Ok(loaded))) = (self.files.get(self.selected), &self.loaded) else {
            return None;
        };
        if *path != file.path {
            return None;
        }
        let at = crate::comments::locate(loaded.lines(*side), *start, anchor)?;
        Some((*side, at, at + (end - start)))
    }

    /// The Comment boxes of the displayed file, with the draft.
    pub(super) fn comment_layout(&self) -> CommentLayout {
        let mut under: HashMap<usize, Vec<CRow>> = HashMap::new();
        let mut header = Vec::new();
        let Some(file) = self.files.get(self.selected) else {
            return CommentLayout {
                header,
                top: self.vp.top,
                under,
            };
        };
        let width = self.diff_area().width as usize;
        for c in self.comments.all() {
            if matches!(&c.target, Target::File { path } if *path == file.path) {
                header.extend(self.saved_box(width, c, "file".into()));
            }
            if let Some((side, start, end)) = self.place_of(c) {
                if let Some(item) = self.item_of_line(side, end) {
                    under.entry(item).or_default().extend(self.saved_box(
                        width,
                        c,
                        lines_label(side, start, end),
                    ));
                }
            }
        }
        if let Some(Editor {
            target:
                Target::Lines {
                    path,
                    side,
                    start,
                    end,
                    ..
                },
            text,
            insert,
            confirm,
        }) = &self.editor
        {
            if *path == file.path {
                if let Some(item) = self.item_of_line(*side, *end) {
                    let mode = if *insert { "INSERT" } else { "COMMENT" };
                    let header = format!("Comment · {mode} · {}", lines_label(*side, *start, *end));
                    let body = format!("{text}{}", if *insert { "█" } else { "" });
                    under.entry(item).or_default().extend(comment_box(
                        width,
                        &header,
                        &body,
                        Some(editor_hint(*insert, *confirm)),
                        Style::new().fg(DRAFT_COLOR),
                    ));
                }
            }
        }
        CommentLayout {
            header,
            top: self.vp.top,
            under,
        }
    }

    fn saved_box(&self, width: usize, c: &Comment, place: String) -> Vec<CRow> {
        let color = match c.state {
            crate::comments::State::Open => COMMENT_COLOR,
            _ => DIM,
        };
        let header = format!("Comment · {} · {place}", c.state.label());
        comment_box(width, &header, &c.text, None, Style::new().fg(color))
    }

    /// The mode name at the start of the status line, padded to 8 columns.
    pub(super) fn mode_badge(&self) -> &'static str {
        match &self.editor {
            Some(ed) if ed.insert => " INSERT ",
            Some(_) => " COMMENT",
            None if self.visual.is_some() => " VISUAL ",
            None if self.commit_anchor.is_some() && self.window() == Focus::Commits => " VISUAL ",
            None => " NORMAL ",
        }
    }

    /// Scroll so that the cursor's row is on screen although boxes take rows.
    pub(super) fn reveal_with_comments(&mut self) {
        let height = self.viewport_height();
        if height == 0 || self.view.items.is_empty() {
            return;
        }
        let layout = self.comment_layout();
        while self.vp.top < self.vp.cursor
            && layout.y(0, self.vp.top, self.vp.cursor) as usize >= height
        {
            self.vp.top += 1;
        }
    }

    /// The side and line the diff cursor stands on: the new side unless the
    /// line only exists in the old version.
    fn cursor_line(&self) -> Option<(Side, usize)> {
        let Item::Line(e) = *self.view.items.get(self.vp.cursor)? else {
            return None;
        };
        let (old, new) = self.entry_lines(e);
        match (new, old) {
            (Some(n), _) => Some((Side::New, n)),
            (None, Some(o)) => Some((Side::Old, o)),
            _ => None,
        }
    }

    /// `c`: a Comment on the line under the cursor.
    fn comment_on_line(&mut self) {
        let Some((side, line)) = self.cursor_line() else {
            self.message = Some("No line to comment on here".into());
            return;
        };
        let (mut start, mut end) = (line, line);
        if let Some(anchor) = self.visual.take() {
            let (lo, hi) = (anchor.min(self.vp.cursor), anchor.max(self.vp.cursor));
            let lines = (lo..=hi).filter_map(|i| match *self.view.items.get(i)? {
                Item::Line(e) => {
                    let (old, new) = self.entry_lines(e);
                    match side {
                        Side::Old => old,
                        Side::New => new,
                    }
                }
                Item::Fold { .. } => None,
            });
            let (first, last) = lines.fold((line, line), |(a, b), l| (a.min(l), b.max(l)));
            (start, end) = (first, last);
        }
        self.begin_lines_comment(side, start, end);
    }

    /// Highlight the rows of the VISUAL selection.
    pub(super) fn render_selection(&self, buf: &mut Buffer, area: Rect, top: u16, height: usize) {
        let Some(anchor) = self.visual else { return };
        let (lo, hi) = (anchor.min(self.vp.cursor), anchor.max(self.vp.cursor));
        let layout = self.comment_layout();
        let bottom = top + height as u16;
        for item in lo.max(self.vp.top)..=hi {
            let y = layout.y(top, self.vp.top, item);
            if y >= bottom {
                break;
            }
            buf.set_style(
                Rect::new(area.x, y, area.width, 1),
                Style::new().bg(Color::Indexed(24)),
            );
        }
    }

    fn begin_lines_comment(&mut self, side: Side, start: usize, end: usize) {
        let (Some(file), Some(Ok(loaded))) = (self.files.get(self.selected), &self.loaded) else {
            return;
        };
        let anchor = crate::comments::Anchor::capture(loaded.lines(side), start, end);
        self.editor = Some(Editor {
            target: Target::Lines {
                path: file.path.clone(),
                side,
                start,
                end,
                anchor,
            },
            text: String::new(),
            insert: false,
            confirm: false,
        });
    }

    /// The status-line counter of Range Comments, which stands in for their
    /// frame while the sidebar is hidden: its column and text.
    fn range_chip(&self) -> Option<(u16, String)> {
        let count = self
            .comments
            .all()
            .iter()
            .filter(|c| c.target == Target::Range)
            .count();
        if count == 0 || self.sidebar_visible() {
            return None;
        }
        let text = format!(" Range ✎{count} ");
        let x = self.size.0.saturating_sub(text.chars().count() as u16);
        Some((x, text))
    }

    pub(super) fn draw_range_chip(&self, buf: &mut Buffer, y: u16) {
        if let Some((x, text)) = self.range_chip() {
            let style = Style::new().fg(Color::Indexed(16)).bg(COMMENT_COLOR);
            buf.set_string(x, y, text, style);
        }
    }

    /// Whether a click at the cell lands on the Range counter.
    pub(super) fn on_range_chip(&self, column: u16, row: u16) -> bool {
        self.range_chip().is_some_and(|(x, text)| {
            row == self.size.1 - BOTTOM_ROWS
                && column >= x
                && column < x + text.chars().count() as u16
        })
    }

    /// The Comments `x` and `o` act on: the Range ones while the cursor is on
    /// the "Весь Range" row, else the line ones that cover the cursor line or,
    /// failing that, the ones on the file.
    pub(super) fn comments_at_cursor(&self) -> Vec<u64> {
        let ids = |keep: &dyn Fn(&Target) -> bool| -> Vec<u64> {
            (self.comments.all().iter())
                .filter(|c| keep(&c.target))
                .map(|c| c.id)
                .collect()
        };
        if self.focus == Focus::Commits && self.commit_cursor == 0 {
            return ids(&|t| *t == Target::Range);
        }
        let Some(file) = self.files.get(self.selected) else {
            return vec![];
        };
        let on_line = self.cursor_line().map(|(side, line)| {
            (self.comments.all().iter())
                .filter(|c| {
                    self.place_of(c)
                        .is_some_and(|(s, a, b)| s == side && (a..=b).contains(&line))
                })
                .map(|c| c.id)
                .collect::<Vec<_>>()
        });
        match on_line {
            Some(found) if !found.is_empty() => found,
            _ => ids(&|t| matches!(t, Target::File { path } if *path == file.path)),
        }
    }

    fn set_comment_state(&mut self, state: State) {
        let ids = self.comments_at_cursor();
        if ids.is_empty() {
            self.message = Some("No Comment here".into());
            return;
        }
        let saved = self.comments.set_state(&ids, state);
        match saved {
            Ok(()) => {
                let verb = if state == State::Closed {
                    "Closed"
                } else {
                    "Reopened"
                };
                let noun = if ids.len() == 1 {
                    "Comment"
                } else {
                    "Comments"
                };
                self.message = Some(format!("{verb} {} {noun}", ids.len()));
            }
            Err(e) => self.message = Some(e),
        }
    }

    /// Comments about files and lines that the Scope leaves off the screen:
    /// their file is not in the list, or their code is not in the open file.
    /// Range Comments are always shown, so they never count.
    fn hidden_comments(&self) -> usize {
        let open = self.files.get(self.selected).map(|f| &f.path);
        self.comments
            .all()
            .iter()
            .filter(|c| match &c.target {
                Target::Range => false,
                Target::File { path } => !self.files.iter().any(|f| f.path == *path),
                Target::Lines { path, .. } => {
                    if open == Some(path) {
                        self.place_of(c)
                            .and_then(|(side, _, end)| self.item_of_line(side, end))
                            .is_none()
                    } else {
                        !self.files.iter().any(|f| f.path == *path)
                            || self.hidden_elsewhere().contains(&c.id)
                    }
                }
            })
            .count()
    }

    /// Ids of line Comments on files of the Scope other than the open one
    /// whose code is gone from the Scope's version of the file. Reading the
    /// files costs, so the answer is kept until the Scope or Comments change.
    fn hidden_elsewhere(&self) -> Vec<u64> {
        let (base, head) = self.scope_ends();
        let key = (base.clone(), head.clone(), self.comments.revision());
        if let Some((k, ids)) = &*self.hidden_cache.borrow() {
            if *k == key {
                return ids.clone();
            }
        }
        let ids: Vec<u64> = self
            .comments
            .all()
            .iter()
            .filter_map(|c| {
                let Target::Lines {
                    path,
                    side,
                    start,
                    anchor,
                    ..
                } = &c.target
                else {
                    return None;
                };
                let file = self.scope_files.iter().find(|f| f.path == *path)?;
                let blob = match side {
                    Side::Old => self.repo.file_at(&base, file.base_path()),
                    Side::New => self.repo.file_at(&head, &file.path),
                };
                let lines: Vec<String> = match blob {
                    Ok(bytes) => String::from_utf8_lossy(&bytes)
                        .lines()
                        .map(str::to_string)
                        .collect(),
                    Err(_) => vec![],
                };
                crate::comments::locate(&lines, *start, anchor)
                    .is_none()
                    .then_some(c.id)
            })
            .collect();
        *self.hidden_cache.borrow_mut() = Some((key, ids.clone()));
        ids
    }

    /// `text` with the hint about hidden Comments, when there are any.
    pub(super) fn with_hidden_hint(&self, text: String) -> String {
        match self.hidden_comments() {
            0 => text,
            n => format!("{text}  {n} скрытых Comments"),
        }
    }

    /// `:comment`: a Comment on the whole Range.
    pub(super) fn comment_on_range(&mut self) {
        self.editor = Some(Editor {
            target: Target::Range,
            text: String::new(),
            insert: false,
            confirm: false,
        });
        self.visual = None;
    }

    /// The Range Comments as frames hanging under the "Весь Range" row: the
    /// state and the first two lines of each.
    pub(super) fn range_frame(&self, width: usize) -> Vec<CRow> {
        let mut rows = Vec::new();
        for c in self.comments.all() {
            if c.target != Target::Range {
                continue;
            }
            let style = Style::new().fg(match c.state {
                crate::comments::State::Open => COMMENT_COLOR,
                _ => DIM,
            });
            let row = |text: String| CRow { text, style };
            rows.push(row(format!("   ┌ Comment · {}", c.state.label())));
            let lines = wrap(&c.text, width.saturating_sub(5));
            let shown = lines.len().min(2);
            for (k, line) in lines.iter().take(shown).enumerate() {
                let more = if k + 1 == shown && lines.len() > shown {
                    "…"
                } else {
                    ""
                };
                rows.push(row(format!("   │ {line}{more}")));
            }
            rows.push(row("   └".to_string()));
        }
        rows
    }

    /// [`Self::range_frame`] cut so that a box of `height` rows keeps its
    /// "Весь Range" row and border.
    pub(super) fn range_frame_in(&self, width: u16, height: u16) -> Vec<CRow> {
        let mut rows = self.range_frame(width as usize);
        rows.truncate(height.saturating_sub(2) as usize);
        rows
    }

    /// `C`: a Comment on the whole file.
    fn comment_on_file(&mut self) {
        let Some(file) = self.files.get(self.selected) else {
            return;
        };
        self.editor = Some(Editor {
            target: Target::File {
                path: file.path.clone(),
            },
            text: String::new(),
            insert: false,
            confirm: false,
        });
        self.visual = None;
    }

    /// File and Range drafts have no line to hang under: a box over the screen.
    pub(super) fn render_editor_popup(&self, buf: &mut Buffer) {
        let Some(ed) = &self.editor else { return };
        let what = match &ed.target {
            Target::Lines { .. } => return,
            Target::File { path } => format!(" Comment on {path} "),
            Target::Range => " Comment on the whole Range ".to_string(),
        };
        let width = self.size.0.saturating_sub(4).min(72);
        let inner = width.saturating_sub(4) as usize;
        let body = format!("{}{}", ed.text, if ed.insert { "█" } else { "" });
        let mut lines = wrap(&body, inner);
        lines.push(String::new());
        lines.push(editor_hint(ed.insert, ed.confirm).to_string());
        let height = (lines.len() as u16 + 2).min(self.size.1);
        let area = Rect::new(
            (self.size.0 - width) / 2,
            (self.size.1 - height) / 2,
            width,
            height,
        );
        for y in area.y..area.y + area.height {
            buf.set_string(area.x, y, " ".repeat(width as usize), Style::new().fg(FG));
        }
        draw_box(buf, area, &what);
        for (k, line) in lines
            .iter()
            .take(height.saturating_sub(2) as usize)
            .enumerate()
        {
            buf.set_stringn(
                area.x + 2,
                area.y + 1 + k as u16,
                line,
                inner,
                Style::new().fg(FG),
            );
        }
    }

    /// Keys of Normal mode that deal with Comments.
    pub(super) fn comment_key(&mut self, key: char) {
        match key {
            'c' => self.comment_on_line(),
            'C' => self.comment_on_file(),
            'x' => self.set_comment_state(State::Closed),
            'o' => self.set_comment_state(State::Open),
            'v' => {
                self.visual = match self.visual {
                    Some(_) => None,
                    None => Some(self.vp.cursor),
                }
            }
            _ => {}
        }
    }

    /// Keys while the editor is open.
    pub(super) fn editor_key(&mut self, key: KeyEvent) {
        let Some(ed) = self.editor.as_mut() else {
            return;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ed.confirm {
            match key.code {
                KeyCode::Char('y') => {
                    self.editor = None;
                    self.message = Some("Comment discarded".into());
                }
                KeyCode::Char('n') | KeyCode::Esc => ed.confirm = false,
                _ => {}
            }
        } else if ed.insert {
            match key.code {
                KeyCode::Esc => ed.insert = false,
                KeyCode::Enter => ed.text.push('\n'),
                KeyCode::Backspace => {
                    ed.text.pop();
                }
                KeyCode::Char(c) if !ctrl && !c.is_control() => ed.text.push(c),
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Char('i' | 'a') if !ctrl => ed.insert = true,
                KeyCode::Enter => self.save_draft(),
                KeyCode::Esc if ed.text.trim().is_empty() => self.editor = None,
                KeyCode::Esc => ed.confirm = true,
                _ => {}
            }
        }
    }

    /// `y` deletes every Comment, anything else keeps them.
    pub(super) fn confirm_clear_key(&mut self, key: KeyEvent) {
        self.confirm_clear = false;
        if key.code == KeyCode::Char('y') {
            match self.comments.clear() {
                Ok(()) => self.message = Some("All Comments deleted".into()),
                Err(e) => self.message = Some(e),
            }
        }
    }

    fn report_saved(&mut self, saved: Result<(), String>) {
        if let Err(e) = saved {
            self.message = Some(e);
        }
    }

    fn save_draft(&mut self) {
        let Some(ed) = self.editor.take() else {
            return;
        };
        let text = ed.text.trim_end().to_string();
        if text.trim().is_empty() {
            self.message = Some("Empty Comment discarded".into());
            return;
        }
        let saved = self.comments.add(ed.target, text).map(|_| ());
        self.report_saved(saved);
    }
}

fn editor_hint(insert: bool, confirm: bool) -> &'static str {
    if confirm {
        "Discard the draft? y - discard, n / Esc - go back"
    } else if insert {
        "INSERT: Esc - normal, Enter - new line"
    } else {
        "i - type, Enter - save, Esc - cancel"
    }
}

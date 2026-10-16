//! Comments in the session: the editor, the boxes drawn under lines and the
//! keys that create them.

use std::collections::HashMap;

use super::*;
use crate::comments::{Comment, Target};

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
    text: String,
    style: Style,
}

/// The Comment rows of the displayed file, by the view item they hang under.
pub(super) struct CommentLayout {
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
        first + (item - top + extra) as u16
    }

    pub(super) fn draw(&self, buf: &mut Buffer, x: u16, y: u16, bottom: u16, item: usize) {
        for (k, row) in self.under.get(&item).into_iter().flatten().enumerate() {
            let y = y + k as u16;
            if y >= bottom {
                return;
            }
            buf.set_stringn(x, y, &row.text, buf.area.width.saturating_sub(x) as usize, row.style);
        }
    }
}

/// `text` cut into lines of at most `width` characters.
fn wrap(text: &str, width: usize) -> Vec<String> {
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

fn comment_box(width: usize, header: &str, body: &str, footer: Option<&str>, style: Style) -> Vec<CRow> {
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

    /// The Comment boxes of the displayed file, with the draft.
    pub(super) fn comment_layout(&self) -> CommentLayout {
        let mut under: HashMap<usize, Vec<CRow>> = HashMap::new();
        let Some(file) = self.files.get(self.selected) else {
            return CommentLayout { under };
        };
        let width = self.diff_area().width as usize;
        for c in self.comments.all() {
            if let Target::Lines {
                path,
                side,
                start,
                end,
                ..
            } = &c.target
            {
                if *path != file.path {
                    continue;
                }
                if let Some(item) = self.item_of_line(*side, *end) {
                    under
                        .entry(item)
                        .or_default()
                        .extend(self.saved_box(width, c, lines_label(*side, *start, *end)));
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
        CommentLayout { under }
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
            let lines = (lo..=hi).filter_map(|i| match self.view.items[i] {
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
            buf.set_style(Rect::new(area.x, y, area.width, 1), Style::new().bg(Color::Indexed(24)));
        }
    }

    fn begin_lines_comment(&mut self, side: Side, start: usize, end: usize) {
        let (Some(file), Some(Ok(loaded))) = (self.files.get(self.selected), &self.loaded) else {
            return;
        };
        let snapshot = loaded.lines(side)[start..=end].to_vec();
        self.editor = Some(Editor {
            target: Target::Lines {
                path: file.path.clone(),
                side,
                start,
                end,
                snapshot,
            },
            text: String::new(),
            insert: false,
            confirm: false,
        });
    }

    /// Keys of Normal mode that deal with Comments.
    pub(super) fn comment_key(&mut self, key: char) {
        match key {
            'c' => self.comment_on_line(),
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

    fn save_draft(&mut self) {
        let Some(ed) = self.editor.take() else {
            return;
        };
        let text = ed.text.trim_end().to_string();
        if text.trim().is_empty() {
            self.message = Some("Empty Comment discarded".into());
            return;
        }
        self.comments.add(ed.target, text);
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

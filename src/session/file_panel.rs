//! The file panel as a directory tree: the rows the sidebar and the file
//! picker share (built by [`crate::file_tree`] for each Group), the cursor
//! walking them, folding, and the description of the Group under the cursor.

use std::collections::HashSet;
use std::ops::Range;

use super::*;
use crate::file_tree::{self, Line};

/// A directory of the tree: the Group it is drawn in and its path.
pub(super) type DirKey = (Option<usize>, String);

/// Where a cursor in the tree stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum At {
    /// File `n` of the displayed list.
    File(usize),
    Dir(DirKey),
}

/// A row of the file panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Row {
    /// The name of Group `n` and its progress.
    Header(usize),
    /// A row of the tree of a Group (`None`: the tree of all files). File
    /// indices are into the displayed list.
    Tree {
        group: Option<usize>,
        line: Line,
        folded: bool,
    },
}

impl Row {
    pub(super) fn at(&self) -> Option<At> {
        match self {
            Row::Header(_) => None,
            Row::Tree {
                line: Line::File { index, .. },
                ..
            } => Some(At::File(*index)),
            Row::Tree {
                group,
                line: Line::Dir { path, .. },
                ..
            } => Some(At::Dir((*group, path.clone()))),
        }
    }

    pub(super) fn group(&self) -> Option<usize> {
        match self {
            Row::Header(n) => Some(*n),
            Row::Tree { group, .. } => *group,
        }
    }

    fn dir_key(&self) -> Option<(DirKey, bool)> {
        match self {
            Row::Tree {
                group,
                line: Line::Dir { path, .. },
                folded,
            } => Some(((*group, path.clone()), *folded)),
            _ => None,
        }
    }
}

#[derive(Default)]
pub(super) struct FilePanel {
    /// The sidebar cursor stands on this directory; `None`: on the open file.
    pub(super) dir: Option<DirKey>,
    folded: HashSet<DirKey>,
    /// `Space i`: the description of Group `n` in an overlay.
    info: Option<(usize, scroll::Scroll)>,
}

/// What a key does to a cursor in the tree.
enum Motion {
    /// Move to row `n`.
    Go(usize),
    /// Fold (or unfold) the directory of row `n` and stand on it.
    Fold(usize, bool),
    /// Open file `n`.
    Open(usize),
}

/// The row a cursor at `at` stands on: the row itself, or when a resize or a
/// fold hid it, the nearest row that holds it.
fn cursor_row(rows: &[Row], at: &At, open_file: usize) -> Option<usize> {
    if let Some(i) = rows.iter().position(|r| r.at().as_ref() == Some(at)) {
        return Some(i);
    }
    let file = match at {
        At::File(i) => *i,
        At::Dir((group, path)) => {
            // A wider row joined the directory into a longer node.
            let inside = format!("{path}/");
            let joined = rows.iter().position(|r| {
                matches!(r, Row::Tree { group: g, line: Line::Dir { path: p, .. }, .. }
                    if g == group && p.starts_with(&inside))
            });
            if joined.is_some() {
                return joined;
            }
            open_file
        }
    };
    rows.iter()
        .position(|r| r.at() == Some(At::File(file)))
        .or_else(|| {
            rows.iter().rposition(|r| {
                matches!(r, Row::Tree { line: Line::Dir { files, .. }, .. } if files.contains(&file))
            })
        })
        .or_else(|| rows.iter().position(|r| r.at().is_some()))
}

/// What `code` does in the tree with the cursor on row `cursor`.
fn motion(rows: &[Row], cursor: usize, code: KeyCode, n: usize, half: usize) -> Option<Motion> {
    let stops: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].at().is_some()).collect();
    let here = stops.iter().position(|&s| s >= cursor).unwrap_or(0);
    let last = stops.len().checked_sub(1)?;
    let depth = |row: &Row| match row {
        Row::Tree { line, .. } => line.depth(),
        Row::Header(_) => 0,
    };
    let parent = || {
        let (group, d) = (rows[cursor].group(), depth(&rows[cursor]));
        rows[..cursor].iter().rposition(|r| {
            r.dir_key().is_some() && r.group() == group && depth(r) < d
        })
    };
    Some(match (code, &rows[cursor]) {
        (KeyCode::Char('j') | KeyCode::Down, _) => Motion::Go(stops[(here + n).min(last)]),
        (KeyCode::Char('k') | KeyCode::Up, _) => Motion::Go(stops[here.saturating_sub(n)]),
        // `Ctrl-d` / `Ctrl-u` come here as these two.
        (KeyCode::PageDown, _) => Motion::Go(stops[(here + half).min(last)]),
        (KeyCode::PageUp, _) => Motion::Go(stops[here.saturating_sub(half)]),
        (KeyCode::Char('G'), _) => Motion::Go(stops[last]),
        (KeyCode::Home, _) => Motion::Go(stops[0]),
        (KeyCode::Char('h') | KeyCode::Left, row) => match row.dir_key() {
            Some((_, false)) => Motion::Fold(cursor, true),
            Some((_, true)) => Motion::Go(parent()?),
            None => Motion::Fold(parent()?, true),
        },
        (KeyCode::Char('l') | KeyCode::Right, row) => match (row.dir_key(), row.at()) {
            (Some((_, true)), _) => Motion::Fold(cursor, false),
            (Some((_, false)), _) => Motion::Go(stops[(here + 1).min(last)]),
            (None, Some(At::File(i))) => Motion::Open(i),
            _ => return None,
        },
        (KeyCode::Enter, row) => match (row.dir_key(), row.at()) {
            (Some((_, folded)), _) => Motion::Fold(cursor, !folded),
            (None, Some(At::File(i))) => Motion::Open(i),
            _ => return None,
        },
        _ => return None,
    })
}

/// `Ctrl-d` / `Ctrl-u` as [`motion`] takes them.
fn half_page(code: KeyCode) -> KeyCode {
    match code {
        KeyCode::Char('d') => KeyCode::PageDown,
        _ => KeyCode::PageUp,
    }
}

/// The first row shown in a box of `capacity` rows with the cursor on `at`.
pub(super) fn offset_for(at: usize, capacity: usize) -> usize {
    (at + 1).saturating_sub(capacity.max(1))
}

impl Session {
    /// Runs of the displayed list that share a Group: one tree each. Without
    /// Groups the whole list is one tree.
    fn group_runs(&self) -> Vec<(Option<usize>, Range<usize>)> {
        let mut runs: Vec<(Option<usize>, Range<usize>)> = Vec::new();
        for (i, file) in self.files.iter().enumerate() {
            let group = self.group_of(&file.path);
            match runs.last_mut() {
                Some((g, run)) if *g == group => run.end = i + 1,
                _ => runs.push((group, i..i + 1)),
            }
        }
        runs
    }

    /// The files reordered as their trees show them, Group by Group.
    pub(super) fn in_tree_order(&self, files: Vec<ChangedFile>) -> Vec<ChangedFile> {
        let mut out = Vec::with_capacity(files.len());
        let mut start = 0;
        while start < files.len() {
            let group = self.group_of(&files[start].path);
            let end = (start..files.len())
                .find(|&i| self.group_of(&files[i].path) != group)
                .unwrap_or(files.len());
            let paths: Vec<&str> = files[start..end].iter().map(|f| f.path.as_str()).collect();
            out.extend(
                file_tree::order(&paths)
                    .into_iter()
                    .map(|i| files[start + i].clone()),
            );
            start = end;
        }
        out
    }

    /// The visible rows of the panel for rows of `width` columns after the
    /// cursor mark.
    pub(super) fn tree_rows(&self, width: usize) -> Vec<Row> {
        let mut rows = Vec::new();
        for (group, run) in self.group_runs() {
            if let Some(g) = group {
                rows.push(Row::Header(g));
            }
            let paths: Vec<&str> = self.files[run.clone()]
                .iter()
                .map(|f| f.path.as_str())
                .collect();
            let lines = file_tree::build(&paths, width);
            let folded = |path: &str| self.panel.folded.contains(&(group, path.to_string()));
            for line in file_tree::visible(&lines, folded) {
                let mut line = line.clone();
                match &mut line {
                    Line::File { index, .. } => *index += run.start,
                    Line::Dir { files, .. } => files.iter_mut().for_each(|i| *i += run.start),
                }
                let folded = matches!(&line, Line::Dir { path, .. } if folded(path));
                rows.push(Row::Tree {
                    group,
                    line,
                    folded,
                });
            }
        }
        rows
    }

    /// The columns a tree row of the sidebar has after its cursor mark.
    fn sidebar_tree_width(&self) -> usize {
        (self.files_area().width as usize).saturating_sub(3)
    }

    pub(super) fn sidebar_rows(&self) -> Vec<Row> {
        self.tree_rows(self.sidebar_tree_width())
    }

    /// Where the sidebar cursor stands.
    pub(super) fn sidebar_at(&self) -> At {
        match &self.panel.dir {
            Some(dir) => At::Dir(dir.clone()),
            None => At::File(self.selected),
        }
    }

    /// The row of the sidebar cursor.
    pub(super) fn sidebar_cursor(&self, rows: &[Row]) -> Option<usize> {
        cursor_row(rows, &self.sidebar_at(), self.selected)
    }

    /// The row of the picker cursor.
    pub(super) fn picker_cursor(&self, rows: &[Row], at: &At) -> Option<usize> {
        cursor_row(rows, at, self.selected)
    }

    /// Selecting a file shows it: the directories above it unfold.
    pub(super) fn unfold_to(&mut self, index: usize) {
        let Some(path) = self.files.get(index).map(|f| f.path.clone()) else {
            return;
        };
        let group = self.group_of(&path);
        for dir in file_tree::ancestors(&path) {
            self.panel.folded.remove(&(group, dir.to_string()));
        }
    }

    fn set_folded(&mut self, key: DirKey, folded: bool) {
        if folded {
            self.panel.folded.insert(key);
        } else {
            self.panel.folded.remove(&key);
        }
    }

    /// Put the sidebar cursor on `at`; a file is opened in the diff.
    fn put_sidebar(&mut self, at: At) {
        match at {
            At::File(i) => self.select(i),
            At::Dir(key) => self.panel.dir = Some(key),
        }
    }

    /// A click (or `Enter`) on row `row` of the sidebar.
    pub(super) fn click_sidebar_row(&mut self, row: &Row) {
        match row {
            Row::Header(n) => self.open_group(*n),
            _ => match (row.dir_key(), row.at()) {
                (Some((key, folded)), _) => {
                    self.set_folded(key.clone(), !folded);
                    self.panel.dir = Some(key);
                }
                (None, Some(at)) => self.put_sidebar(at),
                _ => {}
            },
        }
    }

    /// Open the first file of Group `n`.
    fn open_group(&mut self, n: usize) {
        if let Some(i) = (0..self.files.len()).find(|&i| self.group_of(&self.files[i].path) == Some(n))
        {
            self.select(i);
        }
    }

    /// Keys of the focused file panel: `j/k` and the arrows walk the rows,
    /// `gg`/`G` and `Ctrl-d/u` jump, `h`/`l` fold, `Enter` folds a directory
    /// or opens a file. Returns whether the key was used.
    pub(super) fn files_key(&mut self, key: KeyEvent) -> bool {
        let rows = self.sidebar_rows();
        let Some(cursor) = self.sidebar_cursor(&rows) else {
            return false;
        };
        let last = self.files.len().saturating_sub(1);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let half = (self.sidebar_capacity() / 2).max(1);
        let found = if self.pending == Some('g') && key.code == KeyCode::Char('g') && !ctrl {
            self.pending = None;
            match self.count.take() {
                Some(n) => {
                    self.select((n - 1).min(last));
                    return true;
                }
                None => motion(&rows, cursor, KeyCode::Home, 1, half),
            }
        } else if ctrl && matches!(key.code, KeyCode::Char('d' | 'u')) && self.pending.is_none() {
            self.count = None;
            motion(&rows, cursor, half_page(key.code), 1, half)
        } else if ctrl || self.pending.is_some() {
            return false;
        } else if key.code == KeyCode::Char('G') && self.count.is_some() {
            let n = self.count.take().unwrap_or(1);
            self.select(n.saturating_sub(1).min(last));
            return true;
        } else {
            let n = self.count.unwrap_or(1);
            let found = motion(&rows, cursor, key.code, n, half);
            if found.is_some() {
                self.count = None;
            }
            found
        };
        let Some(found) = found else {
            return false;
        };
        match found {
            Motion::Go(r) => {
                if let Some(at) = rows[r].at() {
                    self.put_sidebar(at);
                }
            }
            Motion::Fold(r, folded) => {
                if let Some((key, _)) = rows[r].dir_key() {
                    self.set_folded(key.clone(), folded);
                    self.panel.dir = Some(key);
                }
            }
            Motion::Open(i) => {
                self.select(i);
                self.focus = Focus::Diff;
            }
        }
        true
    }

    /// Keys of the open file picker: the keys of the file panel, with `Enter`
    /// on a file opening it and closing the picker, and `Esc` closing it.
    pub(super) fn file_picker_key(&mut self, key: KeyEvent, at: At) {
        let rows = self.picker_rows();
        let half = (self.picker_capacity() / 2).max(1);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(cursor) = self.picker_cursor(&rows, &at) else {
            if key.code == KeyCode::Esc {
                self.file_picker = None;
            }
            return;
        };
        let found = match (self.pending.take(), key.code) {
            (Some('g'), KeyCode::Char('g')) => motion(&rows, cursor, KeyCode::Home, 1, half),
            (Some(_), _) => return,
            (None, KeyCode::Char(c @ (' ' | 'g'))) if !ctrl => {
                self.pending = Some(c);
                return;
            }
            (None, KeyCode::Esc) => {
                self.file_picker = None;
                return;
            }
            (None, code @ KeyCode::Char('d' | 'u')) if ctrl => {
                motion(&rows, cursor, half_page(code), 1, half)
            }
            (None, _) if ctrl => return,
            (None, code) => motion(&rows, cursor, code, 1, half),
        };
        match found {
            Some(Motion::Go(r)) => self.file_picker = rows[r].at().or(Some(at)),
            Some(Motion::Fold(r, folded)) => {
                if let Some((key, _)) = rows[r].dir_key() {
                    self.set_folded(key.clone(), folded);
                    self.file_picker = Some(At::Dir(key));
                }
            }
            Some(Motion::Open(i)) => {
                self.file_picker = None;
                self.select(i);
            }
            None => {}
        }
    }

    /// The wheel in the picker: one row a notch.
    pub(super) fn picker_wheel(&mut self, down: bool) {
        let Some(at) = self.file_picker.clone() else {
            return;
        };
        let rows = self.picker_rows();
        let Some(cursor) = self.picker_cursor(&rows, &at) else {
            return;
        };
        let code = if down { KeyCode::Down } else { KeyCode::Up };
        if let Some(Motion::Go(r)) = motion(&rows, cursor, code, 1, 1) {
            self.file_picker = rows[r].at();
        }
    }

    /// A click on row `row` of the picker.
    pub(super) fn click_picker_row(&mut self, row: &Row) {
        match row {
            Row::Header(n) => {
                self.file_picker = None;
                self.open_group(*n);
            }
            _ => match (row.dir_key(), row.at()) {
                (Some((key, folded)), _) => {
                    self.set_folded(key.clone(), !folded);
                    self.file_picker = Some(At::Dir(key));
                }
                (None, Some(At::File(i))) => {
                    self.file_picker = None;
                    self.select(i);
                }
                _ => {}
            },
        }
    }

    /// Draw `rows` from `offset` on, one a line from `y`, in `width` columns
    /// at `x`, the row `cursor` lit.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_tree_rows(
        &self,
        buf: &mut Buffer,
        x: u16,
        y: u16,
        width: usize,
        rows: &[Row],
        offset: usize,
        capacity: usize,
        cursor: Option<usize>,
    ) {
        for (n, row) in rows.iter().enumerate().skip(offset).take(capacity) {
            let y = y + (n - offset) as u16;
            let lit = cursor == Some(n);
            let mut style = Style::new().fg(FG);
            if lit {
                style = style.bg(Color::Indexed(238)).add_modifier(Modifier::BOLD);
            }
            let marker = if lit { '>' } else { ' ' };
            match row {
                Row::Header(g) => self.render_group_row(buf, x, y, width, *g),
                Row::Tree {
                    line: Line::File {
                        indent,
                        name,
                        index,
                        ..
                    },
                    ..
                } => {
                    let file = &self.files[*index];
                    if *index == self.selected {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    let reviewed = match self.mark_of(*index) {
                        Mark::Reviewed => '✓',
                        Mark::Partial => '◐',
                        Mark::Unreviewed => ' ',
                    };
                    let pad_in = " ".repeat(*indent);
                    let line = format!("{marker}{pad_in}{reviewed} {} {name}", file.status.letter());
                    buf.set_stringn(x, y, pad(&line, width), width, style);
                    let color = match file.status {
                        Status::Added => Color::Indexed(114),
                        Status::Deleted => Color::Indexed(174),
                        Status::Modified => Color::Indexed(179),
                        Status::Renamed => Color::Indexed(110),
                    };
                    let letter = x + (1 + indent + 2) as u16;
                    if (letter as usize) < x as usize + width {
                        buf.set_style(Rect::new(letter, y, 1, 1), style.fg(color));
                    }
                }
                Row::Tree {
                    line:
                        Line::Dir {
                            indent,
                            label,
                            files,
                            ..
                        },
                    folded,
                    ..
                } => {
                    let done = files
                        .iter()
                        .filter(|&&i| self.mark_of(i) == Mark::Reviewed)
                        .count();
                    let progress = format!("{done}/{}", files.len());
                    let fold = if *folded { '▸' } else { '▾' };
                    let head = format!("{marker}{}{fold} {label}", " ".repeat(*indent));
                    let room = width.saturating_sub(progress.len() + 1);
                    let line = format!("{}{progress} ", pad(&fit(&head, room, false), room));
                    buf.set_stringn(x, y, &line, width, style.fg(Color::Indexed(110)));
                }
            }
        }
    }

    /// The Group of the row under the cursor of the file panel or picker,
    /// when Groups are shown.
    pub(super) fn cursor_group(&self) -> Option<usize> {
        self.grouping_shown().then_some(())?;
        if let Some(at) = &self.file_picker {
            let rows = self.picker_rows();
            return rows.get(self.picker_cursor(&rows, at)?)?.group();
        }
        if self.window() == Focus::Files {
            let rows = self.sidebar_rows();
            return rows.get(self.sidebar_cursor(&rows)?)?.group();
        }
        let file = self.files.get(self.selected)?;
        self.group_of(&file.path)
    }

    /// The one-line description of the Group under the cursor, for the hint
    /// line of the file panel and picker.
    pub(super) fn group_hint(&self) -> Option<String> {
        let n = self.cursor_group()?;
        let (name, description, _) = self.group_text(n);
        Some(format!("{} {name}: {description}", n + 1))
    }

    // ------------------------------------------------------- description

    pub(super) fn group_info_open(&self) -> bool {
        self.panel.info.is_some()
    }

    /// `Space i`: show or hide the description of the Group under the cursor.
    pub(super) fn toggle_group_info(&mut self) {
        if self.panel.info.take().is_some() {
            return;
        }
        match self.cursor_group() {
            Some(n) => self.panel.info = Some((n, scroll::Scroll::default())),
            None => self.message = Some("No Groups: nothing to describe".into()),
        }
    }

    /// Keys while the description is open: scrolling, `Space i`, `Esc` and
    /// `q` close it. Returns whether the key was taken.
    pub(super) fn group_info_key(&mut self, key: KeyEvent) -> bool {
        let Some((n, mut scroll)) = self.panel.info else {
            return false;
        };
        if self.pending.take() == Some(' ') {
            if key.code == KeyCode::Char('i') {
                self.panel.info = None;
            }
            return true;
        }
        let (total, rows) = (
            self.group_info_lines().len(),
            scroll::text_rows(self.overlay_area()),
        );
        if key.code == KeyCode::Char(' ') {
            self.pending = Some(' ');
        } else if scroll.key(key, total, rows) {
            self.panel.info = Some((n, scroll));
        } else if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            self.panel.info = None;
        }
        true
    }

    pub(super) fn group_info_lines(&self) -> Vec<String> {
        let Some((n, _)) = self.panel.info else {
            return Vec::new();
        };
        super::analysis::wrap(&self.group_text(n).1, scroll::text_cols(self.overlay_area()))
    }

    pub(super) fn group_info_scroll_mut(&mut self) -> Option<&mut scroll::Scroll> {
        self.panel.info.as_mut().map(|(_, scroll)| scroll)
    }

    pub(super) fn render_group_info(&self, buf: &mut Buffer) {
        let Some((n, scroll)) = &self.panel.info else {
            return;
        };
        let area = self.overlay_area();
        clear_area(buf, area);
        let (name, _, _) = self.group_text(*n);
        draw_box(buf, area, &format!(" Group {} {name} (Esc закрыть) ", n + 1));
        scroll::render_lines(buf, area, &self.group_info_lines(), scroll);
    }
}

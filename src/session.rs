//! The review session: the main seam of the application.
//!
//! A [`Session`] opens the current branch of a git clone as a Range, is driven
//! by terminal events (keys, mouse, resize) and renders to a cell [`Frame`]
//! that can be read back as text. It never touches a real terminal, so the
//! same code runs under the real loop in `main.rs` and under test harnesses.

use std::fmt;
use std::path::Path;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::diff::{diff_file, FileDiff, FileKind, Highlight, Limits, Side, Token};
use crate::forge::{Forge, PullRequest};
use crate::git::{ChangedFile, GitError, Range, Repo, Status, Target};
use crate::special;

/// Below this size the session shows a "terminal too small" screen.
pub const MIN_SIZE: (u16, u16) = (60, 15);
/// Width of the file list on the left.
const SIDEBAR_WIDTH: u16 = 32;
/// The diff pane is shown side-by-side from this width; narrower is inline.
pub const SIDE_BY_SIDE_MIN_WIDTH: u16 = 90;
const TAB_WIDTH: usize = 4;
const WHEEL_STEP: usize = 3;

const FG: Color = Color::Indexed(252);
const DIM: Color = Color::Indexed(244);
const ACCENT: Color = Color::Indexed(75);
const REMOVED_BG: Color = Color::Indexed(52);
const REMOVED_FRAGMENT_BG: Color = Color::Indexed(88);
const ADDED_BG: Color = Color::Indexed(22);
const ADDED_FRAGMENT_BG: Color = Color::Indexed(28);

#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<GitError> for Error {
    fn from(e: GitError) -> Self {
        Error(e.0)
    }
}

/// How the diff of a file is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLayout {
    SideBySide,
    Inline,
}

/// A rendered screen: a grid of styled cells.
pub struct Frame {
    buffer: Buffer,
}

impl Frame {
    /// The screen as text: one line per row, trailing blanks trimmed.
    pub fn text(&self) -> String {
        let area = self.buffer.area;
        (0..area.height)
            .map(|y| {
                let row: String = (0..area.width)
                    .map(|x| self.buffer[(x, y)].symbol())
                    .collect();
                row.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Foreground colour of the cell.
    pub fn fg_at(&self, x: u16, y: u16) -> Color {
        self.buffer[(x, y)].fg
    }

    /// Background colour of the cell.
    pub fn bg_at(&self, x: u16, y: u16) -> Color {
        self.buffer[(x, y)].bg
    }

    /// Position (column, row) of the first occurrence of `needle` in a row.
    pub fn find(&self, needle: &str) -> Option<(u16, u16)> {
        let area = self.buffer.area;
        for y in 0..area.height {
            let row: Vec<&str> = (0..area.width)
                .map(|x| self.buffer[(x, y)].symbol())
                .collect();
            let joined: String = row.concat();
            if let Some(byte) = joined.find(needle) {
                let col = joined[..byte].chars().count() as u16;
                return Some((col, y));
            }
        }
        None
    }
}

/// Per-line tokens of one side, for highlighting.
type LineTokens = Vec<Vec<Token>>;

struct Loaded {
    diff: FileDiff,
    old_tokens: LineTokens,
    new_tokens: LineTokens,
}

impl Loaded {
    fn new(diff: FileDiff) -> Loaded {
        let group = |side: Side, lines: usize| -> LineTokens {
            let mut by_line = vec![Vec::new(); lines];
            for t in diff.tokens(side) {
                by_line[t.line as usize].push(t.clone());
            }
            by_line
        };
        Loaded {
            old_tokens: group(Side::Old, diff.old_lines.len()),
            new_tokens: group(Side::New, diff.new_lines.len()),
            diff,
        }
    }

    fn tokens(&self, side: Side) -> &LineTokens {
        match side {
            Side::Old => &self.old_tokens,
            Side::New => &self.new_tokens,
        }
    }

    fn lines(&self, side: Side) -> &[String] {
        match side {
            Side::Old => &self.diff.old_lines,
            Side::New => &self.diff.new_lines,
        }
    }

    /// Whether a line of the side is part of the change.
    fn line_changed(&self, side: Side, line: usize, counterpart: Option<usize>) -> bool {
        let Some(other) = counterpart else {
            return true;
        };
        let other_side = match side {
            Side::Old => Side::New,
            Side::New => Side::Old,
        };
        self.tokens(side)[line].iter().any(|t| t.changed)
            || self.lines(side)[line] != self.lines(other_side)[other]
    }
}

/// One line of the inline view.
#[derive(Clone, Copy)]
struct InlineLine {
    old: Option<usize>,
    new: Option<usize>,
    kind: InlineKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InlineKind {
    Context,
    Removed,
    Added,
}

enum Mode {
    Normal,
    Command(String),
}

pub struct Session {
    repo: Repo,
    range: Range,
    files: Vec<ChangedFile>,
    selected: usize,
    loaded: Option<Result<Loaded, String>>,
    scroll: usize,
    size: (u16, u16),
    layout_override: Option<DiffLayout>,
    mode: Mode,
    pending: Option<char>,
    message: Option<String>,
    /// A standing caveat about the Range, shown until the Review closes.
    warning: Option<String>,
    /// The pull request behind the Range, when the Forge knows one.
    pull_request: Option<PullRequest>,
    quit: bool,
}

impl Session {
    /// Open the current branch of the clone containing `repo_path` as a Range.
    pub fn open(repo_path: &Path, width: u16, height: u16) -> Result<Session, Error> {
        Self::open_target(repo_path, &Target::Current, width, height)
    }

    /// Open the Range that `target` names in the clone containing `repo_path`.
    pub fn open_target(
        repo_path: &Path,
        target: &Target,
        width: u16,
        height: u16,
    ) -> Result<Session, Error> {
        let repo = Repo::open(repo_path)?;
        let range = repo.range_for(target)?;
        Self::open_range(repo, range, *target != Target::Current, width, height)
    }

    /// Like [`Session::open_target`], and ask `forge` for the pull request of
    /// the branch. If the Forge cannot answer, the Review opens all the same
    /// and the reason is shown in the status line.
    pub fn open_target_with_forge(
        repo_path: &Path,
        target: &Target,
        forge: &dyn Forge,
        width: u16,
        height: u16,
    ) -> Result<Session, Error> {
        let mut session = Self::open_target(repo_path, target, width, height)?;
        let branch = match target {
            Target::Branch(name) => Some(name.as_str()),
            Target::Current if session.range.head_name != "HEAD" => {
                Some(session.range.head_name.as_str())
            }
            _ => None,
        };
        if let Some(branch) = branch {
            match forge.pull_request_for_branch(branch) {
                Ok(pr) => session.pull_request = pr,
                Err(e) => session.warn(format!("⚠ {e}")),
            }
        }
        Ok(session)
    }

    fn warn(&mut self, text: String) {
        self.warning = Some(match self.warning.take() {
            Some(w) => format!("{w}  {text}"),
            None => text,
        });
    }

    /// Open pull request `number` as a Range: its head is fetched into a ref
    /// of moonhare's own, with no new branch and no checkout.
    pub fn open_pull_request(
        repo_path: &Path,
        forge: &dyn Forge,
        number: u64,
        width: u16,
        height: u16,
    ) -> Result<Session, Error> {
        let repo = Repo::open(repo_path)?;
        let pr = forge
            .pull_request(number)
            .map_err(|e| Error(e.to_string()))?;
        let reference = forge
            .fetch_pull_request_head(repo.root(), number)
            .map_err(|e| Error(e.to_string()))?;
        let range =
            repo.range_for_head(&format!("#{number}"), &reference, Some(&pr.base_branch))?;
        let mut session = Self::open_range(repo, range, true, width, height)?;
        session.pull_request = Some(pr);
        Ok(session)
    }

    fn open_range(
        repo: Repo,
        range: Range,
        reject_empty: bool,
        width: u16,
        height: u16,
    ) -> Result<Session, Error> {
        if reject_empty && repo.commit_count(&range.base, &range.head)? == 0 {
            return Err(Error(format!(
                "empty range: no commits between {} and {}",
                range.base_name, range.head_name
            )));
        }
        let files = repo.changed_files(&range.base, &range.head)?;
        let merges = repo.merge_commit_count(&range.base, &range.head)?;
        let warning = (merges > 0).then(|| {
            format!("⚠ {merges} merge commit(s) in range, compared against the first parent")
        });
        let mut session = Session {
            repo,
            range,
            files,
            selected: 0,
            loaded: None,
            scroll: 0,
            size: (width, height),
            layout_override: None,
            mode: Mode::Normal,
            pending: None,
            message: None,
            warning,
            pull_request: None,
            quit: false,
        };
        session.load_selected();
        Ok(session)
    }

    /// The pull request the Range belongs to: its title and description.
    pub fn pull_request(&self) -> Option<&PullRequest> {
        self.pull_request.as_ref()
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// The layout in effect: the manual choice if any, else by width.
    pub fn layout(&self) -> DiffLayout {
        self.layout_override.unwrap_or_else(|| {
            if self.diff_area().width >= SIDE_BY_SIDE_MIN_WIDTH {
                DiffLayout::SideBySide
            } else {
                DiffLayout::Inline
            }
        })
    }

    // ------------------------------------------------------------ geometry

    fn too_small(&self) -> bool {
        self.size.0 < MIN_SIZE.0 || self.size.1 < MIN_SIZE.1
    }

    fn sidebar_area(&self) -> Rect {
        Rect::new(
            0,
            0,
            SIDEBAR_WIDTH.min(self.size.0),
            self.size.1.saturating_sub(1),
        )
    }

    /// The diff pane including its header line.
    fn diff_area(&self) -> Rect {
        let x = SIDEBAR_WIDTH.min(self.size.0);
        Rect::new(x, 0, self.size.0 - x, self.size.1.saturating_sub(1))
    }

    /// Rows available to diff lines (the pane minus its header).
    fn viewport_height(&self) -> usize {
        self.diff_area().height.saturating_sub(1) as usize
    }

    /// Rows available to the file list (the pane minus its border).
    fn sidebar_capacity(&self) -> usize {
        self.sidebar_area().height.saturating_sub(2) as usize
    }

    fn sidebar_offset(&self) -> usize {
        let cap = self.sidebar_capacity().max(1);
        (self.selected + 1).saturating_sub(cap)
    }

    // ------------------------------------------------------------ data

    fn load_selected(&mut self) {
        self.scroll = 0;
        self.loaded = self.files.get(self.selected).map(|f| {
            let blob = |rev: &str, path: &str| {
                self.repo
                    .file_at(rev, path)
                    .map(Some)
                    .map_err(|e| e.to_string())
            };
            let old = match f.status {
                Status::Added => Ok(None),
                _ => blob(&self.range.base, f.base_path()),
            };
            let new = match f.status {
                Status::Deleted => Ok(None),
                _ => blob(&self.range.head, &f.path),
            };
            match (old, new) {
                (Ok(old), Ok(new)) => Ok(Loaded::new(diff_file(
                    &f.path,
                    old.as_deref(),
                    new.as_deref(),
                    Limits::default(),
                ))),
                (Err(e), _) | (_, Err(e)) => Err(e),
            }
        });
    }

    fn select(&mut self, index: usize) {
        if index < self.files.len() && index != self.selected {
            self.selected = index;
            self.load_selected();
        }
    }

    fn inline_lines(&self) -> Vec<InlineLine> {
        let Some(Ok(loaded)) = &self.loaded else {
            return vec![];
        };
        let mut out = Vec::new();
        for row in &loaded.diff.rows {
            let changed = |side: Side, line: Option<usize>, other: Option<usize>| {
                line.is_some_and(|l| loaded.line_changed(side, l, other))
            };
            let old_changed = changed(Side::Old, row.old, row.new);
            let new_changed = changed(Side::New, row.new, row.old);
            if !old_changed && !new_changed {
                out.push(InlineLine {
                    old: row.old,
                    new: row.new,
                    kind: InlineKind::Context,
                });
                continue;
            }
            if row.old.is_some() {
                out.push(InlineLine {
                    old: row.old,
                    new: None,
                    kind: InlineKind::Removed,
                });
            }
            if row.new.is_some() {
                out.push(InlineLine {
                    old: None,
                    new: row.new,
                    kind: InlineKind::Added,
                });
            }
        }
        out
    }

    fn line_count(&self) -> usize {
        match (&self.loaded, self.layout()) {
            (Some(Ok(l)), DiffLayout::SideBySide) => l.diff.rows.len(),
            (Some(Ok(_)), DiffLayout::Inline) => self.inline_lines().len(),
            _ => 0,
        }
    }

    fn max_scroll(&self) -> usize {
        self.line_count().saturating_sub(self.viewport_height())
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
    }

    fn scroll_by(&mut self, delta: isize) {
        let target = self.scroll as isize + delta;
        self.scroll = target.clamp(0, self.max_scroll() as isize) as usize;
    }

    // ------------------------------------------------------------ events

    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            Event::Mouse(mouse) => self.on_mouse(mouse),
            Event::Resize(w, h) => {
                self.size = (w, h);
            }
            _ => {}
        }
        self.clamp_scroll();
    }

    fn on_key(&mut self, key: KeyEvent) {
        if let Mode::Command(buf) = &mut self.mode {
            match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Backspace => {
                    if buf.pop().is_none() {
                        self.mode = Mode::Normal;
                    }
                }
                KeyCode::Enter => {
                    let command = std::mem::take(buf);
                    self.mode = Mode::Normal;
                    self.run_command(command.trim());
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => buf.push(c),
                _ => {}
            }
            return;
        }

        self.message = None;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let half_page = (self.viewport_height() / 2).max(1) as isize;

        if let Some(prefix) = self.pending.take() {
            match (prefix, key.code) {
                ('g', KeyCode::Char('g')) => self.scroll = 0,
                (']', KeyCode::Char('f')) => self.select(self.selected + 1),
                ('[', KeyCode::Char('f')) => self.select(self.selected.saturating_sub(1)),
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Char('d') if ctrl => self.scroll_by(half_page),
            KeyCode::Char('u') if ctrl => self.scroll_by(-half_page),
            KeyCode::Char('j') | KeyCode::Down if !ctrl => self.scroll_by(1),
            KeyCode::Char('k') | KeyCode::Up if !ctrl => self.scroll_by(-1),
            KeyCode::Char('G') => self.scroll = self.max_scroll(),
            KeyCode::Char(c @ ('g' | ']' | '[')) if !ctrl => self.pending = Some(c),
            KeyCode::Char('s') if !ctrl => {
                // The manual choice wins over the width-based default from now on.
                self.layout_override = Some(match self.layout() {
                    DiffLayout::SideBySide => DiffLayout::Inline,
                    DiffLayout::Inline => DiffLayout::SideBySide,
                });
            }
            KeyCode::Char(':') if !ctrl => self.mode = Mode::Command(String::new()),
            _ => {}
        }
    }

    fn run_command(&mut self, command: &str) {
        match command {
            "q" | "q!" | "quit" => self.quit = true,
            "" => {}
            other => self.message = Some(format!("Not an editor command: {other}")),
        }
    }

    fn on_mouse(&mut self, mouse: MouseEvent) {
        let at = |area: Rect| area.contains((mouse.column, mouse.row).into());
        match mouse.kind {
            MouseEventKind::ScrollDown if at(self.diff_area()) => {
                self.scroll_by(WHEEL_STEP as isize)
            }
            MouseEventKind::ScrollUp if at(self.diff_area()) => {
                self.scroll_by(-(WHEEL_STEP as isize))
            }
            MouseEventKind::Down(_) if at(self.sidebar_area()) => {
                let area = self.sidebar_area();
                // Row 0 of the sidebar is its border.
                if mouse.row > area.y {
                    let index = self.sidebar_offset() + (mouse.row - area.y - 1) as usize;
                    if (mouse.row - area.y - 1) as usize <= self.sidebar_capacity() {
                        self.select(index);
                    }
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ rendering

    pub fn frame(&mut self) -> Frame {
        let mut buffer = Buffer::empty(Rect::new(0, 0, self.size.0, self.size.1));
        self.render(&mut buffer);
        Frame { buffer }
    }

    /// Draw the whole screen into `buf`, whose area is the terminal.
    pub fn render(&self, buf: &mut Buffer) {
        buf.set_style(buf.area, Style::new().fg(FG));
        if self.too_small() {
            let msg = format!(
                "Terminal too small: need at least {}x{}",
                MIN_SIZE.0, MIN_SIZE.1
            );
            buf.set_stringn(0, 0, &msg, self.size.0 as usize, Style::new().fg(FG));
            return;
        }
        self.render_sidebar(buf);
        self.render_diff(buf);
        self.render_status(buf);
    }

    fn render_sidebar(&self, buf: &mut Buffer) {
        let area = self.sidebar_area();
        let title = format!(" Files ({}) ", self.files.len());
        let border = Style::new().fg(DIM);
        let width = area.width as usize;
        // Top border with title, right border as a column.
        let mut top = format!("┌{title}");
        while top.chars().count() < width.saturating_sub(1) {
            top.push('─');
        }
        top.push('┐');
        buf.set_stringn(area.x, area.y, &top, width, border);
        for y in 1..area.height.saturating_sub(1) {
            buf.set_string(area.x, area.y + y, "│", border);
            buf.set_string(area.x + area.width - 1, area.y + y, "│", border);
        }
        let bottom = format!("└{}┘", "─".repeat(width.saturating_sub(2)));
        buf.set_stringn(area.x, area.y + area.height - 1, &bottom, width, border);

        let inner = width.saturating_sub(2);
        let offset = self.sidebar_offset();
        for (row, (i, file)) in self
            .files
            .iter()
            .enumerate()
            .skip(offset)
            .take(self.sidebar_capacity())
            .enumerate()
        {
            let y = area.y + 1 + row as u16;
            let selected = i == self.selected;
            let color = match file.status {
                Status::Added => Color::Indexed(114),
                Status::Deleted => Color::Indexed(174),
                Status::Modified => Color::Indexed(179),
                Status::Renamed => Color::Indexed(110),
            };
            let mut style = Style::new().fg(FG);
            if selected {
                style = style.bg(Color::Indexed(238)).add_modifier(Modifier::BOLD);
            }
            let marker = if selected { '>' } else { ' ' };
            let path = fit(&file.path, inner.saturating_sub(4), true);
            let line = format!("{marker} {} {path}", file.status.letter());
            buf.set_stringn(area.x + 1, y, pad(&line, inner), inner, style);
            buf.set_style(Rect::new(area.x + 3, y, 1, 1), style.fg(color));
        }
    }

    fn render_diff(&self, buf: &mut Buffer) {
        let area = self.diff_area();
        let w = area.width as usize;
        let Some(file) = self.files.get(self.selected) else {
            let msg = format!(
                "No changes between {} and {}",
                self.range.base_name, self.range.head_name
            );
            buf.set_stringn(
                area.x + 1,
                area.y + 1,
                &msg,
                w.saturating_sub(1),
                Style::new().fg(DIM),
            );
            return;
        };
        let Some(loaded) = &self.loaded else { return };
        let loaded = match loaded {
            Ok(l) => l,
            Err(e) => {
                buf.set_stringn(
                    area.x + 1,
                    area.y + 1,
                    e,
                    w.saturating_sub(1),
                    Style::new().fg(Color::Red),
                );
                return;
            }
        };

        let d = &loaded.diff;
        let header = special::header(file, d);
        let header_style = Style::new()
            .fg(Color::Indexed(231))
            .bg(Color::Indexed(236))
            .add_modifier(Modifier::BOLD);
        buf.set_style(Rect::new(area.x, area.y, area.width, 1), header_style);
        buf.set_stringn(area.x, area.y, &header, w, header_style);

        let top = area.y + 1;
        let height = self.viewport_height();
        if d.kind == FileKind::Binary {
            buf.set_stringn(
                area.x + 1,
                top,
                "Binary file not shown",
                w,
                Style::new().fg(DIM),
            );
            return;
        }
        match self.layout() {
            DiffLayout::SideBySide => self.render_side_by_side(buf, loaded, area, top, height),
            DiffLayout::Inline => self.render_inline(buf, loaded, area, top, height),
        }
    }

    fn render_side_by_side(
        &self,
        buf: &mut Buffer,
        l: &Loaded,
        area: Rect,
        top: u16,
        height: usize,
    ) {
        let digits = digits(l.diff.old_lines.len().max(l.diff.new_lines.len()));
        let half = (area.width as usize).saturating_sub(1) / 2;
        let right_x = area.x + half as u16 + 1;
        let right_w = area.width as usize - half - 1;
        for (screen_row, row) in l
            .diff
            .rows
            .iter()
            .skip(self.scroll)
            .take(height)
            .enumerate()
        {
            let y = top + screen_row as u16;
            buf.set_string(area.x + half as u16, y, "│", Style::new().fg(DIM));
            self.render_half(buf, l, Side::Old, row.old, row.new, area.x, y, half, digits);
            self.render_half(
                buf,
                l,
                Side::New,
                row.new,
                row.old,
                right_x,
                y,
                right_w,
                digits,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_half(
        &self,
        buf: &mut Buffer,
        l: &Loaded,
        side: Side,
        line: Option<usize>,
        counterpart: Option<usize>,
        x: u16,
        y: u16,
        width: usize,
        digits: usize,
    ) {
        let Some(line) = line else { return };
        let changed = l.line_changed(side, line, counterpart);
        let (bg, frag_bg) = match (side, changed) {
            (_, false) => (None, None),
            (Side::Old, true) => (Some(REMOVED_BG), Some(REMOVED_FRAGMENT_BG)),
            (Side::New, true) => (Some(ADDED_BG), Some(ADDED_FRAGMENT_BG)),
        };
        if let Some(bg) = bg {
            buf.set_style(Rect::new(x, y, width as u16, 1), Style::new().bg(bg));
        }
        let gutter = format!("{:>digits$} ", line + 1);
        let mut gutter_style = Style::new().fg(DIM);
        if let Some(bg) = bg {
            gutter_style = gutter_style.bg(bg);
        }
        buf.set_stringn(x, y, &gutter, width, gutter_style);
        let code_x = x + gutter.chars().count() as u16;
        let code_w = width.saturating_sub(gutter.chars().count());
        let cells = code_cells(&l.lines(side)[line], &l.tokens(side)[line], bg, frag_bg);
        draw_cells(buf, code_x, y, code_w, &cells);
    }

    fn render_inline(&self, buf: &mut Buffer, l: &Loaded, area: Rect, top: u16, height: usize) {
        let digits = digits(l.diff.old_lines.len().max(l.diff.new_lines.len()));
        let lines = self.inline_lines();
        let width = area.width as usize;
        for (screen_row, line) in lines.iter().skip(self.scroll).take(height).enumerate() {
            let y = top + screen_row as u16;
            let (side, idx, bg, frag_bg, mark) = match line.kind {
                InlineKind::Context => (Side::New, line.new.unwrap(), None, None, ' '),
                InlineKind::Removed => (
                    Side::Old,
                    line.old.unwrap(),
                    Some(REMOVED_BG),
                    Some(REMOVED_FRAGMENT_BG),
                    '-',
                ),
                InlineKind::Added => (
                    Side::New,
                    line.new.unwrap(),
                    Some(ADDED_BG),
                    Some(ADDED_FRAGMENT_BG),
                    '+',
                ),
            };
            if let Some(bg) = bg {
                buf.set_style(Rect::new(area.x, y, area.width, 1), Style::new().bg(bg));
            }
            let num = |n: Option<usize>| match n {
                Some(n) => format!("{:>digits$}", n + 1),
                None => " ".repeat(digits),
            };
            let gutter = format!("{} {} {mark} ", num(line.old), num(line.new));
            let mut gutter_style = Style::new().fg(DIM);
            if let Some(bg) = bg {
                gutter_style = gutter_style.bg(bg);
            }
            buf.set_stringn(area.x, y, &gutter, width, gutter_style);
            let gw = gutter.chars().count();
            let cells = code_cells(&l.lines(side)[idx], &l.tokens(side)[idx], bg, frag_bg);
            draw_cells(buf, area.x + gw as u16, y, width.saturating_sub(gw), &cells);
        }
    }

    fn render_status(&self, buf: &mut Buffer) {
        let y = self.size.1 - 1;
        let width = self.size.0 as usize;
        let bar = Style::new().fg(FG).bg(Color::Indexed(236));
        buf.set_style(Rect::new(0, y, self.size.0, 1), bar);

        if let Mode::Command(cmd) = &self.mode {
            buf.set_stringn(0, y, format!(":{cmd}█"), width, Style::new().fg(FG));
            buf.set_style(
                Rect::new(0, y, self.size.0, 1),
                Style::new().bg(Color::Reset),
            );
            return;
        }
        let badge = Style::new()
            .fg(Color::Indexed(16))
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD);
        buf.set_string(0, y, " NORMAL ", badge);

        let layout = match self.layout() {
            DiffLayout::SideBySide => "side-by-side",
            DiffLayout::Inline => "inline",
        };
        let how = if self.layout_override.is_some() {
            "manual"
        } else {
            "auto"
        };
        let text = match &self.message {
            Some(m) => format!(" {m}"),
            None => format!(
                " {}..{}  file {}/{}  layout:{layout} ({how})",
                self.range.base_name,
                self.range.head_name,
                if self.files.is_empty() {
                    0
                } else {
                    self.selected + 1
                },
                self.files.len(),
            ),
        };
        let text = match &self.warning {
            Some(w) => format!("{text}  {w}"),
            None => text,
        };
        buf.set_stringn(8, y, &text, width.saturating_sub(8), bar);
    }
}

fn digits(n: usize) -> usize {
    n.max(99).to_string().len()
}

fn pad(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - n))
    }
}

/// Fit `s` into `width` columns, cutting with `…` (from the left for paths).
fn fit(s: &str, width: usize, keep_tail: bool) -> String {
    let n = s.chars().count();
    if n <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    if keep_tail {
        let tail: String = s.chars().skip(n - (width - 1)).collect();
        format!("…{tail}")
    } else {
        let head: String = s.chars().take(width - 1).collect();
        format!("{head}…")
    }
}

fn highlight_color(h: Highlight) -> Color {
    match h {
        Highlight::Keyword => Color::Indexed(176),
        Highlight::String => Color::Indexed(150),
        Highlight::Comment => Color::Indexed(243),
        Highlight::Type => Color::Indexed(110),
        Highlight::Error => Color::Indexed(203),
        Highlight::Delimiter => Color::Indexed(247),
        Highlight::Normal | Highlight::Text | Highlight::Ignorable => FG,
    }
}

/// Styled characters of one source line: syntax colours from the tokens, a
/// line background, and a stronger background on changed fragments. Tabs are
/// expanded.
fn code_cells(
    text: &str,
    tokens: &[Token],
    bg: Option<Color>,
    fragment_bg: Option<Color>,
) -> Vec<(char, Style)> {
    let mut cells = Vec::with_capacity(text.len());
    for (byte, ch) in text.char_indices() {
        let mut style = Style::new().fg(FG);
        if let Some(bg) = bg {
            style = style.bg(bg);
        }
        if let Some(t) = tokens
            .iter()
            .find(|t| (t.start_col as usize) <= byte && byte < t.end_col as usize)
        {
            style = style.fg(highlight_color(t.highlight));
            if t.changed {
                if let Some(fbg) = fragment_bg {
                    style = style.bg(fbg);
                }
            }
        }
        if ch == '\t' {
            cells.extend(std::iter::repeat_n((' ', style), TAB_WIDTH));
        } else {
            cells.push((ch, style));
        }
    }
    cells
}

/// Draw cells into `width` columns, ending with `…` when cut.
fn draw_cells(buf: &mut Buffer, x: u16, y: u16, width: usize, cells: &[(char, Style)]) {
    let cut = cells.len() > width;
    let shown = if cut {
        width.saturating_sub(1)
    } else {
        cells.len()
    };
    for (i, (ch, style)) in cells.iter().take(shown).enumerate() {
        buf.set_string(x + i as u16, y, ch.to_string(), *style);
    }
    if cut && width > 0 {
        let style = cells[shown].1.fg(DIM);
        buf.set_string(x + shown as u16, y, "…", style);
    }
}

//! The review session: the main seam of the application.
//!
//! A [`Session`] opens the current branch of a git clone as a Range, is driven
//! by terminal events (keys, mouse, resize) and renders to a cell [`Frame`]
//! that can be read back as text. It never touches a real terminal, so the
//! same code runs under the real loop in `main.rs` and under test harnesses.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::diff::{diff_file, FileDiff, FileKind, Highlight, Limits, Side, Token};
use crate::fold::{fold, Folded, Item};
use crate::forge::{Forge, PullRequest};
use crate::git::{ChangedFile, Commit, GitError, Range, Repo, Status, Target};
use crate::review::{self, FileChange, Mark, Review};
use crate::special;
use crate::viewport::Viewport;
use diagram::clear_area;
use rounds::HEADER;

/// Below this size the session shows a "terminal too small" screen.
pub const MIN_SIZE: (u16, u16) = (60, 15);
/// Width of the file list on the left.
const SIDEBAR_WIDTH: u16 = 32;
/// Terminals at least this wide show the sidebar by default.
pub const SIDEBAR_AUTO_MIN_WIDTH: u16 = 100;
/// The diff pane is shown side-by-side from this width; narrower is inline.
pub const SIDE_BY_SIDE_MIN_WIDTH: u16 = 90;
const TAB_WIDTH: usize = 4;
/// Rows at the bottom of the screen outside the panes.
const BOTTOM_ROWS: u16 = 2;
const WHEEL_STEP: usize = 3;
/// Columns moved by one `h` / `l` press.
const HSCROLL_STEP: usize = 8;

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
    /// Width in cells of the widest line on either side.
    max_width: usize,
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
        let width = |line: &String| line.chars().map(copy::cell_width).sum::<usize>();
        let max_width = (diff.old_lines.iter().chain(&diff.new_lines))
            .map(width)
            .max()
            .unwrap_or(0);
        Loaded {
            max_width,
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

/// The contiguous run of commits whose combined change is displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// The whole Range.
    All,
    /// Commits `lo..=hi` (0-based indices into the commit list).
    Commits { lo: usize, hi: usize },
}

/// Which pane the motion keys act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Diff,
    Commits,
    Files,
}

mod analysis;
mod comments;
mod copy;
mod diagram;
mod explain;
mod file_bar;
mod file_picker;
mod forge_load;
mod groups;
mod long_list;
mod marks;
mod mechanical_view;
mod mouse;
mod nav;
mod overlay;
mod overlay_mouse;
mod prompt;
mod reload;
mod rounds;
mod sidebar;
mod unreviewed;
mod windows;

enum Mode {
    Normal,
    Command(String),
    /// Typing a `/` search in the commit list.
    Search(String),
}

/// Scope ends and Comments revision, with the ids worked out for them.
type HiddenCache = ((String, String, u64), Vec<u64>);

pub struct Session {
    repo: Repo,
    range: Range,
    commits: Vec<Commit>,
    list: long_list::State,
    scope: Scope,
    focus: Focus,
    /// Cursor in the commit list: 0 is the pinned "Весь Range" row, `n` is
    /// commit `n - 1`.
    commit_cursor: usize,
    /// Where `v` started a selection in the commit list (a cursor position,
    /// never the "Весь Range" row).
    commit_anchor: Option<usize>,
    /// The `Space c` commit picker is open.
    picker: bool,
    /// The `Space f` file picker is open, with its cursor on this file.
    file_picker: Option<usize>,
    /// The selected Mechanical file was expanded with `Enter`.
    mechanical_open: bool,
    files: Vec<ChangedFile>,
    /// Every file of the Scope, before the unreviewed filter.
    scope_files: Vec<ChangedFile>,
    /// `Space u`: only the files still to read, with their delta.
    unreviewed_only: bool,
    /// The files of the whole Range, for the Range-wide progress.
    range_files: Vec<ChangedFile>,
    /// The File changes each commit makes, by commit index.
    commit_changes: Vec<Vec<FileChange>>,
    review: Review,
    comments: crate::comments::Comments,
    editor: Option<comments::Editor>,
    /// VISUAL mode in the diff: the view item the selection started on.
    visual: Option<usize>,
    /// `:comments clear` waits for `y` / `n`.
    confirm_clear: bool,
    /// Ids of the Comments hidden in the other files of the Scope, with the
    /// Scope ends and Comments revision they were worked out for.
    hidden_cache: std::cell::RefCell<Option<HiddenCache>>,
    selected: usize,
    loaded: Option<Result<Loaded, String>>,
    vp: Viewport,
    hscroll: usize,
    /// Digits typed so far: the count of the next command.
    count: Option<usize>,
    /// The folded display model of the loaded file, built for `view_layout`.
    view: Folded,
    view_layout: DiffLayout,
    /// Lines of the inline layout; empty in side-by-side.
    inline: Vec<InlineLine>,
    size: (u16, u16),
    layout_override: Option<DiffLayout>,
    /// The Reviewer's `Space e` choice; it beats the width-based default.
    sidebar_manual: Option<bool>,
    /// The width the Reviewer dragged or typed; `None` is the default.
    sidebar_custom: Option<u16>,
    /// Emit exact RGB instead of the 256-colour palette.
    truecolor: bool,
    mode: Mode,
    pending: Option<char>,
    /// How long `pending` has been waiting.
    pending_for: std::time::Duration,
    /// The `?` help is open, scrolled to this line.
    help: Option<usize>,
    dirty: bool,
    message: Option<String>,
    /// A standing caveat about the Range, shown until the Review closes.
    warning: Option<String>,
    /// The pull request behind the Range, when the Forge knows one.
    pull_request: Option<PullRequest>,
    /// The request for that pull request, while it is on its way.
    forge_load: forge_load::ForgeLoad,
    quit: bool,
    analysis: analysis::Analysis,
    /// The Analyst's answers kept on disk between runs.
    analyst_cache: crate::analyst_cache::AnalystCache,
    reload: reload::Reload,
    diagrams: diagram::Diagrams,
    groups: groups::Groups,
    explain: explain::Explain,
    /// Clipboard sequences waiting to be written to the terminal.
    terminal_out: Vec<String>,
    /// The Prompt overlay (`Space p s`), scrolled to this line.
    prompt_view: Option<(String, usize)>,
    /// The Prompts copied so far, which cut the commits into Rounds.
    prompts: crate::rounds::Prompts,
    mouse: mouse::Mouse,
    /// The file list scrolled by the wheel: the selected file it was done
    /// for, and the first row shown.
    sidebar_scroll: Option<(usize, usize)>,
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
        let explicit = match target {
            Target::Range { base, head } => Some(format!("{base}..{head}")),
            _ => None,
        };
        let reject_empty = *target != Target::Current;
        let mut session = Self::open_range(repo, range, reject_empty, explicit, width, height)?;
        session.reload.target = Some(target.clone());
        session.analysis.head_rev = match target {
            Target::Current => "HEAD".to_string(),
            Target::Branch(name) => format!("refs/heads/{name}"),
            Target::Range { head, .. } => head.clone(),
        };
        Ok(session)
    }

    /// Like [`Session::open_target`], and ask `forge` for the pull request of
    /// the branch. The Review opens at once; the answer is fetched in the
    /// background and the status line shows that it is awaited. If the Forge
    /// cannot answer, the reason is shown in the status line.
    pub fn open_target_with_forge(
        repo_path: &Path,
        target: &Target,
        forge: Arc<dyn Forge>,
        width: u16,
        height: u16,
    ) -> Result<Session, Error> {
        let mut session = Self::open_target(repo_path, target, width, height)?;
        let branch = match target {
            Target::Branch(name) => Some(name.clone()),
            Target::Current if session.range.head_name != "HEAD" => {
                Some(session.range.head_name.clone())
            }
            _ => None,
        };
        if let Some(branch) = branch {
            session.request_pull_request(forge, branch);
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
    /// of moonhare's own, with no new branch and no checkout. This waits for
    /// the Forge; [`crate::opening::Opening`] does the waiting in the background.
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
        Self::open_fetched_pull_request(repo_path, pr, &reference, width, height)
    }

    /// Open a pull request whose head the Forge has already fetched into
    /// `reference`.
    pub fn open_fetched_pull_request(
        repo_path: &Path,
        pr: PullRequest,
        reference: &str,
        width: u16,
        height: u16,
    ) -> Result<Session, Error> {
        let repo = Repo::open(repo_path)?;
        let range =
            repo.range_for_head(&format!("#{}", pr.number), reference, Some(&pr.base_branch))?;
        let mut session = Self::open_range(repo, range, true, None, width, height)?;
        session.pull_request = Some(pr);
        session.analysis.head_rev = reference.to_string();
        Ok(session)
    }

    fn open_range(
        repo: Repo,
        range: Range,
        reject_empty: bool,
        explicit_range: Option<String>,
        width: u16,
        height: u16,
    ) -> Result<Session, Error> {
        if reject_empty && repo.commit_count(&range.base, &range.head)? == 0 {
            return Err(Error(format!(
                "empty range: no commits between {} and {}",
                range.base_name, range.head_name
            )));
        }
        let commits = repo.commits(&range.base, &range.head)?;
        let files = repo.changed_files(&range.base, &range.head)?;
        let commit_changes = reload::changes_per_commit(&repo, &range, &commits)?;
        // A Review belongs to the branch name, or to the explicit range string.
        let review_name = match explicit_range {
            Some(text) => format!("range {text}"),
            None => format!("branch {}", range.head_name),
        };
        let review = Review::open(&repo, &review_name)?;
        let merges = repo.merge_commit_count(&range.base, &range.head)?;
        let warning = (merges > 0).then(|| {
            format!("⚠ {merges} merge commit(s) in range, compared against the first parent")
        });
        let review_dir = review.dir().to_path_buf();
        let mut session = Session {
            repo,
            range,
            commits,
            list: Default::default(),
            scope: Scope::All,
            focus: Focus::Diff,
            commit_cursor: 0,
            commit_anchor: None,
            picker: false,
            range_files: files.clone(),
            commit_changes,
            review,
            comments: crate::comments::Comments::open(&review_dir),
            editor: None,
            visual: None,
            confirm_clear: false,
            hidden_cache: Default::default(),
            file_picker: None,
            mechanical_open: false,
            scope_files: files.clone(),
            unreviewed_only: false,
            files,
            selected: 0,
            loaded: None,
            vp: Viewport::default(),
            hscroll: 0,
            count: None,
            view: Folded::default(),
            view_layout: DiffLayout::SideBySide,
            inline: Vec::new(),
            size: (width, height),
            layout_override: None,
            sidebar_manual: None,
            sidebar_custom: None,
            truecolor: false,
            mode: Mode::Normal,
            pending: None,
            pending_for: std::time::Duration::ZERO,
            help: None,
            dirty: true,
            message: None,
            warning,
            pull_request: None,
            forge_load: Default::default(),
            quit: false,
            analysis: Default::default(),
            analyst_cache: crate::analyst_cache::AnalystCache::new(&review_dir),
            reload: Default::default(),
            diagrams: Default::default(),
            groups: Default::default(),
            explain: Default::default(),
            terminal_out: Vec::new(),
            prompt_view: None,
            prompts: crate::rounds::Prompts::open(&review_dir),
            mouse: mouse::Mouse::default(),
            sidebar_scroll: None,
        };
        session.load_selected();
        session.sync_view();
        Ok(session)
    }

    /// Sequences the terminal must be sent (clipboard copies), oldest first.
    pub fn take_terminal_output(&mut self) -> Vec<String> {
        std::mem::take(&mut self.terminal_out)
    }

    /// Time passing without input; opens the which-key window once a key
    /// prefix has been waiting long enough.
    /// Returns whether what is drawn may have changed.
    pub fn elapse(&mut self, elapsed: std::time::Duration) -> bool {
        self.mouse_time(elapsed);
        let mut changed = false;
        if self.pending.is_some() {
            let before = self.pending_for;
            self.pending_for += elapsed;
            let delay = crate::hints::WHICH_KEY_DELAY;
            changed = before < delay && self.pending_for >= delay;
        }
        self.poll_reload(elapsed) || changed
    }

    /// Time passing, for the watch on the branch only. The real loop calls it
    /// also between input events, so that typing never starves the watch.
    pub fn watch(&mut self, elapsed: std::time::Duration) -> bool {
        self.mouse_time(elapsed);
        self.poll_reload(elapsed)
    }

    /// Draw with truecolor (exact RGB) instead of the 256-colour palette.
    pub fn set_truecolor(&mut self, on: bool) {
        self.truecolor = on;
    }

    /// How long until a [`elapse`](Self::elapse) changes what is drawn, if any is
    /// due: the real loop sleeps this long instead of redrawing idly.
    pub fn next_tick_in(&self) -> Option<std::time::Duration> {
        let waiting = self.pending.is_some() && self.help.is_none();
        waiting
            .then(|| crate::hints::WHICH_KEY_DELAY.checked_sub(self.pending_for))?
            .filter(|left| !left.is_zero())
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

    /// Whether the sidebar is shown: the manual choice, else by width.
    pub fn sidebar_visible(&self) -> bool {
        self.sidebar_manual
            .unwrap_or(self.size.0 >= SIDEBAR_AUTO_MIN_WIDTH)
    }

    fn sidebar_width(&self) -> u16 {
        if self.sidebar_visible() {
            self.sidebar_custom
                .map_or(SIDEBAR_WIDTH, |w| self.custom_sidebar_width(w))
                .min(self.size.0)
        } else {
            0
        }
    }

    fn too_small(&self) -> bool {
        self.size.0 < MIN_SIZE.0 || self.size.1 < MIN_SIZE.1
    }

    /// The whole left column: commit list above, file list below.
    fn sidebar_area(&self) -> Rect {
        Rect::new(
            0,
            0,
            self.sidebar_width(),
            self.size.1.saturating_sub(BOTTOM_ROWS),
        )
    }

    /// The diff pane including its header line.
    fn diff_area(&self) -> Rect {
        let x = self.sidebar_width();
        Rect::new(
            x,
            0,
            self.size.0 - x,
            self.size.1.saturating_sub(BOTTOM_ROWS),
        )
    }

    /// Rows available to diff lines (the pane minus its header).
    fn viewport_height(&self) -> usize {
        self.diff_area().height.saturating_sub(1) as usize
    }

    /// The commit list: about a third of the column, but enough rows for a
    /// short Range.
    fn commits_area(&self) -> Rect {
        let side = self.sidebar_area();
        let cap = (side.height * 35 / 100)
            .max(11)
            .min(side.height.saturating_sub(3));
        let frame = self
            .range_frame(side.width.saturating_sub(2) as usize)
            .len() as u16;
        let wanted = self.list_rows().len() as u16 + 3 + frame.min(cap.saturating_sub(4));
        Rect::new(side.x, side.y, side.width, wanted.min(cap))
    }

    fn files_area(&self) -> Rect {
        let side = self.sidebar_area();
        let top = self.commits_area().height;
        Rect::new(side.x, side.y + top, side.width, side.height - top)
    }

    /// Rows available to the file list (the pane minus its border and the
    /// two progress rows).
    fn sidebar_capacity(&self) -> usize {
        self.files_area()
            .height
            .saturating_sub(2 + marks::PROGRESS_ROWS) as usize
    }

    /// The first sidebar row shown: scrolled so the selected file is visible.
    fn sidebar_offset(&self) -> usize {
        let cap = self.sidebar_capacity().max(1);
        // The wheel scrolled the list away from the selected file.
        if let Some((selected, offset)) = self.sidebar_scroll {
            if selected == self.selected {
                return offset.min(self.sidebar_rows().len().saturating_sub(cap));
            }
        }
        let at = self
            .sidebar_rows()
            .iter()
            .position(|r| *r == groups::Row::File(self.selected))
            .unwrap_or(self.selected);
        (at + 1).saturating_sub(cap)
    }

    // ------------------------------------------------------------ data

    /// The two revisions whose difference the current Scope displays.
    fn scope_ends(&self) -> (String, String) {
        self.ends_of(self.scope)
    }

    fn ends_of(&self, scope: Scope) -> (String, String) {
        match scope {
            Scope::All => (self.range.base.clone(), self.range.head.clone()),
            Scope::Commits { lo, hi } => (self.parent_of(lo), self.commits[hi].sha.clone()),
        }
    }

    /// The revision just before commit `index`.
    fn parent_of(&self, index: usize) -> String {
        match index.checked_sub(1) {
            Some(prev) => self.commits[prev].sha.clone(),
            None => self.range.base.clone(),
        }
    }

    fn load_selected(&mut self) {
        let (scope_base, scope_head) = self.scope_ends();
        self.vp = Viewport::default();
        self.hscroll = 0;
        self.view = Folded::default();
        self.dirty = true;
        self.mechanical_open = false;
        self.loaded = self.files.get(self.selected).map(|f| {
            let blob = |rev: &str, path: &str| {
                self.repo
                    .file_at(rev, path)
                    .map(Some)
                    .map_err(|e| e.to_string())
            };
            let old = match self.delta_baseline(&f.path) {
                Some(reviewed) => reviewed,
                None => match f.status {
                    Status::Added => Ok(None),
                    _ => blob(&scope_base, f.base_path()),
                },
            };
            let new = match f.status {
                Status::Deleted => Ok(None),
                _ => blob(&scope_head, &f.path),
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

    /// Make `scope` the displayed Scope and reload the file list and diff.
    fn apply_scope(&mut self, scope: Scope) {
        let (base, head) = self.ends_of(scope);
        // On a git error keep the Scope and the screen as they are.
        let files = match self.repo.changed_files(&base, &head) {
            Ok(files) => files,
            Err(e) => {
                self.message = Some(format!("Cannot apply the Scope: {e}"));
                return;
            }
        };
        self.scope = scope;
        let previous = self.files.get(self.selected).map(|f| f.path.clone());
        self.scope_files = files;
        self.files = self.visible_files();
        self.selected = previous
            .and_then(|p| self.files.iter().position(|f| f.path == p))
            .unwrap_or(0);
        self.load_selected();
    }

    /// Apply the Scope the commit-list cursor stands on.
    fn apply_cursor_scope(&mut self) {
        let scope = match (self.commit_cursor, self.commit_anchor.take()) {
            (0, _) => Scope::All,
            (n, None) => Scope::Commits {
                lo: n - 1,
                hi: n - 1,
            },
            (n, Some(a)) => Scope::Commits {
                lo: n.min(a) - 1,
                hi: n.max(a) - 1,
            },
        };
        self.apply_scope(scope);
    }

    fn select(&mut self, index: usize) {
        if index < self.files.len() && index != self.selected {
            self.selected = index;
            self.load_selected();
        }
    }

    fn compute_inline(&self) -> Vec<InlineLine> {
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

    /// Old and new line numbers (0-based) of an entry of the built layout.
    fn entry_lines(&self, entry: usize) -> (Option<usize>, Option<usize>) {
        match (&self.loaded, self.view_layout) {
            (Some(Ok(l)), DiffLayout::SideBySide) => {
                let r = l.diff.rows[entry];
                (r.old, r.new)
            }
            (Some(Ok(_)), DiffLayout::Inline) => {
                let r = self.inline[entry];
                (r.old, r.new)
            }
            _ => (None, None),
        }
    }

    /// The source lines an item stands for (the first one of a fold).
    fn item_lines(&self, item: Item) -> (Option<usize>, Option<usize>) {
        match item {
            Item::Line(e) | Item::Fold { from: e, .. } => self.entry_lines(e),
        }
    }

    /// Rebuild the folded display model when the file or the layout changed,
    /// keeping the cursor on the same source line.
    fn sync_view(&mut self) {
        let layout = self.layout();
        if !self.dirty && self.view_layout == layout {
            return;
        }
        let anchor = (!self.dirty)
            .then(|| self.view.items.get(self.vp.cursor).copied())
            .flatten()
            .map(|item| self.item_lines(item));
        self.dirty = false;
        // View indexes mean something else in a new file, Scope or layout.
        self.visual = None;
        self.drop_mouse_selection();
        self.view_layout = layout;
        self.inline = Vec::new();
        let mut changed = Vec::new();
        if let Some(Ok(l)) = &self.loaded {
            match layout {
                DiffLayout::SideBySide => {
                    for row in &l.diff.rows {
                        let flag = |side, line: Option<usize>, other| {
                            line.is_some_and(|x| l.line_changed(side, x, other))
                        };
                        changed.push(
                            flag(Side::Old, row.old, row.new) || flag(Side::New, row.new, row.old),
                        );
                    }
                }
                DiffLayout::Inline => {
                    self.inline = self.compute_inline();
                    changed = self
                        .inline
                        .iter()
                        .map(|l| l.kind != InlineKind::Context)
                        .collect();
                }
            }
        }
        self.view = if self.collapsed() {
            Folded::default()
        } else {
            fold(&changed)
        };
        if let Some((old, new)) = anchor {
            let found = self.view.items.iter().position(|&item| {
                let (o, n) = self.item_lines(item);
                (new.is_some() && n == new) || (new.is_none() && o == old)
            });
            if let Some(i) = found {
                self.vp.cursor = i;
            }
        }
        self.clamp_view();
    }

    fn line_count(&self) -> usize {
        self.view.items.len()
    }

    fn clamp_view(&mut self) {
        self.vp.clamp(self.line_count(), self.viewport_height());
    }

    // ------------------------------------------------------------ events

    pub fn handle(&mut self, event: Event) {
        let waiting = self.pending;
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            Event::Mouse(mouse) => self.on_mouse(mouse),
            Event::Resize(w, h) => {
                self.size = (w, h);
                self.drop_mouse_selection();
            }
            _ => {}
        }
        if self.pending != waiting || self.pending.is_none() {
            self.pending_for = std::time::Duration::ZERO;
        }
        if !self.overlay_open() {
            self.mouse.text = None;
        }
        self.sync_view();
        self.reveal_with_comments();
    }

    fn on_key(&mut self, key: KeyEvent) {
        if matches!(self.mode, Mode::Search(_)) {
            self.search_key(key);
            return;
        }
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
        if self.editor.is_some() {
            self.editor_key(key);
            return;
        }
        if self.confirm_clear {
            self.confirm_clear_key(key);
            return;
        }
        if self.help.is_some() {
            self.help_key(key.code);
            return;
        }
        if self.diagram_key(key)
            || self.explain_key(key)
            || self.analysis_key(key)
            || self.groups_key(key)
            || self.prompt_key(key)
        {
            return;
        }
        if let Some(cursor) = self.file_picker {
            self.file_picker_key(key, cursor);
            return;
        }
        if self.picker {
            self.picker_key(key);
            return;
        }
        if key.code == KeyCode::Char('?') && self.pending.is_none() {
            self.help = Some(0);
            return;
        }
        if self.window_key(key.code, key.modifiers) {
            return;
        }
        if self.focus == Focus::Commits
            && self.sidebar_visible()
            && (self.list_key(key) || self.commits_key(key))
        {
            return;
        }
        if self.focus == Focus::Files && self.sidebar_visible() && self.files_key(key) {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if !ctrl {
            match (self.pending, key.code) {
                (Some(prefix @ (' ' | ']' | '[')), KeyCode::Char('c')) => {
                    self.pending = None;
                    self.count = None;
                    match prefix {
                        ' ' => self.picker = true,
                        ']' => self.step_commit(true),
                        _ => self.step_commit(false),
                    }
                    return;
                }
                (Some(' '), KeyCode::Char('e')) => {
                    self.pending = None;
                    self.count = None;
                    self.sidebar_manual = Some(!self.sidebar_visible());
                    return;
                }
                (Some(' '), KeyCode::Char('u')) => {
                    self.pending = None;
                    self.count = None;
                    self.toggle_unreviewed();
                    return;
                }
                (Some(' '), KeyCode::Char('f')) => {
                    self.pending = None;
                    self.count = None;
                    self.open_file_picker();
                    return;
                }
                (Some(' '), KeyCode::Char('y')) => {
                    self.pending = None;
                    self.count = None;
                    self.yank(true);
                    return;
                }
                (None, KeyCode::Char(' ')) => {
                    self.pending = Some(' ');
                    return;
                }
                _ => {}
            }
        }
        if self.nav_key(key.code, ctrl) {
            return;
        }

        match key.code {
            KeyCode::Char('s') if !ctrl => {
                // The manual choice wins over the width-based default from now on.
                self.layout_override = Some(match self.layout() {
                    DiffLayout::SideBySide => DiffLayout::Inline,
                    DiffLayout::Inline => DiffLayout::SideBySide,
                });
            }
            KeyCode::Char('a') if !ctrl => self.show_whole_range(),
            KeyCode::Char('Y') if !ctrl => self.copy_prompt(),
            KeyCode::Char('y') if !ctrl => self.yank(false),
            KeyCode::Char('m') if !ctrl => self.toggle_mark(),
            KeyCode::Char('M') if !ctrl => self.mark_and_advance(),
            KeyCode::Char('R') if !ctrl => self.reload(),
            KeyCode::Char(c @ ('c' | 'C' | 'v' | 'x' | 'o')) if !ctrl => self.comment_key(c),
            KeyCode::Esc if self.visual.is_some() => self.visual = None,
            KeyCode::Esc if self.clear_mouse_selection() => {}
            KeyCode::Enter if !ctrl => self.expand_mechanical(),
            KeyCode::Char(':') if !ctrl => self.mode = Mode::Command(String::new()),
            _ => {}
        }
    }

    /// Keys of the open commit picker: the commit list's keys, with `Enter` and
    /// `a` applying a Scope and closing, and `Esc` closing.
    fn picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc if self.commit_anchor.is_some() => self.commit_anchor = None,
            KeyCode::Esc => self.picker = false,
            KeyCode::Enter => {
                self.apply_cursor_scope();
                self.picker = false;
            }
            KeyCode::Char('a') => {
                self.show_whole_range();
                self.picker = false;
            }
            _ => {
                if !self.list_key(key) {
                    self.commits_key(key);
                }
            }
        }
    }

    /// Keys of the focused commit list; returns whether the key was used.
    fn commits_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) || self.pending.is_some() {
            return false;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.step_list(1),
            KeyCode::Char('k') | KeyCode::Up => self.step_list(-1),
            KeyCode::Char('v') => {
                self.commit_anchor = match self.commit_anchor {
                    Some(_) => None,
                    None if self.commit_cursor > 0 => Some(self.commit_cursor),
                    None => None,
                };
            }
            KeyCode::Esc if self.commit_anchor.is_some() => self.commit_anchor = None,
            KeyCode::Enter => self.apply_cursor_scope(),
            _ => return false,
        }
        true
    }

    /// `all`, `3` or `3-5`, as `:scope` takes them.
    fn scope_label(&self) -> String {
        match self.scope {
            Scope::All => "all".to_string(),
            Scope::Commits { lo, hi } if lo == hi => (lo + 1).to_string(),
            Scope::Commits { lo, hi } => format!("{}-{}", lo + 1, hi + 1),
        }
    }

    fn show_whole_range(&mut self) {
        self.commit_cursor = 0;
        self.commit_anchor = None;
        self.apply_scope(Scope::All);
    }

    /// `]c` / `[c`: the single commit after / before the current Scope, applied
    /// at once. From the whole Range they land on the first / last commit.
    fn step_commit(&mut self, forward: bool) {
        let last = self.commits.len();
        let target = match (self.scope, forward) {
            _ if last == 0 => return,
            (Scope::All, true) => 1,
            (Scope::All, false) => last,
            (Scope::Commits { hi, .. }, true) => (hi + 2).min(last),
            (Scope::Commits { lo, .. }, false) => lo.max(1),
        };
        self.commit_cursor = target;
        self.commit_anchor = None;
        self.reveal_commit(target - 1);
        self.apply_scope(Scope::Commits {
            lo: target - 1,
            hi: target - 1,
        });
    }

    /// `:scope all`, `:scope N` or `:scope N-M` with 1-based commit numbers.
    fn scope_command(&mut self, arg: &str) -> Result<(), String> {
        let number = |t: &str| t.trim().parse::<usize>().ok().filter(|&n| n >= 1);
        let scope = if arg == "all" {
            Scope::All
        } else {
            let (lo, hi) = match arg.split_once('-') {
                Some((a, b)) => (number(a), number(b)),
                None => (number(arg), number(arg)),
            };
            let (Some(lo), Some(hi)) = (lo, hi) else {
                return Err(format!("Usage: :scope all | N | N-M (got \"{arg}\")"));
            };
            if lo > hi || hi > self.commits.len() {
                return Err(format!(
                    "No such commits: {arg} (the Range has {})",
                    self.commits.len()
                ));
            }
            Scope::Commits {
                lo: lo - 1,
                hi: hi - 1,
            }
        };
        self.apply_scope(scope);
        self.commit_cursor = match scope {
            Scope::All => 0,
            Scope::Commits { lo, .. } => lo + 1,
        };
        if let Scope::Commits { lo, .. } = scope {
            self.reveal_commit(lo);
        }
        self.commit_anchor = None;
        Ok(())
    }

    fn run_command(&mut self, command: &str) {
        match command {
            "q" | "q!" | "quit" => self.quit = true,
            "" => {}
            "reset" => self.reset_review(),
            "reload" => self.reload(),
            "comment" => self.comment_on_range(),
            other if other.split_whitespace().next() == Some("sidebar") => {
                self.sidebar_command(other["sidebar".len()..].trim())
            }
            "comments clear" => {
                self.confirm_clear = true;
                self.message = Some("Delete all Comments for good? y / n".into());
            }
            other if other.split_whitespace().next() == Some("set") => {
                self.set_command(other["set".len()..].trim())
            }
            other if self.analysis_command(other) || self.diagram_command(other) => {}
            other if self.prompt_command(other) => {}
            other if other.split_whitespace().next() == Some("scope") => {
                let arg = other["scope".len()..].trim();
                if let Err(e) = self.scope_command(arg) {
                    self.message = Some(e);
                }
            }
            other => self.message = Some(format!("Not an editor command: {other}")),
        }
    }

    fn on_mouse(&mut self, mouse: MouseEvent) {
        // Nothing is drawn on a terminal this small: there is nothing to hit.
        if !self.mouse_enabled() || self.too_small() {
            return;
        }
        if self.editor.is_some() || self.confirm_clear {
            return;
        }
        if self.overlay_mouse(mouse) {
            return;
        }
        if self.file_picker_mouse(mouse)
            || self.picker_mouse(mouse)
            || self.sidebar_mouse(mouse)
            || self.diff_mouse(mouse)
            || self.list_mouse(mouse)
        {
            return;
        }
        let at = |area: Rect| area.contains((mouse.column, mouse.row).into());
        match mouse.kind {
            MouseEventKind::Down(_) if self.on_range_chip(mouse.column, mouse.row) => {
                self.picker = true;
            }
            MouseEventKind::ScrollDown if at(self.diff_area()) => {
                self.scroll_view(WHEEL_STEP as isize)
            }
            MouseEventKind::ScrollUp if at(self.diff_area()) => {
                self.scroll_view(-(WHEEL_STEP as isize))
            }
            MouseEventKind::Down(_) if at(self.commits_area()) && !self.picker => {
                self.click_commits(mouse.row);
            }
            MouseEventKind::Down(_) if at(self.files_area()) && !self.picker => {
                self.focus = Focus::Files;
                let area = self.files_area();
                // Row 0 of the sidebar is its border.
                if mouse.row > area.y {
                    let at = self.sidebar_offset() + (mouse.row - area.y - 1) as usize;
                    if ((mouse.row - area.y - 1) as usize) < self.sidebar_capacity() {
                        if let Some(groups::Row::File(index)) = self.sidebar_rows().get(at) {
                            self.select(*index);
                        }
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
        self.render_screen(buf);
        if self.truecolor {
            crate::color::to_truecolor(buf);
        }
    }

    fn render_screen(&self, buf: &mut Buffer) {
        self.render_base(buf);
        self.render_overlay_selection(buf);
    }

    /// Everything but the highlight of text selected in an overlay.
    fn render_base(&self, buf: &mut Buffer) {
        buf.set_style(buf.area, Style::new().fg(FG));
        if self.too_small() {
            let msg = format!(
                "Terminal too small: need at least {}x{}",
                MIN_SIZE.0, MIN_SIZE.1
            );
            buf.set_stringn(0, 0, &msg, self.size.0 as usize, Style::new().fg(FG));
            return;
        }
        if self.sidebar_visible() {
            self.render_sidebar(buf);
        }
        self.render_diff(buf);
        self.render_status(buf);
        self.render_hints(buf);
        if self.picker {
            self.render_picker(buf);
        }
        self.render_summary(buf);
        self.render_diagram_overlay(buf);
        self.render_explain(buf);
        if let Some(cursor) = self.file_picker {
            self.render_file_picker(buf, cursor);
        }
        if let Some(offset) = self.help {
            self.render_help(buf, offset);
        }
        if let Some(prefix) = self.which_key_prefix() {
            self.render_which_key(buf, prefix);
        }
        self.render_editor_popup(buf);
        self.render_prompt_view(buf);
    }

    fn render_sidebar(&self, buf: &mut Buffer) {
        self.render_commits(buf);
        self.render_files(buf);
    }

    fn render_commits(&self, buf: &mut Buffer) {
        let area = self.commits_area();
        draw_box_focused(
            buf,
            area,
            &self.commits_title(),
            self.window() == Focus::Commits,
        );
        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        self.render_commit_rows(buf, inner, self.focus == Focus::Commits);
    }

    /// " Commits 4/5 " (the commit under the cursor) or " Commits 5 " while
    /// the cursor is on the "Весь Range" row.
    fn commits_title(&self) -> String {
        match self.commit_cursor {
            0 => format!(" Commits {} ", self.commits.len()),
            n => format!(" Commits {n}/{} ", self.commits.len()),
        }
    }

    /// Whether the commit list marks commit `index` as inside the Scope.
    fn in_scope(&self, index: usize) -> bool {
        matches!(self.scope, Scope::Commits { lo, hi } if (lo..=hi).contains(&index))
    }

    /// The commit list inside `area`: the "Весь Range" row stays pinned at the
    /// top while the commits scroll to keep the cursor visible.
    fn render_commit_rows(&self, buf: &mut Buffer, area: Rect, show_cursor: bool) {
        let width = area.width as usize;
        let frame = self.range_frame_in(area.width, area.height);
        let capacity = self.list_capacity(area.width, area.height);
        let shown = self.shown_rows(capacity);
        let selection = self
            .commit_anchor
            .map(|a| (a.min(self.commit_cursor), a.max(self.commit_cursor)));
        let rows = std::iter::once((0, "▣ Весь Range".to_string(), self.scope == Scope::All))
            .chain(
                shown
                    .pinned
                    .map(|span| (HEADER, format!("── {}", span.title), false)),
            )
            .chain(shown.rows.into_iter().map(|row| match row {
                rounds::ListRow::Header(title) => (HEADER, format!("── {title}"), false),
                rounds::ListRow::Folded { title, start, end } => {
                    let position = match self.commit_cursor {
                        n if (start + 1..=end).contains(&n) => n,
                        _ => start + 1,
                    };
                    let in_scope = (start..end).any(|i| self.in_scope(i));
                    (position, format!("▸ {title}"), in_scope)
                }
                rounds::ListRow::Commit(i) => {
                    let c = &self.commits[i];
                    let sha = &c.sha[..7.min(c.sha.len())];
                    (
                        i + 1,
                        format!("{:>2} {sha} {}", i + 1, c.subject),
                        self.in_scope(i),
                    )
                }
            }));
        for (row, (position, text, in_scope)) in rows.enumerate() {
            let cursor = show_cursor && position == self.commit_cursor;
            let mark = if in_scope { '●' } else { ' ' };
            let line = format!("{}{mark} {text}", if cursor { '>' } else { ' ' });
            let mut style = Style::new().fg(if position == HEADER { DIM } else { FG });
            if selection.is_some_and(|(lo, hi)| (lo..=hi).contains(&position)) {
                style = style.bg(Color::Indexed(24));
            }
            if cursor {
                style = style.bg(Color::Indexed(238)).add_modifier(Modifier::BOLD);
            }
            let y = area.y + row as u16 + if row > 0 { frame.len() as u16 } else { 0 };
            buf.set_style(Rect::new(area.x, y, area.width, 1), style);
            buf.set_stringn(
                area.x,
                y,
                pad(&fit(&line, width, false), width),
                width,
                style,
            );
            if row == 0 {
                for (k, f) in frame.iter().enumerate() {
                    let y = area.y + 1 + k as u16;
                    buf.set_stringn(area.x, y, fit(&f.text, width, false), width, f.style);
                }
            }
        }
    }

    /// The `Space c` picker: the commit list in a box over the screen.
    /// The box of the `Space c` picker.
    fn picker_area(&self) -> Rect {
        let width = self.size.0.saturating_sub(4).min(72);
        let frame = self.range_frame(width.saturating_sub(2) as usize).len() as u16;
        let height = (self.list_rows().len() as u16 + 4 + frame)
            .min(self.size.1.saturating_sub(BOTTOM_ROWS + 1));
        Rect::new(
            (self.size.0 - width) / 2,
            (self.size.1 - BOTTOM_ROWS - height) / 2,
            width,
            height,
        )
    }

    /// The commit rows inside the picker box.
    fn picker_inner(&self) -> Rect {
        let area = self.picker_area();
        Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(3),
        )
    }

    fn render_picker(&self, buf: &mut Buffer) {
        let area = self.picker_area();
        buf.set_style(area, Style::new().fg(FG).bg(Color::Reset));
        clear_area(buf, area);
        draw_box(buf, area, " Pick a Scope ");
        let inner = self.picker_inner();
        self.render_commit_rows(buf, inner, true);
        let hint = "j/k move  v select  Enter apply  a whole Range  Esc close";
        let hint_w = inner.width as usize;
        buf.set_stringn(
            inner.x,
            area.y + area.height - 2,
            fit(hint, hint_w, false),
            hint_w,
            Style::new().fg(DIM),
        );
    }

    fn render_files(&self, buf: &mut Buffer) {
        let area = self.files_area();
        draw_box_focused(
            buf,
            area,
            &format!(" Files ({}){} ", self.files.len(), self.groups_note()),
            self.window() == Focus::Files,
        );
        let width = area.width as usize;
        let inner = width.saturating_sub(2);
        let offset = self.sidebar_offset();
        self.render_progress(buf, area);
        for (row, shown) in self
            .sidebar_rows()
            .into_iter()
            .skip(offset)
            .take(self.sidebar_capacity())
            .enumerate()
        {
            let y = area.y + 1 + row as u16;
            let groups::Row::File(i) = shown else {
                self.render_group_row(buf, area.x + 1, y, inner, shown);
                continue;
            };
            let file = &self.files[i];
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
            let reviewed = match self.mark_of(i) {
                Mark::Reviewed => '✓',
                Mark::Partial => '◐',
                Mark::Unreviewed => ' ',
            };
            let path = fit(&file.path, inner.saturating_sub(4), true);
            let line = format!("{marker}{reviewed}{} {path}", file.status.letter());
            buf.set_stringn(area.x + 1, y, pad(&line, inner), inner, style);
            buf.set_style(Rect::new(area.x + 3, y, 1, 1), style.fg(color));
        }
    }

    fn render_diff(&self, buf: &mut Buffer) {
        let area = self.diff_area();
        let w = area.width as usize;
        let Some(file) = self.files.get(self.selected) else {
            let msg = if self.unreviewed_only && !self.scope_files.is_empty() {
                "Nothing left to review: every file in this Scope is reviewed".to_string()
            } else {
                format!(
                    "No changes between {} and {}",
                    self.range.base_name, self.range.head_name
                )
            };
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
        let header = self.file_bar(file, d);
        let header_style = Style::new()
            .fg(Color::Indexed(231))
            .bg(Color::Indexed(236))
            .add_modifier(Modifier::BOLD);
        buf.set_style(Rect::new(area.x, area.y, area.width, 1), header_style);
        buf.set_stringn(area.x, area.y, &header, w, header_style);

        let top = area.y + 1;
        let height = self.viewport_height();
        if self.collapsed() {
            self.render_collapsed(buf, file, area, top);
            return;
        }
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
        self.comment_layout()
            .draw_header(buf, area.x, top, top + height as u16);
        self.render_selection(buf, area, top, height);
        self.render_mouse_selection(buf, top, height);
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
        let layout = self.comment_layout();
        let bottom = top + height as u16;
        for (screen_row, item) in self
            .view
            .items
            .iter()
            .skip(self.vp.top)
            .take(height)
            .enumerate()
        {
            let y = layout.y(top, self.vp.top, self.vp.top + screen_row);
            if y >= bottom {
                break;
            }
            let is_cursor = self.vp.top + screen_row == self.vp.cursor;
            let row = match *item {
                Item::Line(e) => l.diff.rows[e],
                Item::Fold { len, .. } => {
                    self.render_fold(buf, area.x, y, area.width, len, is_cursor);
                    continue;
                }
            };
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
            if is_cursor {
                let marker = Style::new().fg(ACCENT).add_modifier(Modifier::BOLD);
                let gutter = (digits + 1) as u16;
                buf.set_style(Rect::new(area.x, y, gutter, 1), marker);
                buf.set_style(Rect::new(right_x, y, gutter, 1), marker);
            }
            layout.draw(buf, area.x, y + 1, bottom, self.vp.top + screen_row);
        }
    }

    /// The placeholder row of a folded stretch of unchanged lines.
    fn render_fold(&self, buf: &mut Buffer, x: u16, y: u16, width: u16, len: usize, cursor: bool) {
        let mut style = Style::new().fg(DIM).bg(Color::Indexed(235));
        if cursor {
            style = style.fg(ACCENT).add_modifier(Modifier::BOLD);
        }
        buf.set_style(Rect::new(x, y, width, 1), style);
        let text = format!("  ⋯ {len} unchanged lines");
        buf.set_stringn(x, y, &text, width as usize, style);
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
        draw_cells(buf, code_x, y, code_w, shifted(&cells, self.hscroll));
    }

    fn render_inline(&self, buf: &mut Buffer, l: &Loaded, area: Rect, top: u16, height: usize) {
        let digits = digits(l.diff.old_lines.len().max(l.diff.new_lines.len()));
        let width = area.width as usize;
        let layout = self.comment_layout();
        let bottom = top + height as u16;
        for (screen_row, item) in self
            .view
            .items
            .iter()
            .skip(self.vp.top)
            .take(height)
            .enumerate()
        {
            let y = layout.y(top, self.vp.top, self.vp.top + screen_row);
            if y >= bottom {
                break;
            }
            let is_cursor = self.vp.top + screen_row == self.vp.cursor;
            let line = match *item {
                Item::Line(e) => &self.inline[e],
                Item::Fold { len, .. } => {
                    self.render_fold(buf, area.x, y, area.width, len, is_cursor);
                    continue;
                }
            };
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
            let code_w = width.saturating_sub(gw);
            draw_cells(
                buf,
                area.x + gw as u16,
                y,
                code_w,
                shifted(&cells, self.hscroll),
            );
            if is_cursor {
                let marker = Style::new().fg(ACCENT).add_modifier(Modifier::BOLD);
                buf.set_style(Rect::new(area.x, y, gw as u16, 1), marker);
            }
            layout.draw(buf, area.x, y + 1, bottom, self.vp.top + screen_row);
        }
    }

    fn render_status(&self, buf: &mut Buffer) {
        let y = self.size.1 - BOTTOM_ROWS;
        let width = self.size.0 as usize;
        let bar = Style::new().fg(FG).bg(Color::Indexed(236));
        buf.set_style(Rect::new(0, y, self.size.0, 1), bar);
        let badge = Style::new()
            .fg(Color::Indexed(16))
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD);
        buf.set_string(0, y, self.mode_badge(), badge);

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
                " {}..{}  scope:{}  round:{}  layout:{layout} ({how}){}",
                self.range.base_name,
                self.range.head_name,
                self.scope_label(),
                self.current_round(),
                self.settings_status(),
            ),
        };
        let text = self.groups_status(&text);
        let text = self.analysis_status(&text);
        let text = self.reload_status(&text);
        let text = self.explain_status(&text);
        let text = self.busy_status(&text);
        let text = self.forge_status(&text);
        let text = match &self.warning {
            Some(w) => format!("{text}  {w}"),
            None => text,
        };
        let text = self.with_hidden_hint(text);
        let text = match self.unreviewed_only {
            true => format!(" [only unreviewed]{text}"),
            false => text,
        };
        buf.set_stringn(8, y, &text, width.saturating_sub(8), bar);
        self.draw_range_chip(buf, y);
    }
}

/// A bordered box with a title in the top border.
fn draw_box(buf: &mut Buffer, area: Rect, title: &str) {
    draw_box_focused(buf, area, title, false);
}

/// A box whose border is lit while its window has the focus.
fn draw_box_focused(buf: &mut Buffer, area: Rect, title: &str, focused: bool) {
    let border = Style::new().fg(if focused { ACCENT } else { DIM });
    let width = area.width as usize;
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
            // A wide character takes its cell and one more, which is only
            // a placeholder; a zero-width one has no cell.
            match copy::cell_width(ch) {
                0 => {}
                1 => cells.push((ch, style)),
                n => {
                    cells.push((ch, style));
                    cells.extend(std::iter::repeat_n((copy::CONTINUATION, style), n - 1));
                }
            }
        }
    }
    cells
}

/// The cells from column `offset` on.
fn shifted(cells: &[(char, Style)], offset: usize) -> &[(char, Style)] {
    &cells[offset.min(cells.len())..]
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
        if *ch == copy::CONTINUATION {
            continue;
        }
        buf.set_string(x + i as u16, y, ch.to_string(), *style);
    }
    if cut && width > 0 {
        let style = cells[shown].1.fg(DIM);
        buf.set_string(x + shown as u16, y, "…", style);
    }
}

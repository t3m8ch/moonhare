// PROTOTYPE 4 shared state and key handling. One adaptive layout (ui.rs) draws this
// state; mouse handling lives in mouse.rs.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::data::*;

pub const MIN_W: u16 = 60;
pub const MIN_H: u16 = 15;
/// Terminals at least this wide show the sidebar by default (a manual toggle wins).
pub const SIDEBAR_AUTO_W: u16 = 100;
pub const WHICHKEY_DELAY: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Visual,
    Command,
    Search,
    Comment,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Commits,
    Files,
    Diff,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DiffLayout {
    Auto,
    Side,
    Inline,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    Help,
    Summary,
    Diagram,
    Prompt,
    Commits,
    Files,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Done,
    Partial,
    Todo,
}

/// A pending key prefix; the which-key popup lists what can follow it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Chord {
    G,
    Next,
    Prev,
    CtrlW,
    Leader,
    /// <Space>p ...: the Prompt family
    LeaderPrompt,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListItem {
    Group(usize),
    /// position in visible_files()
    File(usize),
}

pub struct Editor {
    pub text: String,
    pub insert: bool,
    pub kind: CKind,
    pub file: usize,
    pub a: usize,
    pub b: usize,
    /// "Discard? y/n" is showing
    pub confirm: bool,
}

pub enum RV {
    Row(usize, Row),
    Elide(usize),
}

// ---- geometry recorded by draw() for the mouse ------------------------------------

/// One code cell run on a screen line (a column of the diff).
#[derive(Clone)]
pub struct Seg {
    /// 0 = left/old column, 1 = right/new column (side-by-side), 2 = inline
    pub col: u8,
    /// x relative to the diff's inner area
    pub x0: u16,
    pub w: u16,
    pub text: Vec<char>,
    /// horizontal scroll: chars hidden on the left
    pub skip: usize,
    /// inline only: ' ' context, '+' added, '-' removed (side-by-side segs are all ' ')
    pub mark: char,
}

#[derive(Clone)]
pub struct LineMeta {
    pub src: Option<usize>,
    pub sub: u8,
    pub segs: Vec<Seg>,
}

pub struct DiffGeo {
    pub inner: Rect,
    pub fi: usize,
    pub inline: bool,
    pub width: usize,
    /// visible screen lines only
    pub lines: Vec<LineMeta>,
}

pub struct OverlayGeo {
    pub inner: Rect,
    pub scroll: usize,
    /// plain text of every content line (not just visible ones)
    pub texts: Vec<String>,
}

/// Sentinel "commit index" of the pinned "▣ Весь Range" row in the commit list.
pub const RANGE_ROW: usize = usize::MAX;

#[derive(Default)]
pub struct Hits {
    pub panes: Vec<(Rect, Pane)>,
    /// commit index, or RANGE_ROW for the pinned "Весь Range" row
    pub commit_rows: Vec<(Rect, usize)>,
    /// the `Range ✎N` chip in the top strip (sidebar hidden): click opens the Commits picker
    pub range_chip: Option<Rect>,
    /// index into list_items()
    pub list_rows: Vec<(Rect, usize)>,
    pub diff: Option<DiffGeo>,
    pub overlay_box: Option<Rect>,
    pub overlay: Option<OverlayGeo>,
    /// Commits picker: commit index. Files picker: position in visible_files().
    pub picker_rows: Vec<(Rect, usize)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DPos {
    pub src: usize,
    pub sub: u8,
    pub ch: usize,
}

impl DPos {
    pub fn key(&self) -> (usize, u8, usize) {
        (self.src, self.sub, self.ch)
    }
}

/// A mouse text selection inside one column of the diff.
#[derive(Clone, Copy)]
pub struct DiffSel {
    pub a: DPos,
    pub b: DPos,
    pub col: u8,
}

impl DiffSel {
    pub fn norm(&self) -> (DPos, DPos) {
        if self.a.key() <= self.b.key() {
            (self.a, self.b)
        } else {
            (self.b, self.a)
        }
    }
}

pub type OSel = ((usize, usize), (usize, usize));

pub struct App {
    pub size: (u16, u16),
    pub mode: Mode,
    pub focus: Pane,
    pub files: Vec<File>,
    pub commits: Vec<Commit>,
    pub groups: Vec<Group>,
    pub comments: Vec<Comment>,
    pub reviewed: HashSet<(usize, usize)>,
    pub scope: (usize, usize),
    pub unreviewed_only: bool,
    pub dlayout: DiffLayout,
    pub last_inline: bool,
    pub sidebar_manual: Option<bool>,
    pub folded: HashSet<usize>,
    pub list_cursor: usize,
    pub commit_cursor: usize,
    /// The cursor sits on the pinned "▣ Весь Range" row above commit 1 (Q49).
    pub on_range: bool,
    pub commit_anchor: Option<usize>,
    pub file_cursor: usize,
    pub diff_cursor: usize,
    pub diff_anchor: Option<usize>,
    pub hscroll: usize,
    // scroll is anchored to a diff row, not a screen line, so it survives layout changes
    pub scroll_fi: Option<usize>,
    pub scroll_src: usize,
    pub diff_scroll: usize,
    /// Pending viewport scroll in screen lines (Ctrl-e/y/f/b); applied by the next draw.
    pub scroll_delta: isize,
    pub view_h: usize,
    pub overlay: Option<Overlay>,
    pub overlay_scroll: usize,
    pub overlay_cursor: usize,
    pub chord: Option<Chord>,
    pub chord_at: Instant,
    pub whichkey: bool,
    pub cmd: String,
    pub search_input: String,
    pub search: String,
    pub search_hits: Vec<usize>,
    /// the active search ran in the commit list: search_hits are commit indexes
    pub search_commits: bool,
    /// Vim count prefix being typed (`10j`, `12G`)
    pub count: Option<usize>,
    /// visible rows of the commit pane, set by the last draw (page size for Ctrl-d/u there)
    pub commit_h: usize,
    pub search_idx: usize,
    pub editor: Option<Editor>,
    pub toast: Option<String>,
    pub round: u8,
    /// per Round: the Prompt numbers that opened it ("after Prompt #1"); a Round that got no
    /// commits before the next Prompt is merged into the next one
    pub round_after: Vec<Vec<u32>>,
    pub prompt_no: u32,
    pub last_prompt: Option<String>,
    pub quit: bool,
    // mouse
    pub mouse_on: bool,
    pub mouse_dirty: bool,
    pub hits: Hits,
    pub dsel: Option<DiffSel>,
    pub osel: Option<OSel>,
    pub dragging: bool,
    pub drag_moved: bool,
    pub drag_overlay: bool,
    pub drag_start: Option<(DPos, u8)>,
    pub drag_start_o: Option<(usize, usize)>,
    pub multi_click: bool,
    pub last_click: Option<(Instant, u16, u16, u8)>,
    // clipboard
    pub osc_out: Vec<String>,
    pub clip_log: Vec<String>,
}

pub fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for ch in data.chunks(3) {
        let n = (ch[0] as u32) << 16 | (*ch.get(1).unwrap_or(&0) as u32) << 8 | *ch.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if ch.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if ch.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn line_no(f: &File, r: usize) -> usize {
    f.rows[r].new_no.max(f.rows[r].old_no)
}

impl App {
    pub fn new(size: (u16, u16)) -> App {
        let files = files();
        let reviewed: HashSet<(usize, usize)> = initial_reviewed(&files).into_iter().collect();
        let comments = initial_comments(&files);
        let n = commits().len();
        let mut app = App {
            size,
            mode: Mode::Normal,
            focus: Pane::Diff,
            files,
            commits: commits(),
            groups: groups(),
            comments,
            reviewed,
            scope: (0, n - 1),
            unreviewed_only: false,
            dlayout: DiffLayout::Auto,
            last_inline: false,
            sidebar_manual: None,
            folded: HashSet::new(),
            list_cursor: 0,
            commit_cursor: 0,
            on_range: true,
            commit_anchor: None,
            file_cursor: 1, // src/retry/backoff.rs
            diff_cursor: 0,
            diff_anchor: None,
            hscroll: 0,
            scroll_fi: None,
            scroll_src: 0,
            diff_scroll: 0,
            scroll_delta: 0,
            view_h: 20,
            overlay: None,
            overlay_scroll: 0,
            overlay_cursor: 0,
            chord: None,
            chord_at: Instant::now(),
            whichkey: false,
            cmd: String::new(),
            search_input: String::new(),
            search: String::new(),
            search_hits: Vec::new(),
            search_commits: false,
            count: None,
            commit_h: 8,
            search_idx: 0,
            editor: None,
            toast: None,
            round: 2,
            round_after: vec![vec![], vec![1]],
            prompt_no: 1,
            last_prompt: None,
            quit: false,
            mouse_on: true,
            mouse_dirty: false,
            hits: Hits::default(),
            dsel: None,
            osel: None,
            dragging: false,
            drag_moved: false,
            drag_overlay: false,
            drag_start: None,
            drag_start_o: None,
            multi_click: false,
            last_click: None,
            osc_out: Vec::new(),
            clip_log: Vec::new(),
        };
        app.reset_diff_cursor();
        app.sync_list();
        app
    }

    // ---------- layout state ----------

    pub fn sidebar_visible(&self) -> bool {
        self.sidebar_manual.unwrap_or(self.size.0 >= SIDEBAR_AUTO_W)
    }

    pub fn too_small(&self) -> bool {
        self.size.0 < MIN_W || self.size.1 < MIN_H
    }

    /// Resize keeps everything (cursor, Scope, filter, overlays, Comment draft). Only the
    /// mouse selection is dropped, because its screen geometry is gone.
    pub fn on_resize(&mut self, w: u16, h: u16) {
        self.size = (w, h);
        self.dsel = None;
        self.osel = None;
        self.dragging = false;
        self.drag_moved = false;
        if !self.sidebar_visible() && self.focus != Pane::Diff {
            self.focus = Pane::Diff;
        }
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        if self.chord.is_some() && !self.whichkey && now.saturating_duration_since(self.chord_at) >= WHICHKEY_DELAY {
            self.whichkey = true;
            return true;
        }
        false
    }

    pub fn state_line(&self) -> String {
        let fi = self.current_file();
        format!(
            "size={}x{} sidebar={} focus={} file={} cursor={} scroll_src={} hscroll={} scope={} unreviewed_only={} overlay={} editor_draft={:?} mode={} mouse={} comments={}",
            self.size.0,
            self.size.1,
            if self.sidebar_visible() { "shown" } else { "hidden" },
            match self.focus {
                Pane::Commits => "commits",
                Pane::Files => "files",
                Pane::Diff => "diff",
            },
            fi.map_or("-", |i| self.files[i].path),
            self.diff_cursor,
            self.scroll_src,
            self.hscroll,
            self.scope_label(),
            self.unreviewed_only,
            self.overlay.is_some(),
            self.editor.as_ref().map(|e| e.text.clone()),
            match self.mode {
                Mode::Normal => "NORMAL",
                Mode::Visual => "VISUAL",
                Mode::Command => "COMMAND",
                Mode::Search => "SEARCH",
                Mode::Comment => "COMMENT",
            },
            self.mouse_on,
            self.comments.len()
        )
    }

    // ---------- derived queries ----------

    pub fn in_scope(&self, c: usize) -> bool {
        c >= self.scope.0 && c <= self.scope.1
    }

    pub fn scope_is_whole(&self) -> bool {
        self.scope == (0, self.commits.len() - 1)
    }

    pub fn scope_label(&self) -> String {
        let (lo, hi) = self.scope;
        if self.scope_is_whole() {
            format!("whole Range ({} commits)", self.commits.len())
        } else if lo == hi {
            format!("c{} {}", lo + 1, self.commits[lo].sha)
        } else {
            format!("c{}..c{} ({} commits)", lo + 1, hi + 1, hi - lo + 1)
        }
    }

    pub fn atoms(&self, fi: usize) -> Vec<usize> {
        self.files[fi].commits.iter().copied().filter(|c| self.in_scope(*c)).collect()
    }

    pub fn file_status(&self, fi: usize) -> Status {
        let a = self.atoms(fi);
        if a.is_empty() {
            return Status::Todo;
        }
        let done = a.iter().filter(|c| self.reviewed.contains(&(fi, **c))).count();
        if done == a.len() {
            Status::Done
        } else if done > 0 {
            Status::Partial
        } else {
            Status::Todo
        }
    }

    fn file_visible(&self, fi: usize) -> bool {
        let a = self.atoms(fi);
        !a.is_empty() && (!self.unreviewed_only || a.iter().any(|c| !self.reviewed.contains(&(fi, *c))))
    }

    pub fn visible_files(&self) -> Vec<usize> {
        (0..self.files.len()).filter(|i| self.file_visible(*i)).collect()
    }

    pub fn current_file(&self) -> Option<usize> {
        self.visible_files().get(self.file_cursor).copied()
    }

    /// (reviewed files, files in scope) ignoring the unreviewed filter.
    pub fn progress(&self) -> (usize, usize) {
        let in_scope: Vec<usize> = (0..self.files.len()).filter(|i| !self.atoms(*i).is_empty()).collect();
        let done = in_scope.iter().filter(|i| self.file_status(**i) == Status::Done).count();
        (done, in_scope.len())
    }

    pub fn group_progress(&self, g: usize) -> (usize, usize) {
        let fs: Vec<usize> = (0..self.files.len()).filter(|i| self.files[*i].group == g && !self.atoms(*i).is_empty()).collect();
        let done = fs.iter().filter(|i| self.file_status(**i) == Status::Done).count();
        (done, fs.len())
    }

    pub fn comment_count(&self, fi: usize) -> usize {
        self.comments.iter().filter(|c| c.kind != CKind::Range && c.file == fi).count()
    }

    pub fn range_comment_count(&self) -> usize {
        self.comments.iter().filter(|c| c.kind == CKind::Range).count()
    }

    /// Comments that exist but are not on screen anywhere right now (Scope / filter hid them).
    pub fn hidden_comments(&self) -> usize {
        let vf = self.visible_files();
        self.comments
            .iter()
            .filter(|c| match c.kind {
                CKind::Range => false,
                CKind::File => !vf.contains(&c.file),
                CKind::Line => !vf.contains(&c.file) || !self.visible_src(c.file).contains(&c.b),
            })
            .count()
    }

    pub fn hidden_in_file(&self, fi: usize) -> usize {
        let vis = self.visible_src(fi);
        self.comments.iter().filter(|c| c.kind == CKind::Line && c.file == fi && !vis.contains(&c.b)).count()
    }

    /// The row as it should look for the current Scope and filter, or None if it does not exist there.
    fn effective(&self, fi: usize, r: &Row) -> Option<Row> {
        if r.kind == Kind::Ctx {
            return Some(r.clone());
        }
        let (lo, hi) = self.scope;
        let n = r.commit;
        let rev = |c: usize| self.unreviewed_only && self.reviewed.contains(&(fi, c));
        let ctx = |text: &str| {
            let mut x = r.clone();
            x.kind = Kind::Ctx;
            x.old = text.to_string();
            x.new = text.to_string();
            x.old_span = None;
            x.new_span = None;
            if x.old_no == 0 {
                x.old_no = x.new_no;
            }
            if x.new_no == 0 {
                x.new_no = x.old_no;
            }
            x
        };
        let add = |text: &str| {
            let mut x = r.clone();
            x.kind = Kind::Add;
            x.old = String::new();
            x.new = text.to_string();
            x.old_span = None;
            x.new_span = None;
            x
        };
        match r.kind {
            Kind::Ctx => Some(r.clone()),
            Kind::Add => {
                if n > hi {
                    None
                } else if n < lo || rev(n) {
                    Some(ctx(&r.new))
                } else {
                    Some(r.clone())
                }
            }
            Kind::Del => {
                if n > hi {
                    Some(ctx(&r.old))
                } else if n < lo || rev(n) {
                    None
                } else {
                    Some(r.clone())
                }
            }
            Kind::Chg => {
                let old_base = match r.born {
                    None => true,
                    Some(b) => b < lo || rev(b),
                };
                if n > hi {
                    if old_base {
                        Some(ctx(&r.old))
                    } else if r.born.map_or(false, |b| b <= hi) {
                        Some(add(&r.old))
                    } else {
                        None
                    }
                } else if n < lo {
                    Some(ctx(&r.new))
                } else if rev(n) {
                    Some(ctx(&r.new))
                } else if old_base {
                    Some(r.clone())
                } else {
                    Some(add(&r.new))
                }
            }
        }
    }

    pub fn rows_view(&self, fi: usize) -> Vec<RV> {
        let f = &self.files[fi];
        let eff: Vec<Option<Row>> = f.rows.iter().map(|r| self.effective(fi, r)).collect();
        let n = eff.len();
        let changed: Vec<bool> = eff.iter().map(|e| matches!(e, Some(r) if r.kind != Kind::Ctx)).collect();
        let mut show = vec![false; n];
        for i in 0..n {
            if changed[i] {
                let lo = i.saturating_sub(3);
                let hi = (i + 3).min(n - 1);
                for s in show.iter_mut().take(hi + 1).skip(lo) {
                    *s = true;
                }
            }
        }
        let mut out = Vec::new();
        let mut hidden = 0;
        for (i, e) in eff.into_iter().enumerate() {
            let Some(r) = e else { continue };
            if show[i] {
                if hidden > 0 {
                    out.push(RV::Elide(hidden));
                    hidden = 0;
                }
                out.push(RV::Row(i, r));
            } else {
                hidden += 1;
            }
        }
        if hidden > 0 {
            out.push(RV::Elide(hidden));
        }
        out
    }

    pub fn visible_src(&self, fi: usize) -> Vec<usize> {
        self.rows_view(fi)
            .into_iter()
            .filter_map(|v| match v {
                RV::Row(i, _) => Some(i),
                RV::Elide(_) => None,
            })
            .collect()
    }

    fn hunk_starts(&self, fi: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut prev_changed = false;
        for v in self.rows_view(fi) {
            match v {
                RV::Row(i, r) => {
                    let ch = r.kind != Kind::Ctx;
                    if ch && !prev_changed {
                        out.push(i);
                    }
                    prev_changed = ch;
                }
                RV::Elide(_) => prev_changed = false,
            }
        }
        out
    }

    fn first_changed(&self, fi: usize) -> usize {
        self.hunk_starts(fi).first().copied().or_else(|| self.visible_src(fi).first().copied()).unwrap_or(0)
    }

    pub fn commit_of_row(&self, fi: usize, ri: usize) -> usize {
        let c = self.files[fi].rows[ri].commit;
        if c == usize::MAX {
            self.scope.1
        } else {
            c
        }
    }

    // ---------- Groups / files list (sidebar tree) ----------

    pub fn list_items(&self) -> Vec<ListItem> {
        items_for(self, true)
    }

    /// Put the list cursor on the current file, unfolding its Group if needed.
    pub fn sync_list(&mut self) {
        let vf = self.visible_files();
        if vf.is_empty() {
            self.list_cursor = 0;
            return;
        }
        let pos = self.file_cursor.min(vf.len() - 1);
        let g = self.files[vf[pos]].group;
        self.folded.remove(&g);
        let items = self.list_items();
        self.list_cursor = items.iter().position(|i| *i == ListItem::File(pos)).unwrap_or(0);
    }

    pub fn list_move(&mut self, d: isize) {
        let items = self.list_items();
        if items.is_empty() {
            return;
        }
        let np = (self.list_cursor as isize + d).clamp(0, items.len() as isize - 1) as usize;
        self.list_goto(np);
    }

    pub fn list_goto(&mut self, idx: usize) {
        let items = self.list_items();
        if items.is_empty() {
            return;
        }
        let idx = idx.min(items.len() - 1);
        match items[idx] {
            ListItem::File(pos) => {
                self.select_file(pos);
                self.list_cursor = idx;
            }
            ListItem::Group(_) => self.list_cursor = idx,
        }
    }

    fn list_fold(&mut self) {
        let items = self.list_items();
        let Some(it) = items.get(self.list_cursor).copied() else { return };
        let vf = self.visible_files();
        let g = match it {
            ListItem::Group(g) => g,
            ListItem::File(pos) => self.files[vf[pos]].group,
        };
        self.folded.insert(g);
        let items = self.list_items();
        self.list_cursor = items.iter().position(|i| *i == ListItem::Group(g)).unwrap_or(0);
    }

    fn list_unfold_or_enter(&mut self) {
        let items = self.list_items();
        match items.get(self.list_cursor).copied() {
            Some(ListItem::Group(g)) => {
                if self.folded.remove(&g) {
                    // unfolded
                } else {
                    self.list_move(1);
                }
            }
            Some(ListItem::File(_)) => self.focus = Pane::Diff,
            None => {}
        }
    }

    pub fn list_toggle_fold(&mut self, g: usize) {
        if !self.folded.remove(&g) {
            self.folded.insert(g);
        }
    }

    // ---------- selection upkeep ----------

    pub fn reset_diff_cursor(&mut self) {
        self.diff_anchor = None;
        self.dsel = None;
        self.hscroll = 0;
        self.scroll_fi = None;
        self.diff_scroll = 0;
        self.diff_cursor = match self.current_file() {
            Some(fi) => self.first_changed(fi),
            None => 0,
        };
    }

    fn ensure_diff_cursor(&mut self) {
        if let Some(fi) = self.current_file() {
            let vis = self.visible_src(fi);
            if !vis.contains(&self.diff_cursor) {
                let near = vis.iter().copied().find(|v| *v >= self.diff_cursor).or(vis.last().copied());
                self.diff_cursor = near.unwrap_or(0);
            }
        }
    }

    /// Re-establish the file cursor after anything that changes which files are visible.
    pub fn refresh(&mut self, keep: Option<usize>) {
        self.dsel = None;
        let vf = self.visible_files();
        if let Some(f) = keep {
            if let Some(p) = vf.iter().position(|x| *x == f) {
                self.file_cursor = p;
                self.ensure_diff_cursor();
                self.sync_list();
                return;
            }
        }
        self.file_cursor = self.file_cursor.min(vf.len().saturating_sub(1));
        self.reset_diff_cursor();
        self.sync_list();
    }

    pub fn select_file(&mut self, pos: usize) {
        let n = self.visible_files().len();
        if n == 0 {
            return;
        }
        self.file_cursor = pos.min(n - 1);
        self.reset_diff_cursor();
        self.sync_list();
    }

    // ---------- movement ----------

    pub fn file_move(&mut self, d: isize) {
        let n = self.visible_files().len() as isize;
        if n == 0 {
            return;
        }
        let p = (self.file_cursor as isize + d).clamp(0, n - 1) as usize;
        if p == self.file_cursor {
            self.toast = Some(if d > 0 { "last file".into() } else { "first file".into() });
            return;
        }
        self.select_file(p);
    }

    /// Next / previous file that still has unreviewed changes in this Scope.
    fn jump_unreviewed(&mut self, dir: isize) {
        let vf = self.visible_files();
        let n = vf.len() as isize;
        let mut p = self.file_cursor as isize + dir;
        while p >= 0 && p < n {
            if self.file_status(vf[p as usize]) != Status::Done {
                self.select_file(p as usize);
                return;
            }
            p += dir;
        }
        self.toast = Some(if dir > 0 { "no more unreviewed files below".into() } else { "no unreviewed files above".into() });
    }

    pub fn diff_move(&mut self, d: isize) {
        let Some(fi) = self.current_file() else { return };
        let vis = self.visible_src(fi);
        if vis.is_empty() {
            return;
        }
        let pos = vis.iter().position(|v| *v == self.diff_cursor).unwrap_or(0) as isize;
        let np = (pos + d).clamp(0, vis.len() as isize - 1) as usize;
        self.diff_cursor = vis[np];
    }

    fn page(&mut self, dir: isize) {
        let step = (self.view_h / 2).max(1) as isize;
        match self.focus {
            Pane::Diff => self.diff_move(dir * step),
            Pane::Files => self.list_move(dir * step),
            Pane::Commits => self.commit_move(dir * (self.commit_h / 2).max(1) as isize),
        }
    }

    /// `NG`: go to the N-th (1-based) commit / list item / diff row of the focused pane.
    fn goto_nth(&mut self, n: usize) {
        let n = n.max(1);
        match self.focus {
            Pane::Commits => {
                self.on_range = false;
                self.commit_cursor = (n - 1).min(self.commits.len() - 1);
            }
            Pane::Files => self.list_goto(n - 1),
            Pane::Diff => {
                if let Some(fi) = self.current_file() {
                    let vis = self.visible_src(fi);
                    if !vis.is_empty() {
                        self.diff_cursor = vis[(n - 1).min(vis.len() - 1)];
                    }
                }
            }
        }
    }

    /// Ctrl-e / Ctrl-y (one line) and Ctrl-f / Ctrl-b (a page): scroll the view, not the cursor.
    /// In the diff the next draw moves the cursor only if it would leave the view, like Vim.
    fn scroll_view(&mut self, d: isize) {
        match self.focus {
            Pane::Diff => self.scroll_delta += d,
            Pane::Files => self.list_move(d),
            Pane::Commits => self.commit_move(d),
        }
    }

    fn mv(&mut self, d: isize) {
        match self.focus {
            Pane::Diff => self.diff_move(d),
            Pane::Files => self.list_move(d),
            Pane::Commits => self.commit_move(d),
        }
    }

    fn go_top(&mut self) {
        match self.focus {
            Pane::Diff => self.diff_move(-100000),
            Pane::Files => self.list_goto(0),
            Pane::Commits => {
                self.commit_cursor = 0;
                self.on_range = self.commit_anchor.is_none();
            }
        }
    }

    fn go_bottom(&mut self) {
        match self.focus {
            Pane::Diff => self.diff_move(100000),
            Pane::Files => self.list_goto(usize::MAX / 2),
            Pane::Commits => {
                self.commit_cursor = self.commits.len() - 1;
                self.on_range = false;
            }
        }
    }

    fn hscroll_by(&mut self, d: isize) {
        let Some(fi) = self.current_file() else { return };
        let max_len = self.files[fi].rows.iter().map(|r| r.old.chars().count().max(r.new.chars().count())).max().unwrap_or(0);
        let cap = max_len.saturating_sub(8) as isize;
        self.hscroll = (self.hscroll as isize + d).clamp(0, cap.max(0)) as usize;
    }

    fn next_hunk(&mut self, dir: isize) {
        let Some(fi) = self.current_file() else { return };
        let hs = self.hunk_starts(fi);
        let cur = self.diff_cursor;
        let target = if dir > 0 {
            hs.iter().copied().find(|h| *h > cur)
        } else {
            hs.iter().copied().rev().find(|h| *h < cur)
        };
        match target {
            Some(t) => self.diff_cursor = t,
            None => self.toast = Some(if dir > 0 { "last hunk in this file".into() } else { "first hunk in this file".into() }),
        }
    }

    /// `]c` / `[c`: an explicit command, so it applies a single-commit Scope right away.
    fn step_commit(&mut self, dir: isize) {
        let n = self.commits.len() as isize;
        let single = self.scope.0 == self.scope.1 && !self.scope_is_whole();
        self.commit_cursor = if single {
            (self.commit_cursor as isize + dir).clamp(0, n - 1) as usize
        } else if dir > 0 {
            0
        } else {
            (n - 1) as usize
        };
        self.on_range = false;
        self.commit_anchor = None;
        self.apply_commit_scope();
    }

    // ---------- commits / Scope (Q37: the cursor moves freely, Enter applies) ----------

    pub fn commit_move(&mut self, d: isize) {
        let n = self.commits.len() as isize;
        // the pinned "Весь Range" row is position -1; a visual selection cannot include it
        let lo = if self.commit_anchor.is_some() { 0 } else { -1 };
        let pos = if self.on_range { -1 } else { self.commit_cursor as isize };
        let np = (pos + d).clamp(lo, n - 1);
        if np < 0 {
            self.on_range = true;
        } else {
            self.on_range = false;
            self.commit_cursor = np as usize;
        }
    }

    /// Mouse / picker rows carry a commit index, or RANGE_ROW for the pinned row.
    pub fn set_commit_cursor(&mut self, idx: usize) {
        if idx == RANGE_ROW {
            self.on_range = true;
        } else {
            self.on_range = false;
            self.commit_cursor = idx;
        }
    }

    /// The commit range the user is currently selecting (visual) or pointing at.
    pub fn commit_selection(&self) -> (usize, usize) {
        match self.commit_anchor {
            Some(a) => (a.min(self.commit_cursor), a.max(self.commit_cursor)),
            None => (self.commit_cursor, self.commit_cursor),
        }
    }

    pub fn apply_commit_scope(&mut self) {
        if self.on_range {
            self.commit_all();
            return;
        }
        let keep = self.current_file();
        self.scope = self.commit_selection();
        self.commit_anchor = None;
        if self.mode == Mode::Visual {
            self.mode = Mode::Normal;
        }
        self.refresh(keep);
        self.toast = Some(format!("Scope: {}", self.scope_label()));
    }

    pub fn commit_all(&mut self) {
        let keep = self.current_file();
        self.scope = (0, self.commits.len() - 1);
        self.on_range = true;
        self.commit_anchor = None;
        if self.mode == Mode::Visual {
            self.mode = Mode::Normal;
        }
        self.refresh(keep);
        self.toast = Some("Scope: whole Range".into());
    }

    pub fn commit_visual_toggle(&mut self) {
        if self.on_range && self.commit_anchor.is_none() {
            self.toast = Some("▣ Весь Range: Enter применяет весь Range, v выделяет диапазон коммитов (j)".into());
            return;
        }
        if self.commit_anchor.is_some() {
            self.commit_anchor = None;
            self.mode = Mode::Normal;
        } else {
            self.commit_anchor = Some(self.commit_cursor);
            self.mode = Mode::Visual;
        }
    }

    pub fn commit_visual_cancel(&mut self) {
        self.commit_anchor = None;
        self.mode = Mode::Normal;
    }

    // ---------- Reviewed / filter / layout ----------

    fn toggle_reviewed(&mut self) {
        let Some(fi) = self.current_file() else { return };
        let atoms = self.atoms(fi);
        let all = atoms.iter().all(|c| self.reviewed.contains(&(fi, *c)));
        let path = self.files[fi].path;
        if all {
            for c in &atoms {
                self.reviewed.remove(&(fi, *c));
            }
            self.toast = Some(format!("unmarked {path}"));
            self.refresh(Some(fi));
        } else {
            for c in &atoms {
                self.reviewed.insert((fi, *c));
            }
            let still = self.file_visible(fi);
            self.refresh(if still { Some(fi) } else { None });
            self.toast = Some(format!("Reviewed: {path}"));
        }
    }

    /// `M`: mark Reviewed and jump to the next file that is not yet reviewed.
    fn mark_and_next(&mut self) {
        let Some(fi) = self.current_file() else { return };
        let vf = self.visible_files();
        let next = vf
            .iter()
            .skip(self.file_cursor + 1)
            .copied()
            .find(|f| self.file_status(*f) != Status::Done)
            .or_else(|| vf.iter().take(self.file_cursor).copied().find(|f| self.file_status(*f) != Status::Done));
        for c in self.atoms(fi) {
            self.reviewed.insert((fi, c));
        }
        let path = self.files[fi].path;
        match next {
            Some(nf) => {
                self.refresh(Some(nf));
                self.toast = Some(format!("Reviewed: {path}  →  {}", self.files[nf].path));
            }
            None => {
                let still = self.file_visible(fi);
                self.refresh(if still { Some(fi) } else { None });
                self.toast = Some(format!("Reviewed: {path}  (nothing left to review in this Scope)"));
            }
        }
    }

    fn toggle_filter(&mut self) {
        let keep = self.current_file();
        self.unreviewed_only = !self.unreviewed_only;
        self.refresh(keep);
        self.reset_diff_cursor();
        self.toast = Some(format!("filter unreviewed-only: {}", if self.unreviewed_only { "on" } else { "off" }));
    }

    /// `s`: switch between side-by-side and inline; the manual choice beats the width heuristic.
    fn toggle_layout(&mut self) {
        self.dlayout = if self.last_inline { DiffLayout::Side } else { DiffLayout::Inline };
        self.dsel = None;
        self.toast = Some(match self.dlayout {
            DiffLayout::Side => "diff layout: side-by-side (manual; :layout auto restores the width rule)".into(),
            _ => "diff layout: inline (manual; :layout auto restores the width rule)".into(),
        });
    }

    fn toggle_sidebar(&mut self) {
        let now = self.sidebar_visible();
        self.sidebar_manual = Some(!now);
        if now && self.focus != Pane::Diff {
            self.focus = Pane::Diff;
        }
        self.toast = Some(if now {
            "sidebar hidden: <Space>c commits, <Space>f files, <Space>e shows it again".into()
        } else {
            "sidebar shown (manual: it stays until you toggle it again)".into()
        });
    }

    // ---------- search ----------

    fn search_exec(&mut self) {
        self.search = self.search_input.clone();
        self.search_hits.clear();
        self.search_idx = 0;
        let q = self.search.to_lowercase();
        if q.is_empty() {
            return;
        }
        if self.focus == Pane::Commits {
            // in the commit list `/` matches "c12", the sha and the message
            self.search_commits = true;
            for (i, c) in self.commits.iter().enumerate() {
                if format!("c{} {} {}", i + 1, c.sha, c.msg).to_lowercase().contains(&q) {
                    self.search_hits.push(i);
                }
            }
            if self.search_hits.is_empty() {
                self.toast = Some(format!("E486: pattern not found: {}", self.search));
            } else {
                let cur = if self.on_range { 0 } else { self.commit_cursor };
                self.search_idx = self.search_hits.iter().position(|h| *h >= cur).unwrap_or(0);
                self.set_commit_cursor(self.search_hits[self.search_idx]);
            }
            return;
        }
        self.search_commits = false;
        let Some(fi) = self.current_file() else { return };
        for v in self.rows_view(fi) {
            if let RV::Row(i, r) = v {
                if r.old.to_lowercase().contains(&q) || r.new.to_lowercase().contains(&q) {
                    self.search_hits.push(i);
                }
            }
        }
        if self.search_hits.is_empty() {
            self.toast = Some(format!("E486: pattern not found: {}", self.search));
        } else {
            let cur = self.diff_cursor;
            self.search_idx = self.search_hits.iter().position(|h| *h >= cur).unwrap_or(0);
            self.diff_cursor = self.search_hits[self.search_idx];
        }
    }

    fn search_next(&mut self, dir: isize) {
        if self.search_hits.is_empty() {
            self.toast = Some("no search; use /".into());
            return;
        }
        let n = self.search_hits.len() as isize;
        self.search_idx = ((self.search_idx as isize + dir).rem_euclid(n)) as usize;
        if self.search_commits {
            self.set_commit_cursor(self.search_hits[self.search_idx]);
        } else {
            self.diff_cursor = self.search_hits[self.search_idx];
        }
    }

    // ---------- clipboard ----------

    /// Queue a real OSC 52 write (ESC ] 52 ; c ; base64 ST); main.rs flushes it to the terminal.
    pub fn emit_clip(&mut self, text: &str) {
        self.osc_out.push(format!("\x1b]52;c;{}\x1b\\", b64(text.as_bytes())));
        self.clip_log.push(text.to_string());
    }

    /// VISUAL `y` (code only) and `<Space>y` (code + Comments as quoted lines). Both are plain
    /// copies: no Comment changes state and no Round closes.
    /// Format (Q46): if the rows contain removed lines AND added lines, copy a diff with
    /// ' ' '+' '-' prefixes; otherwise plain code (new code if nothing was removed, old code
    /// if nothing was added).
    pub fn copy_visual(&mut self, with_comments: bool) {
        let (Some(fi), Some(an)) = (self.current_file(), self.diff_anchor) else {
            self.toast = Some("сначала выделите строки через v в диффе".into());
            return;
        };
        let (a, b) = (an.min(self.diff_cursor), an.max(self.diff_cursor));
        let vis = self.visible_src(fi);
        // (mark, text, source row)
        let mut lines: Vec<(char, String, usize)> = Vec::new();
        let view: Vec<(usize, Row)> = self
            .rows_view(fi)
            .into_iter()
            .filter_map(|v| match v {
                RV::Row(i, row) => Some((i, row)),
                RV::Elide(_) => None,
            })
            .collect();
        for ri in vis.into_iter().filter(|r| *r >= a && *r <= b) {
            let Some((_, eff)) = view.iter().find(|(i, _)| *i == ri) else { continue };
            let (k, o, n) = (eff.kind, eff.old.clone(), eff.new.clone());
            match k {
                Kind::Ctx => lines.push((' ', n, ri)),
                Kind::Add => lines.push(('+', n, ri)),
                Kind::Del => lines.push(('-', o, ri)),
                Kind::Chg => {
                    lines.push(('-', o, ri));
                    lines.push(('+', n, ri));
                }
            }
        }
        let as_diff = lines.iter().any(|l| l.0 == '+') && lines.iter().any(|l| l.0 == '-');
        let (text, quoted) = self.render_copy(fi, &lines, as_diff, with_comments);
        self.diff_anchor = None;
        self.mode = Mode::Normal;
        self.emit_clip(&text);
        self.toast = Some(self.copy_toast(&text, as_diff, with_comments, quoted));
    }

    pub fn copy_toast(&self, text: &str, as_diff: bool, with_comments: bool, quoted: usize) -> String {
        format!(
            "Скопировано {} симв. ({}{})",
            text.chars().count(),
            if as_diff { "как diff" } else { "как код" },
            if with_comments { format!(" + {quoted} Comments") } else { String::new() }
        )
    }

    /// Join copied code lines. With `with_comments`, every Line Comment whose last anchored row
    /// lies inside the copied rows is inserted after the last copied line at or before that row,
    /// as quoted `> ` lines (Q47). `parts` = (mark, code, source row).
    pub fn render_copy(&self, fi: usize, parts: &[(char, String, usize)], as_diff: bool, with_comments: bool) -> (String, usize) {
        let mut after: Vec<Vec<usize>> = vec![Vec::new(); parts.len()];
        let mut quoted = 0;
        if with_comments && !parts.is_empty() {
            let (first, last) = (parts[0].2, parts[parts.len() - 1].2);
            for (ci, c) in self.comments.iter().enumerate() {
                if c.kind != CKind::Line || c.file != fi || c.b < first || c.b > last {
                    continue;
                }
                if let Some(idx) = parts.iter().rposition(|p| p.2 <= c.b) {
                    after[idx].push(ci);
                    quoted += 1;
                }
            }
        }
        let mut out: Vec<String> = Vec::new();
        for (i, (mark, text, _)) in parts.iter().enumerate() {
            out.push(if as_diff { format!("{mark}{text}") } else { text.clone() });
            for ci in &after[i] {
                for l in self.comments[*ci].text.lines() {
                    out.push(format!("> {l}"));
                }
            }
        }
        (out.join("\n"), quoted)
    }

    // ---------- Comments / Prompt ----------

    pub fn anchor_text(&self, kind: CKind, fi: usize, a: usize, b: usize) -> String {
        match kind {
            CKind::Line => {
                let f = &self.files[fi];
                let (la, lb) = (line_no(f, a), line_no(f, b));
                if la == lb {
                    format!("строку {la}")
                } else {
                    format!("строки {la}–{lb}")
                }
            }
            CKind::File => "файл".into(),
            CKind::Range => "весь Range".into(),
        }
    }

    fn open_editor(&mut self, kind: CKind) {
        let fi = self.current_file();
        let (file, a, b) = match kind {
            CKind::Line => {
                let Some(fi) = fi else { return };
                let (a, b) = match self.diff_anchor {
                    Some(an) => (an.min(self.diff_cursor), an.max(self.diff_cursor)),
                    None => (self.diff_cursor, self.diff_cursor),
                };
                (fi, a, b)
            }
            CKind::File => {
                let Some(fi) = fi else { return };
                (fi, 0, 0)
            }
            CKind::Range => (0, 0, 0),
        };
        self.diff_anchor = None;
        self.editor = Some(Editor { text: String::new(), insert: false, kind, file, a, b, confirm: false });
        self.mode = Mode::Comment;
    }

    fn save_editor(&mut self) {
        if let Some(ed) = self.editor.take() {
            let text = ed.text.trim().to_string();
            if text.is_empty() {
                self.toast = Some("empty Comment discarded".into());
            } else {
                let commit = match ed.kind {
                    CKind::Line => self.commit_of_row(ed.file, ed.b),
                    _ => self.scope.1,
                };
                self.comments.push(Comment { kind: ed.kind, file: ed.file, a: ed.a, b: ed.b, text, state: CState::Open, commit });
                self.toast = Some(format!("Comment на {} сохранён (open)", self.anchor_text(ed.kind, ed.file, ed.a, ed.b)));
            }
        }
        self.mode = Mode::Normal;
    }

    pub fn row_text(&self, fi: usize, ri: usize) -> String {
        let r = &self.files[fi].rows[ri];
        match r.kind {
            Kind::Del => r.old.clone(),
            _ => r.new.clone(),
        }
    }

    /// Prompt order (Q43): Range-level, then file-level, then line-level.
    pub fn build_prompt(&self, idxs: &[usize]) -> String {
        let first = self.commits.first().unwrap().sha;
        let last = self.commits.last().unwrap().sha;
        let mut s = String::new();
        s.push_str(&format!(
            "You are the author of branch `feat/http-retry` (commits {first}..{last}).\nBelow are review comments from the reviewer. Answer every question and make the requested changes.\n"
        ));
        let of = |k: CKind| idxs.iter().copied().filter(move |i| self.comments[*i].kind == k).collect::<Vec<_>>();
        let range = of(CKind::Range);
        if !range.is_empty() {
            s.push_str("\n## General comments\n");
            for (n, i) in range.iter().enumerate() {
                s.push_str(&format!("\n{}. {}\n", n + 1, self.comments[*i].text));
            }
        }
        let file = of(CKind::File);
        if !file.is_empty() {
            s.push_str("\n## File comments\n");
            for (n, i) in file.iter().enumerate() {
                let c = &self.comments[*i];
                s.push_str(&format!("\n{}. {}\n{}\n", n + 1, self.files[c.file].path, c.text));
            }
        }
        let line = of(CKind::Line);
        if !line.is_empty() {
            s.push_str("\n## Line comments\n");
            for (n, i) in line.iter().enumerate() {
                let c = &self.comments[*i];
                let f = &self.files[c.file];
                let (la, lb) = (line_no(f, c.a), line_no(f, c.b));
                s.push_str(&format!("\n{}. {}:{}", n + 1, f.path, if la == lb { la.to_string() } else { format!("{la}-{lb}") }));
                s.push_str(&format!(" (commit {})\n```\n", self.commits[c.commit].sha));
                for r in c.a..=c.b {
                    s.push_str(&self.row_text(c.file, r));
                    s.push('\n');
                }
                s.push_str("```\n");
                s.push_str(&c.text);
                s.push('\n');
            }
        }
        s
    }

    pub fn open_comment_idxs(&self) -> Vec<usize> {
        (0..self.comments.len()).filter(|i| self.comments[*i].state == CState::Open).collect()
    }

    /// Any copy of Comments as Prompt text: the copied Comments become `sent` and the Round
    /// closes. A Round that got no commits at all is not closed into an empty Round of its
    /// own: it stays and the new Prompt number is added to it ("merges into the next one").
    fn copy_prompt(&mut self, idxs: Vec<usize>, what: &str) {
        if idxs.is_empty() {
            self.toast = Some(format!("{what}: нечего копировать (нет подходящих Comments)"));
            return;
        }
        let text = self.build_prompt(&idxs);
        for i in &idxs {
            self.comments[*i].state = CState::Sent;
        }
        self.prompt_no += 1;
        let closed = self.round;
        let merged = !self.commits.iter().any(|c| c.round == self.round);
        if merged {
            self.round_after[self.round as usize - 1].push(self.prompt_no);
        } else {
            self.round += 1;
            self.round_after.push(vec![self.prompt_no]);
        }
        self.emit_clip(&text);
        self.toast = Some(format!(
            "Prompt #{} ({what}) → буфер через OSC 52: {} симв., {} Comments · {}",
            self.prompt_no,
            text.chars().count(),
            idxs.len(),
            if merged {
                format!("Round {closed} ещё без коммитов, Prompt добавлен к нему")
            } else {
                format!("Round {closed} закрыт, теперь Round {}", self.round)
            }
        ));
        self.last_prompt = Some(text);
    }

    fn yank_prompt(&mut self) {
        let idxs = self.open_comment_idxs();
        self.copy_prompt(idxs, "все open");
    }

    fn yank_file_prompt(&mut self) {
        let Some(fi) = self.current_file() else { return };
        let idxs: Vec<usize> = self.open_comment_idxs().into_iter().filter(|i| self.comments[*i].kind != CKind::Range && self.comments[*i].file == fi).collect();
        self.copy_prompt(idxs, "текущий файл");
    }

    fn yank_cursor_prompt(&mut self) {
        let idxs = self.comments_under_cursor();
        self.copy_prompt(idxs, "под курсором");
    }

    /// Comments anchored to the cursor row (any row of their range), plus the file-level ones
    /// when the cursor is on the first row of the file (where their box is drawn).
    pub fn comments_under_cursor(&self) -> Vec<usize> {
        let Some(fi) = self.current_file() else { return Vec::new() };
        let first = self.visible_src(fi).first().copied();
        (0..self.comments.len())
            .filter(|i| {
                let c = &self.comments[*i];
                match c.kind {
                    CKind::Line => c.file == fi && c.a <= self.diff_cursor && self.diff_cursor <= c.b,
                    CKind::File => c.file == fi && Some(self.diff_cursor) == first,
                    CKind::Range => false,
                }
            })
            .collect()
    }

    /// `o`: reopen the sent Comment(s) under the cursor.
    fn reopen_comment(&mut self) {
        let mut n = 0;
        for i in self.comments_under_cursor() {
            if self.comments[i].state == CState::Sent {
                self.comments[i].state = CState::Open;
                n += 1;
            }
        }
        self.toast = Some(if n > 0 { format!("{n} Comment снова open") } else { "под курсором нет отправленных Comments".into() });
    }

    // ---------- panes ----------

    /// Ctrl-w h/j/k/l and Ctrl-h/j/k/l: geometric focus moves between the three panes.
    fn pane_dir(&mut self, c: char) {
        if !self.sidebar_visible() {
            self.toast = Some("sidebar hidden: <Space>e shows it, <Space>c / <Space>f open the pickers".into());
            return;
        }
        self.focus = match (self.focus, c) {
            (Pane::Diff, 'h') => Pane::Files,
            (Pane::Files, 'l') | (Pane::Commits, 'l') => Pane::Diff,
            (Pane::Files, 'k') => Pane::Commits,
            (Pane::Commits, 'j') => Pane::Files,
            (p, _) => p,
        };
    }

    fn cycle_focus(&mut self) {
        if !self.sidebar_visible() {
            return;
        }
        self.focus = match self.focus {
            Pane::Commits => Pane::Files,
            Pane::Files => Pane::Diff,
            Pane::Diff => Pane::Commits,
        };
    }

    pub fn open_overlay(&mut self, o: Overlay) {
        self.overlay = Some(o);
        self.overlay_scroll = 0;
        self.osel = None;
        self.dsel = None;
        match o {
            Overlay::Commits => {
                self.commit_anchor = None;
                self.mode = Mode::Normal;
            }
            Overlay::Files => self.overlay_cursor = self.file_cursor,
            _ => {}
        }
    }

    pub fn close_overlay(&mut self) {
        if self.overlay == Some(Overlay::Commits) {
            self.commit_anchor = None;
            self.mode = Mode::Normal;
        }
        self.overlay = None;
        self.osel = None;
        self.drag_overlay = false;
    }

    // ---------- commands ----------

    fn run_command(&mut self, line: &str) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["q"] | ["quit"] | ["q!"] => self.quit = true,
            ["scope", "all"] => self.commit_all(),
            ["scope", r] => {
                let parse = |s: &str| s.parse::<usize>().ok().filter(|n| *n >= 1 && *n <= self.commits.len());
                let (lo, hi) = match r.split_once('-') {
                    Some((a, b)) => (parse(a), parse(b)),
                    None => (parse(r), parse(r)),
                };
                if let (Some(lo), Some(hi)) = (lo, hi) {
                    let keep = self.current_file();
                    self.scope = (lo.min(hi) - 1, lo.max(hi) - 1);
                    self.commit_cursor = lo.min(hi) - 1;
                    self.on_range = false;
                    self.refresh(keep);
                } else {
                    self.toast = Some("usage: :scope all | :scope 3 | :scope 3-5".into());
                }
            }
            ["reload"] => self.toast = Some("reload: no new commits on the branch (prototype)".into()),
            ["analyze"] => self.toast = Some("Analyst queued: Summary + Groups (prototype, nothing runs)".into()),
            ["diagram"] => self.open_overlay(Overlay::Diagram),
            ["help"] => self.open_overlay(Overlay::Help),
            ["summary"] => self.open_overlay(Overlay::Summary),
            ["prompt", "show"] => self.open_overlay(Overlay::Prompt),
            ["prompt", "save", ..] => self.toast = Some("Prompt written to .git/moonhare/prompt-2.md (prototype: not really)".into()),
            ["reset"] => self.toast = Some("reset: would clear this Review (prototype: not really)".into()),
            ["comment"] => self.open_editor(CKind::Range),
            ["set", "mouse"] => {
                self.mouse_on = true;
                self.mouse_dirty = true;
                self.toast = Some("mouse on (terminal text selection needs Shift while it is on)".into());
            }
            ["set", "nomouse"] => {
                self.mouse_on = false;
                self.mouse_dirty = true;
                self.dsel = None;
                self.osel = None;
                self.toast = Some("mouse off: the terminal selects text natively now".into());
            }
            ["set", "sidebar"] => {
                self.sidebar_manual = Some(true);
                self.toast = Some("sidebar shown".into());
            }
            ["set", "nosidebar"] => {
                self.sidebar_manual = Some(false);
                if self.focus != Pane::Diff {
                    self.focus = Pane::Diff;
                }
                self.toast = Some("sidebar hidden".into());
            }
            ["layout", m] => {
                self.dlayout = match *m {
                    "side" => DiffLayout::Side,
                    "inline" => DiffLayout::Inline,
                    _ => DiffLayout::Auto,
                };
                self.dsel = None;
                self.toast = Some(format!("diff layout: {m}"));
            }
            [] => {}
            _ => self.toast = Some(format!("not a command: {line}")),
        }
    }

    // ---------- keys ----------

    pub fn on_key(&mut self, k: KeyEvent) {
        self.toast = None;
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.overlay.is_some() {
            self.overlay_key(k);
            return;
        }
        match self.mode {
            Mode::Command => self.command_key(k),
            Mode::Search => self.search_key(k),
            Mode::Comment => self.comment_key(k),
            Mode::Normal | Mode::Visual => self.normal_key(k),
        }
    }

    fn command_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                let line = std::mem::take(&mut self.cmd);
                self.mode = Mode::Normal;
                self.run_command(line.trim());
            }
            KeyCode::Backspace => {
                if self.cmd.pop().is_none() {
                    self.mode = Mode::Normal;
                }
            }
            KeyCode::Char(c) => self.cmd.push(c),
            _ => {}
        }
    }

    fn search_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                self.search_exec();
            }
            KeyCode::Backspace => {
                if self.search_input.pop().is_none() {
                    self.mode = Mode::Normal;
                }
            }
            KeyCode::Char(c) => self.search_input.push(c),
            _ => {}
        }
    }

    /// Q7: `i` inserts, `Esc` goes back to normal, `Enter` in normal saves,
    /// a second `Esc` in normal discards (with a y/n question if there is text).
    fn comment_key(&mut self, k: KeyEvent) {
        let Some(ed) = self.editor.as_mut() else {
            self.mode = Mode::Normal;
            return;
        };
        if ed.confirm {
            match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.editor = None;
                    self.mode = Mode::Normal;
                    self.toast = Some("Comment discarded".into());
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => ed.confirm = false,
                _ => {}
            }
            return;
        }
        if ed.insert {
            match k.code {
                KeyCode::Esc => ed.insert = false,
                KeyCode::Enter => ed.text.push('\n'),
                KeyCode::Backspace => {
                    ed.text.pop();
                }
                KeyCode::Char(c) => ed.text.push(c),
                _ => {}
            }
        } else {
            match k.code {
                KeyCode::Char('i') | KeyCode::Char('a') => ed.insert = true,
                KeyCode::Enter => self.save_editor(),
                KeyCode::Esc => {
                    if ed.text.trim().is_empty() {
                        self.editor = None;
                        self.mode = Mode::Normal;
                        self.toast = Some("Comment cancelled".into());
                    } else {
                        ed.confirm = true;
                    }
                }
                _ => {}
            }
        }
    }

    fn overlay_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let o = self.overlay.unwrap();
        match o {
            Overlay::Commits => match k.code {
                KeyCode::Esc | KeyCode::Char('q') => self.close_overlay(),
                KeyCode::Enter => {
                    self.apply_commit_scope();
                    self.close_overlay();
                }
                KeyCode::Char('j') | KeyCode::Down => self.commit_move(1),
                KeyCode::Char('k') | KeyCode::Up => self.commit_move(-1),
                KeyCode::Char('v') => self.commit_visual_toggle(),
                KeyCode::Char('a') => {
                    self.commit_all();
                    self.close_overlay();
                }
                _ => {}
            },
            Overlay::Files => {
                let n = self.visible_files().len();
                match k.code {
                    KeyCode::Esc | KeyCode::Char('q') => self.close_overlay(),
                    KeyCode::Enter => {
                        let p = self.overlay_cursor;
                        self.close_overlay();
                        self.select_file(p);
                    }
                    KeyCode::Char('j') | KeyCode::Down => self.overlay_cursor = (self.overlay_cursor + 1).min(n.saturating_sub(1)),
                    KeyCode::Char('k') | KeyCode::Up => self.overlay_cursor = self.overlay_cursor.saturating_sub(1),
                    KeyCode::Char('g') => self.overlay_cursor = 0,
                    KeyCode::Char('G') => self.overlay_cursor = n.saturating_sub(1),
                    _ => {}
                }
            }
            _ => match k.code {
                KeyCode::Esc => {
                    if self.osel.is_some() {
                        self.osel = None;
                    } else {
                        self.close_overlay();
                    }
                }
                KeyCode::Char('q') => self.close_overlay(),
                KeyCode::Char('j') | KeyCode::Down => self.overlay_scroll += 1,
                KeyCode::Char('k') | KeyCode::Up => self.overlay_scroll = self.overlay_scroll.saturating_sub(1),
                KeyCode::Char('d') if ctrl => self.overlay_scroll += 10,
                KeyCode::Char('u') if ctrl => self.overlay_scroll = self.overlay_scroll.saturating_sub(10),
                KeyCode::Char('g') => self.overlay_scroll = 0,
                _ => {}
            },
        }
    }

    fn start_chord(&mut self, c: Chord) {
        self.chord = Some(c);
        self.chord_at = Instant::now();
        self.whichkey = false;
    }

    fn chord_key(&mut self, ch: Chord, k: KeyEvent) {
        let KeyCode::Char(c) = k.code else { return };
        match (ch, c) {
            (Chord::G, 'g') => self.go_top(),
            (Chord::G, 'd') => self.toast = Some("gd (к определению): позже, отдельный срез с LSP".into()),
            (Chord::G, 'r') => self.toast = Some("gr (ссылки): позже, отдельный срез с LSP".into()),
            (Chord::Next, 'h') => self.next_hunk(1),
            (Chord::Prev, 'h') => self.next_hunk(-1),
            (Chord::Next, 'f') => self.file_move(1),
            (Chord::Prev, 'f') => self.file_move(-1),
            (Chord::Next, 'u') => self.jump_unreviewed(1),
            (Chord::Prev, 'u') => self.jump_unreviewed(-1),
            (Chord::Next, 'c') => self.step_commit(1),
            (Chord::Prev, 'c') => self.step_commit(-1),
            (Chord::CtrlW, 'h' | 'j' | 'k' | 'l') => self.pane_dir(c),
            (Chord::CtrlW, 'w') => self.cycle_focus(),
            (Chord::Leader, 'f') => self.open_overlay(Overlay::Files),
            (Chord::Leader, 'c') => self.open_overlay(Overlay::Commits),
            (Chord::Leader, 'e') => self.toggle_sidebar(),
            (Chord::Leader, 's') => self.open_overlay(Overlay::Summary),
            (Chord::Leader, 'p') => self.start_chord(Chord::LeaderPrompt),
            (Chord::Leader, 'y') => self.copy_with_comments(),
            (Chord::Leader, 'u') => self.toggle_filter(),
            (Chord::Leader, 'd') => self.open_overlay(Overlay::Diagram),
            (Chord::LeaderPrompt, 'p') => self.yank_prompt(),
            (Chord::LeaderPrompt, 'f') => self.yank_file_prompt(),
            (Chord::LeaderPrompt, 'c') => self.yank_cursor_prompt(),
            (Chord::LeaderPrompt, 's') => self.open_overlay(Overlay::Prompt),
            _ => {}
        }
    }

    fn enter_key(&mut self) {
        match self.focus {
            Pane::Commits => self.apply_commit_scope(),
            Pane::Files => {
                let items = self.list_items();
                match items.get(self.list_cursor).copied() {
                    Some(ListItem::Group(g)) => self.list_toggle_fold(g),
                    Some(ListItem::File(_)) => self.focus = Pane::Diff,
                    None => {}
                }
            }
            Pane::Diff => {}
        }
    }

    fn normal_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(ch) = self.chord.take() {
            self.whichkey = false;
            if k.code != KeyCode::Esc {
                self.chord_key(ch, k);
            }
            return;
        }
        // Vim count prefix: `10j`, `5k`, `12G`; a lone `0` still resets the sideways scroll
        if !ctrl {
            if let KeyCode::Char(c) = k.code {
                if c.is_ascii_digit() && (c != '0' || self.count.is_some()) {
                    let d = c.to_digit(10).unwrap() as usize;
                    self.count = Some((self.count.unwrap_or(0) * 10 + d).min(9999));
                    return;
                }
            }
        }
        let count_opt = self.count.take();
        let cnt = count_opt.unwrap_or(1).max(1) as isize;
        match k.code {
            KeyCode::Esc => self.escape(),
            KeyCode::Tab => self.cycle_focus(),
            KeyCode::Enter => self.enter_key(),
            KeyCode::Down => self.mv(cnt),
            KeyCode::Up => self.mv(-cnt),
            KeyCode::Left => self.h_key(),
            KeyCode::Right => self.l_key(),
            KeyCode::Char(c) if ctrl => match c {
                'd' => self.page(1),
                'u' => self.page(-1),
                'f' => self.scroll_view((self.view_h as isize - 2).max(1)),
                'b' => self.scroll_view(-(self.view_h as isize - 2).max(1)),
                'e' => self.scroll_view(1),
                'y' => self.scroll_view(-1),
                'w' => self.start_chord(Chord::CtrlW),
                'h' | 'j' | 'k' | 'l' => self.pane_dir(c),
                _ => {}
            },
            KeyCode::Char(c) => match c {
                ' ' => self.start_chord(Chord::Leader),
                'j' => self.mv(cnt),
                'k' => self.mv(-cnt),
                'h' => self.h_key(),
                'l' => self.l_key(),
                'g' => self.start_chord(Chord::G),
                '[' => self.start_chord(Chord::Prev),
                ']' => self.start_chord(Chord::Next),
                'G' => match count_opt {
                    Some(n) => self.goto_nth(n),
                    None => self.go_bottom(),
                },
                'v' => self.visual_toggle(),
                ':' => {
                    self.mode = Mode::Command;
                    self.cmd.clear();
                }
                '/' => {
                    self.mode = Mode::Search;
                    self.search_input.clear();
                }
                'n' => self.search_next(1),
                'N' => self.search_next(-1),
                'm' => self.toggle_reviewed(),
                'M' => self.mark_and_next(),
                's' => self.toggle_layout(),
                'c' => {
                    if self.focus == Pane::Diff {
                        self.open_editor(CKind::Line)
                    } else {
                        self.toast = Some("c comments diff lines: move focus to the diff first (Ctrl-w l); C comments the whole file".into())
                    }
                }
                'C' => self.open_editor(CKind::File),
                'y' => {
                    if self.mode == Mode::Visual && self.diff_anchor.is_some() {
                        self.copy_visual(false);
                    }
                }
                'Y' => self.yank_prompt(),
                'o' => {
                    if self.focus == Pane::Diff {
                        self.reopen_comment()
                    }
                }
                '?' => self.open_overlay(Overlay::Help),
                'a' => {
                    if self.focus == Pane::Commits {
                        self.commit_all()
                    }
                }
                '0' => self.hscroll = 0,
                _ => {}
            },
            _ => {}
        }
    }

    fn h_key(&mut self) {
        match self.focus {
            Pane::Diff => self.hscroll_by(-8),
            Pane::Files => self.list_fold(),
            Pane::Commits => {}
        }
    }

    fn l_key(&mut self) {
        match self.focus {
            Pane::Diff => self.hscroll_by(8),
            Pane::Files => self.list_unfold_or_enter(),
            Pane::Commits => {}
        }
    }

    fn escape(&mut self) {
        if self.dsel.is_some() {
            self.dsel = None;
        } else if self.mode == Mode::Visual {
            if self.commit_anchor.is_some() {
                self.commit_visual_cancel();
            } else {
                self.diff_anchor = None;
                self.mode = Mode::Normal;
            }
        } else if !self.search_hits.is_empty() {
            self.search_hits.clear();
        }
    }

    fn visual_toggle(&mut self) {
        if self.mode == Mode::Visual {
            self.commit_anchor = None;
            self.diff_anchor = None;
            self.mode = Mode::Normal;
            return;
        }
        match self.focus {
            Pane::Commits => self.commit_visual_toggle(),
            Pane::Diff => {
                self.diff_anchor = Some(self.diff_cursor);
                self.mode = Mode::Visual;
            }
            Pane::Files => self.toast = Some("v выделяет коммиты (в списке коммитов) или строки (в диффе)".into()),
        }
    }
}

/// Group headers + files in display order. With `honor_folds` the files of folded Groups are left out.
pub fn items_for(app: &App, honor_folds: bool) -> Vec<ListItem> {
    let vf = app.visible_files();
    let mut out = Vec::new();
    let mut last = usize::MAX;
    for (pos, fi) in vf.iter().enumerate() {
        let g = app.files[*fi].group;
        if g != last {
            last = g;
            out.push(ListItem::Group(g));
        }
        if !(honor_folds && app.folded.contains(&g)) {
            out.push(ListItem::File(pos));
        }
    }
    out
}

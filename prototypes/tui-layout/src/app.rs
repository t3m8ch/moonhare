// PROTOTYPE shared state and key handling. All three layout variants drive this
// same state; the layouts themselves live in va.rs / vb.rs / vc.rs.

use std::collections::HashSet;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::data::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    A,
    B,
    C,
}

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

pub struct Editor {
    pub text: String,
    pub insert: bool,
    pub file: usize,
    pub a: usize,
    pub b: usize,
}

pub enum RV {
    Row(usize, Row),
    Elide(usize),
}

pub struct App {
    pub variant: Variant,
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
    pub commit_cursor: usize,
    pub commit_anchor: Option<usize>,
    pub scope_before: (usize, usize),
    pub file_cursor: usize,
    pub diff_cursor: usize,
    pub diff_anchor: Option<usize>,
    pub diff_scroll: usize,
    pub view_h: usize,
    pub overlay: Option<Overlay>,
    pub overlay_scroll: usize,
    pub overlay_cursor: usize,
    pub pending: Option<char>,
    pub cmd: String,
    pub search_input: String,
    pub search: String,
    pub search_hits: Vec<usize>,
    pub search_idx: usize,
    pub editor: Option<Editor>,
    pub toast: Option<String>,
    pub round: u8,
    pub prompt_no: u32,
    pub last_prompt: Option<String>,
    pub quit: bool,
}

impl App {
    pub fn new(variant: Variant) -> App {
        let files = files();
        let reviewed: HashSet<(usize, usize)> = initial_reviewed(&files).into_iter().collect();
        let comments = initial_comments(&files);
        let n = commits().len();
        let mut app = App {
            variant,
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
            commit_cursor: 0,
            commit_anchor: None,
            scope_before: (0, n - 1),
            file_cursor: 1, // src/retry/backoff.rs
            diff_cursor: 0,
            diff_anchor: None,
            diff_scroll: 0,
            view_h: 20,
            overlay: None,
            overlay_scroll: 0,
            overlay_cursor: 0,
            pending: None,
            cmd: String::new(),
            search_input: String::new(),
            search: String::new(),
            search_hits: Vec::new(),
            search_idx: 0,
            editor: None,
            toast: None,
            round: 2,
            prompt_no: 1,
            last_prompt: None,
            quit: false,
        };
        app.set_variant(variant);
        app.reset_diff_cursor();
        app
    }

    pub fn set_variant(&mut self, v: Variant) {
        let keep = self.current_file();
        self.variant = v;
        self.overlay = None;
        self.mode = Mode::Normal;
        self.commit_anchor = None;
        self.diff_anchor = None;
        self.focus = match v {
            Variant::A => Pane::Files,
            _ => Pane::Diff,
        };
        if v == Variant::C {
            self.unreviewed_only = true; // the queue is the unreviewed work by definition
        }
        self.refresh(keep);
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
        self.comments.iter().filter(|c| c.file == fi).count()
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
            // a context row has both sides; approximate the missing line number
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

    // ---------- selection upkeep ----------

    pub fn reset_diff_cursor(&mut self) {
        self.diff_anchor = None;
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
        let vf = self.visible_files();
        if let Some(f) = keep {
            if let Some(p) = vf.iter().position(|x| *x == f) {
                self.file_cursor = p;
                self.ensure_diff_cursor();
                return;
            }
        }
        self.file_cursor = self.file_cursor.min(vf.len().saturating_sub(1));
        self.reset_diff_cursor();
    }

    pub fn select_file(&mut self, pos: usize) {
        let n = self.visible_files().len();
        if n == 0 {
            return;
        }
        self.file_cursor = pos.min(n - 1);
        self.reset_diff_cursor();
    }

    // ---------- movement ----------

    pub fn file_move(&mut self, d: isize) {
        let n = self.visible_files().len() as isize;
        if n == 0 {
            return;
        }
        let p = (self.file_cursor as isize + d).clamp(0, n - 1) as usize;
        self.select_file(p);
    }

    fn diff_move(&mut self, d: isize) {
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
            Pane::Files => self.file_move(dir * step),
            Pane::Commits => self.commit_move(dir),
        }
    }

    fn mv(&mut self, d: isize) {
        match self.focus {
            Pane::Diff => self.diff_move(d),
            Pane::Files => self.file_move(d),
            Pane::Commits => self.commit_move(d),
        }
    }

    fn go_top(&mut self) {
        match self.focus {
            Pane::Diff => self.diff_move(-100000),
            Pane::Files => self.select_file(0),
            Pane::Commits => {
                self.commit_cursor = 0;
                self.apply_commit_cursor();
            }
        }
    }

    fn go_bottom(&mut self) {
        match self.focus {
            Pane::Diff => self.diff_move(100000),
            Pane::Files => self.select_file(usize::MAX / 2),
            Pane::Commits => {
                self.commit_cursor = self.commits.len() - 1;
                self.apply_commit_cursor();
            }
        }
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
        self.apply_commit_cursor();
    }

    // ---------- commits / Scope ----------

    fn apply_commit_cursor(&mut self) {
        let keep = self.current_file();
        self.scope = match self.commit_anchor {
            Some(a) => (a.min(self.commit_cursor), a.max(self.commit_cursor)),
            None => (self.commit_cursor, self.commit_cursor),
        };
        self.refresh(keep);
    }

    pub fn commit_move(&mut self, d: isize) {
        let n = self.commits.len() as isize;
        self.commit_cursor = (self.commit_cursor as isize + d).clamp(0, n - 1) as usize;
        self.apply_commit_cursor();
    }

    pub fn commit_all(&mut self) {
        let keep = self.current_file();
        self.scope = (0, self.commits.len() - 1);
        self.commit_anchor = None;
        if self.mode == Mode::Visual {
            self.mode = Mode::Normal;
        }
        self.refresh(keep);
        self.toast = Some("Scope: whole Range".into());
    }

    pub fn commit_visual_toggle(&mut self) {
        if self.commit_anchor.is_some() {
            self.commit_anchor = None;
            self.mode = Mode::Normal;
        } else {
            self.commit_anchor = Some(self.commit_cursor);
            self.scope_before = self.scope;
            self.mode = Mode::Visual;
            self.apply_commit_cursor();
        }
    }

    pub fn commit_visual_cancel(&mut self) {
        if self.commit_anchor.take().is_some() {
            let keep = self.current_file();
            self.scope = self.scope_before;
            self.mode = Mode::Normal;
            self.refresh(keep);
        }
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
            let pos = self.file_cursor;
            let was_visible_after = self.file_visible(fi);
            if self.variant == Variant::C && was_visible_after {
                // queue semantics: done means move on
                self.file_move(1);
                if self.file_cursor == pos {
                    self.refresh(Some(fi));
                }
            } else {
                self.refresh(if was_visible_after { Some(fi) } else { None });
            }
            self.toast = Some(format!("Reviewed: {path}"));
        }
    }

    fn toggle_filter(&mut self) {
        let keep = self.current_file();
        self.unreviewed_only = !self.unreviewed_only;
        self.refresh(keep);
        self.reset_diff_cursor(); // land on the first hunk of what is left
        self.toast = Some(format!("filter unreviewed-only: {}", if self.unreviewed_only { "on" } else { "off" }));
    }

    fn cycle_layout(&mut self) {
        self.dlayout = match self.dlayout {
            DiffLayout::Auto => DiffLayout::Side,
            DiffLayout::Side => DiffLayout::Inline,
            DiffLayout::Inline => DiffLayout::Auto,
        };
        self.toast = Some(match self.dlayout {
            DiffLayout::Auto => "diff layout: auto (side-by-side when wide)".into(),
            DiffLayout::Side => "diff layout: side-by-side".into(),
            DiffLayout::Inline => "diff layout: inline".into(),
        });
    }

    // ---------- search ----------

    fn search_exec(&mut self) {
        self.search = self.search_input.clone();
        self.search_hits.clear();
        self.search_idx = 0;
        let Some(fi) = self.current_file() else { return };
        let q = self.search.to_lowercase();
        if q.is_empty() {
            return;
        }
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
        self.diff_cursor = self.search_hits[self.search_idx];
    }

    // ---------- Comments / Prompt ----------

    fn open_editor(&mut self) {
        let Some(fi) = self.current_file() else { return };
        let (a, b) = match self.diff_anchor {
            Some(an) => (an.min(self.diff_cursor), an.max(self.diff_cursor)),
            None => (self.diff_cursor, self.diff_cursor),
        };
        self.diff_anchor = None;
        self.editor = Some(Editor { text: String::new(), insert: false, file: fi, a, b });
        self.mode = Mode::Comment;
    }

    fn save_editor(&mut self) {
        if let Some(ed) = self.editor.take() {
            let text = ed.text.trim().to_string();
            if text.is_empty() {
                self.toast = Some("empty Comment discarded".into());
            } else {
                let commit = self.commit_of_row(ed.file, ed.b);
                self.comments.push(Comment { file: ed.file, a: ed.a, b: ed.b, text, state: CState::Open, commit });
                self.toast = Some("Comment saved (open)".into());
            }
        }
        self.mode = Mode::Normal;
    }

    fn row_text(&self, fi: usize, ri: usize) -> String {
        let r = &self.files[fi].rows[ri];
        match r.kind {
            Kind::Del => r.old.clone(),
            _ => r.new.clone(),
        }
    }

    pub fn build_prompt(&self, idxs: &[usize]) -> String {
        let first = self.commits.first().unwrap().sha;
        let last = self.commits.last().unwrap().sha;
        let mut s = String::new();
        s.push_str(&format!(
            "You are the author of branch `feat/http-retry` (commits {first}..{last}).\nBelow are review comments from the reviewer. Answer every question and make the requested changes.\n"
        ));
        for (n, i) in idxs.iter().enumerate() {
            let c = &self.comments[*i];
            let f = &self.files[c.file];
            let la = f.rows[c.a].new_no.max(f.rows[c.a].old_no);
            let lb = f.rows[c.b].new_no.max(f.rows[c.b].old_no);
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
        s
    }

    pub fn open_comment_idxs(&self) -> Vec<usize> {
        (0..self.comments.len()).filter(|i| self.comments[*i].state == CState::Open).collect()
    }

    fn yank_prompt(&mut self) {
        let open = self.open_comment_idxs();
        if open.is_empty() {
            self.toast = Some("no open Comments: nothing to put in a Prompt".into());
            return;
        }
        let text = self.build_prompt(&open);
        for i in &open {
            self.comments[*i].state = CState::Sent;
        }
        self.prompt_no += 1;
        let closed = self.round;
        self.round += 1;
        self.toast = Some(format!(
            "Prompt #{} copied via OSC 52 ({} chars, {} Comments) - Round {} closed, now Round {}",
            self.prompt_no,
            text.chars().count(),
            open.len(),
            closed,
            self.round
        ));
        self.last_prompt = Some(text);
    }

    // ---------- panes (variant A) ----------

    pub fn go_commits(&mut self) {
        match self.variant {
            Variant::A => self.focus = Pane::Commits,
            _ => self.open_overlay(Overlay::Commits),
        }
    }

    pub fn go_files(&mut self) {
        match self.variant {
            Variant::A => self.focus = Pane::Files,
            _ => self.open_overlay(Overlay::Files),
        }
    }

    pub fn open_overlay(&mut self, o: Overlay) {
        self.overlay = Some(o);
        self.overlay_scroll = 0;
        match o {
            Overlay::Commits => {
                self.scope_before = self.scope;
                self.commit_anchor = None;
            }
            Overlay::Files => self.overlay_cursor = self.file_cursor,
            _ => {}
        }
    }

    fn close_overlay(&mut self, cancel: bool) {
        if self.overlay == Some(Overlay::Commits) {
            if cancel {
                let keep = self.current_file();
                self.scope = self.scope_before;
                self.refresh(keep);
            }
            self.commit_anchor = None;
            self.mode = Mode::Normal;
        }
        self.overlay = None;
    }

    fn pane_move(&mut self, c: char) {
        if self.variant != Variant::A {
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
        if self.variant != Variant::A {
            return;
        }
        self.focus = match self.focus {
            Pane::Commits => Pane::Files,
            Pane::Files => Pane::Diff,
            Pane::Diff => Pane::Commits,
        };
    }

    // ---------- commands ----------

    fn run_command(&mut self, line: &str) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["q"] | ["quit"] | ["q!"] => self.quit = true,
            ["variant", v] | ["var", v] => match *v {
                "a" | "A" => self.set_variant(Variant::A),
                "b" | "B" => self.set_variant(Variant::B),
                "c" | "C" => self.set_variant(Variant::C),
                _ => self.toast = Some("usage: :variant a|b|c".into()),
            },
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

    fn comment_key(&mut self, k: KeyEvent) {
        let insert = self.editor.as_ref().map_or(false, |e| e.insert);
        if insert {
            let ed = self.editor.as_mut().unwrap();
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
                KeyCode::Char('i') | KeyCode::Char('a') => {
                    if let Some(ed) = self.editor.as_mut() {
                        ed.insert = true;
                    }
                }
                KeyCode::Enter => self.save_editor(),
                KeyCode::Esc => {
                    self.editor = None;
                    self.mode = Mode::Normal;
                    self.toast = Some("Comment cancelled".into());
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
                KeyCode::Esc => self.close_overlay(true),
                KeyCode::Enter => self.close_overlay(false),
                KeyCode::Char('j') | KeyCode::Down => self.commit_move(1),
                KeyCode::Char('k') | KeyCode::Up => self.commit_move(-1),
                KeyCode::Char('v') => self.commit_visual_toggle(),
                KeyCode::Char('a') => self.commit_all(),
                _ => {}
            },
            Overlay::Files => {
                let n = self.visible_files().len();
                match k.code {
                    KeyCode::Esc | KeyCode::Char('q') => self.close_overlay(false),
                    KeyCode::Enter => {
                        let p = self.overlay_cursor;
                        self.close_overlay(false);
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
                KeyCode::Esc | KeyCode::Char('q') => self.close_overlay(false),
                KeyCode::Char('j') | KeyCode::Down => self.overlay_scroll += 1,
                KeyCode::Char('k') | KeyCode::Up => self.overlay_scroll = self.overlay_scroll.saturating_sub(1),
                KeyCode::Char('d') if ctrl => self.overlay_scroll += 10,
                KeyCode::Char('u') if ctrl => self.overlay_scroll = self.overlay_scroll.saturating_sub(10),
                KeyCode::Char('g') => self.overlay_scroll = 0,
                _ => {}
            },
        }
    }

    fn normal_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(p) = self.pending.take() {
            if let KeyCode::Char(c) = k.code {
                match (p, c) {
                    ('g', 'g') => self.go_top(),
                    ('g', 'c') => self.go_commits(),
                    ('g', 'f') => self.go_files(),
                    (']', 'h') => self.next_hunk(1),
                    ('[', 'h') => self.next_hunk(-1),
                    (']', 'f') => self.file_move(1),
                    ('[', 'f') => self.file_move(-1),
                    (']', 'c') => self.step_commit(1),
                    ('[', 'c') => self.step_commit(-1),
                    ('w', 'h' | 'j' | 'k' | 'l') => self.pane_move(c),
                    _ => {}
                }
            }
            return;
        }
        match k.code {
            KeyCode::F(1) => self.set_variant(Variant::A),
            KeyCode::F(2) => self.set_variant(Variant::B),
            KeyCode::F(3) => self.set_variant(Variant::C),
            KeyCode::Esc => self.escape(),
            KeyCode::Tab => self.cycle_focus(),
            KeyCode::Enter => match (self.variant, self.focus) {
                (Variant::A, Pane::Commits) => self.focus = Pane::Files,
                (Variant::A, Pane::Files) => self.focus = Pane::Diff,
                _ => {}
            },
            KeyCode::Down => self.mv(1),
            KeyCode::Up => self.mv(-1),
            KeyCode::Char(c) if ctrl => match c {
                'd' => self.page(1),
                'u' => self.page(-1),
                'w' => self.pending = Some('w'),
                'n' => self.search_next(1),
                'p' => self.search_next(-1),
                _ => {}
            },
            KeyCode::Char(c) => match c {
                'j' => self.mv(1),
                'k' => self.mv(-1),
                'h' | 'l' => self.pane_move(c),
                'g' | '[' | ']' => self.pending = Some(c),
                'G' => self.go_bottom(),
                'v' => self.visual_toggle(),
                ':' => {
                    self.mode = Mode::Command;
                    self.cmd.clear();
                }
                '/' => {
                    self.mode = Mode::Search;
                    self.search_input.clear();
                }
                'n' => {
                    if self.variant == Variant::C {
                        self.file_move(1)
                    } else {
                        self.search_next(1)
                    }
                }
                'p' => {
                    if self.variant == Variant::C {
                        self.file_move(-1)
                    }
                }
                'N' => self.search_next(-1),
                'm' => self.toggle_reviewed(),
                'u' => self.toggle_filter(),
                's' => self.cycle_layout(),
                'c' => {
                    if self.focus == Pane::Diff {
                        self.open_editor()
                    } else {
                        self.toast = Some("c comments a diff line: move focus to the diff first (l)".into())
                    }
                }
                'S' => self.open_overlay(Overlay::Summary),
                'Y' => self.yank_prompt(),
                '?' => self.open_overlay(Overlay::Help),
                'a' => {
                    if self.focus == Pane::Commits {
                        self.commit_all()
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn escape(&mut self) {
        if self.mode == Mode::Visual {
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
            Pane::Files => self.toast = Some("v selects commits (gc) or diff lines".into()),
        }
    }
}

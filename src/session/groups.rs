//! The Analyst's Groups inside the session: built in the background, shown in
//! the sidebar in place of the plain file list.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use super::*;
use crate::analyst::{Analyst, AnalystError, DiagramSubject, GroupRequest, Grouping};

type Key = (String, String);

type Answer = (Key, Result<Grouping, AnalystError>);

pub(super) struct Groups {
    analyst: Option<Arc<dyn Analyst>>,
    cache: HashMap<Key, Placed>,
    pending: HashSet<Key>,
    /// Why the last request for a key failed.
    errors: HashMap<Key, String>,
    /// The cached keys, oldest first.
    built: Vec<Key>,
    /// `Space g`: show the directory tree even when Groups are ready.
    tree: bool,
    sender: Sender<Answer>,
    receiver: Receiver<Answer>,
}

impl Default for Groups {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Groups {
            analyst: None,
            cache: HashMap::new(),
            pending: HashSet::new(),
            errors: HashMap::new(),
            built: Vec::new(),
            tree: false,
            sender,
            receiver,
        }
    }
}

/// Groups with the Group of every file looked up once, when they arrive.
struct Placed {
    grouping: Grouping,
    /// Group of each file: the first that lists it.
    index: HashMap<String, usize>,
}

impl Placed {
    fn new(grouping: Grouping) -> Placed {
        let mut index = HashMap::new();
        for (n, group) in grouping.groups.iter().enumerate() {
            for file in &group.files {
                index.entry(file.clone()).or_insert(n);
            }
        }
        Placed { grouping, index }
    }
}

/// A row of the sidebar's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Row {
    /// The name of Group `n` and its progress.
    Header(usize),
    /// The one-line description of Group `n`.
    Description(usize),
    /// File `n` of the displayed list.
    File(usize),
}

impl Session {
    /// Hand the Analyst over and start grouping at once.
    pub(super) fn start_grouping(&mut self, analyst: Arc<dyn Analyst>) {
        self.groups.analyst = Some(analyst);
        self.request_grouping(false);
    }

    /// Ask for the Groups of the current head and language unless there are
    /// some or they are on their way. `force` asks again.
    pub(super) fn request_grouping(&mut self, force: bool) {
        let Some(analyst) = self.groups.analyst.clone() else {
            return;
        };
        let key = self.current_key();
        if (!force && self.groups.cache.contains_key(&key))
            || !self.groups.pending.insert(key.clone())
        {
            return;
        }
        let request = GroupRequest {
            repo: self.repo.root().to_path_buf(),
            base: self.range.base.clone(),
            head: key.0.clone(),
            language: key.1.clone(),
            settings: self.analysis.settings.clone(),
            pull_request: self.pull_request.clone(),
            files: self.range_files.iter().map(|f| f.path.clone()).collect(),
        };
        let sender = self.groups.sender.clone();
        std::thread::spawn(move || {
            // A panic in the Analyst is an answer too, or the wait never ends.
            let answer =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| analyst.group(&request)))
                    .unwrap_or_else(|_| Err(AnalystError::Failed("the Analyst crashed".into())));
            let _ = sender.send((key, answer));
        });
    }

    fn take_grouping(&mut self, (key, answer): Answer) {
        self.groups.pending.remove(&key);
        match answer {
            Ok(grouping) => {
                self.groups.errors.remove(&key);
                self.groups.built.retain(|k| *k != key);
                self.groups.built.push(key.clone());
                self.groups.cache.insert(key, Placed::new(grouping));
                self.refilter();
            }
            Err(e) => {
                self.groups.errors.insert(key, e.to_string());
            }
        }
    }

    /// The status line, with the reason the Groups could not be built in
    /// front of it when that is the case.
    pub(super) fn groups_status(&self, rest: &str) -> String {
        match self.groups.errors.get(&self.current_key()) {
            Some(error) => format!("⚠ Groups: {error}  {rest}"),
            None => rest.to_string(),
        }
    }

    pub(super) fn collect_groupings(&mut self) {
        while let Ok(answer) = self.groups.receiver.try_recv() {
            self.take_grouping(answer);
        }
    }

    pub(super) fn wait_for_groupings(&mut self) {
        while !self.groups.pending.is_empty() {
            match self.groups.receiver.recv() {
                Ok(answer) => self.take_grouping(answer),
                Err(_) => break,
            }
        }
    }

    fn grouping(&self) -> Option<&Placed> {
        if self.groups.tree {
            return None;
        }
        self.groups_for_key().map(|(placed, _)| placed)
    }

    /// The Groups of the current head, or failing that the newest ones built
    /// for another head in this language: they are stale.
    fn groups_for_key(&self) -> Option<(&Placed, bool)> {
        let key = self.current_key();
        if let Some(grouping) = self.groups.cache.get(&key) {
            return Some((grouping, false));
        }
        let old = self
            .groups
            .built
            .iter()
            .rev()
            .find(|(_, language)| *language == key.1)?;
        Some((&self.groups.cache[old], true))
    }

    /// Note for the title of the sidebar when the Groups shown are out of date.
    pub(super) fn groups_note(&self) -> &'static str {
        match self.grouping() {
            Some(_) if self.groups_for_key().is_some_and(|(_, stale)| stale) => " · groups stale",
            _ => "",
        }
    }

    /// `Space g`: switch the sidebar between Groups and the directory tree.
    /// Returns whether the key was used.
    pub(super) fn groups_key(&mut self, key: KeyEvent) -> bool {
        if self.pending == Some(' ') && key.code == KeyCode::Char('g') {
            self.pending = None;
            self.count = None;
            self.groups.tree = !self.groups.tree;
            self.refilter();
            return true;
        }
        false
    }

    /// Which Group file `path` is in, if the sidebar shows Groups. A file in
    /// two Groups belongs to the first; one in none, or one the Analyst never
    /// saw, belongs to the Group after the last, "Other".
    fn group_of(&self, path: &str) -> Option<usize> {
        let placed = self.grouping()?;
        Some(
            placed
                .index
                .get(path)
                .copied()
                .unwrap_or(placed.grouping.groups.len()),
        )
    }

    /// The Group `path` is in, as the subject of a diagram.
    pub(super) fn group_subject(&self, path: &str) -> Option<DiagramSubject> {
        let n = self.group_of(path)?;
        let files = self
            .range_files
            .iter()
            .filter(|f| self.group_of(&f.path) == Some(n))
            .map(|f| f.path.clone())
            .collect();
        Some(DiagramSubject::Group {
            name: self.group_text(n).0,
            files,
        })
    }

    /// Name, description and Mechanical mark of Group `n`.
    fn group_text(&self, n: usize) -> (String, String, bool) {
        match self.grouping().and_then(|p| p.grouping.groups.get(n)) {
            Some(g) => (g.name.clone(), g.description.clone(), g.mechanical),
            None => (
                "Other".into(),
                "files the Analyst did not place".into(),
                false,
            ),
        }
    }

    /// The files in reading order when the sidebar shows Groups.
    pub(super) fn in_group_order(&self, mut files: Vec<ChangedFile>) -> Vec<ChangedFile> {
        if self.grouping().is_some() {
            files.sort_by_key(|f| self.group_of(&f.path).unwrap_or(usize::MAX));
        }
        files
    }

    /// The rows of the sidebar list.
    pub(super) fn sidebar_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut last = None;
        for (i, file) in self.files.iter().enumerate() {
            if self.grouping().is_some() {
                let group = self.group_of(&file.path);
                if last != Some(group) {
                    last = Some(group);
                    if let Some(g) = group {
                        rows.push(Row::Header(g));
                        rows.push(Row::Description(g));
                    }
                }
            }
            rows.push(Row::File(i));
        }
        rows
    }

    /// Keys of the focused file list: `j/k` and the arrows pick a file, `gg`/`G`
    /// and `Ctrl-d/u` jump, `Enter` opens it in the diff. Returns whether the key was used.
    pub(super) fn files_key(&mut self, key: KeyEvent) -> bool {
        let last = self.files.len().saturating_sub(1);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.pending == Some('g') && key.code == KeyCode::Char('g') && !ctrl {
            self.pending = None;
            let n = self.count.take().unwrap_or(1);
            self.select((n - 1).min(last));
            return true;
        }
        if ctrl && matches!(key.code, KeyCode::Char('d' | 'u')) && self.pending.is_none() {
            self.count = None;
            let half = (self.sidebar_capacity() / 2).max(1);
            if key.code == KeyCode::Char('d') {
                self.select(self.selected.saturating_add(half).min(last));
            } else {
                self.select(self.selected.saturating_sub(half));
            }
            return true;
        }
        if ctrl || self.pending.is_some() {
            return false;
        }
        match key.code {
            KeyCode::Char('G') => {
                let n = self.count.take();
                self.select(n.map_or(last, |n| n.saturating_sub(1).min(last)));
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let n = self.count.take().unwrap_or(1);
                self.select(self.selected.saturating_add(n).min(last));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let n = self.count.take().unwrap_or(1);
                self.select(self.selected.saturating_sub(n));
            }
            KeyCode::Enter => self.focus = Focus::Diff,
            _ => return false,
        }
        true
    }

    /// `]g` / `[g`: move `n` Groups on, to the first file of the Group.
    /// Without Groups the whole list is one.
    pub(super) fn jump_group(&mut self, forward: bool, n: usize) {
        let group = |i: usize| self.group_of(&self.files[i].path);
        let starts: Vec<usize> = (0..self.files.len())
            .filter(|&i| i == 0 || group(i) != group(i - 1))
            .collect();
        let Some(current) = starts.iter().rposition(|&s| s <= self.selected) else {
            return;
        };
        let target = if forward {
            (current + n).min(starts.len() - 1)
        } else {
            current.saturating_sub(n)
        };
        self.select(starts[target]);
    }

    /// `done/total` of the files of Group `n` in the Scope.
    fn group_progress(&self, n: usize) -> (usize, usize) {
        let files: Vec<_> = self
            .scope_files
            .iter()
            .filter(|f| self.group_of(&f.path) == Some(n))
            .collect();
        let done = files
            .iter()
            .filter(|f| self.review.state(&self.changes_of(&f.path, self.scope)) == Mark::Reviewed)
            .count();
        (done, files.len())
    }

    pub(super) fn render_group_row(
        &self,
        buf: &mut Buffer,
        x: u16,
        y: u16,
        width: usize,
        row: Row,
    ) {
        match row {
            Row::Header(n) => {
                let (done, total) = self.group_progress(n);
                let progress = format!("{done}/{total}");
                let (name, _, mechanical) = self.group_text(n);
                let tag = if mechanical { " [mech]" } else { "" };
                let name = fit(
                    &format!("{} {name}{tag}", n + 1),
                    width.saturating_sub(progress.len() + 2),
                    false,
                );
                let line = format!(" {name}");
                let line = format!(
                    "{}{progress} ",
                    pad(&line, width.saturating_sub(progress.len() + 1))
                );
                buf.set_stringn(
                    x,
                    y,
                    line,
                    width,
                    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
                );
            }
            Row::Description(n) => {
                let text = format!(" {}", self.group_text(n).1);
                buf.set_stringn(x, y, fit(&text, width, false), width, Style::new().fg(DIM));
            }
            Row::File(_) => {}
        }
    }
}

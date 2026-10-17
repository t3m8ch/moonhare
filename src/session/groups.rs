//! The Analyst's Groups inside the session: built in the background, shown in
//! the sidebar in place of the plain file list.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use super::analysis::Pending;
use super::*;
use crate::analyst::{Analyst, AnalystError, DiagramSubject, GroupRequest, Grouping};
use crate::analyst_cache::AnalystCache;

type Key = (String, String);

type Answer = (Key, Result<Grouping, AnalystError>);

pub(super) struct Groups {
    analyst: Option<Arc<dyn Analyst>>,
    cache: HashMap<Key, Placed>,
    pending: Pending<Key>,
    /// Keys whose Groups were read from the disk cache, not asked for.
    from_cache: HashSet<Key>,
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
            pending: Pending::default(),
            from_cache: HashSet::new(),
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
    /// Group of each file (the first that lists it) and its place in the
    /// reading order of that Group.
    index: HashMap<String, (usize, usize)>,
}

impl Placed {
    fn new(grouping: Grouping) -> Placed {
        let mut index = HashMap::new();
        for (n, group) in grouping.groups.iter().enumerate() {
            for (rank, file) in group.files.iter().enumerate() {
                index.entry(file.clone()).or_insert((n, rank));
            }
        }
        Placed { grouping, index }
    }
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
        let now = self.clock();
        if (!force && self.groups.cache.contains_key(&key))
            || !self.groups.pending.insert(key.clone(), now)
        {
            return;
        }
        let cache = self.analyst_cache.clone();
        let cache_key = AnalystCache::key("groups", &[&self.range.base, &key.0, &key.1]);
        if !force {
            if let Some(grouping) = cache.load_grouping(&cache_key) {
                self.groups.pending.remove(&key);
                self.groups.from_cache.insert(key.clone());
                self.keep_grouping(key, grouping);
                return;
            }
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
            if let Ok(grouping) = &answer {
                cache.store_grouping(&cache_key, grouping);
            }
            let _ = sender.send((key, answer));
        });
    }

    fn take_grouping(&mut self, (key, answer): Answer) {
        self.groups.pending.remove(&key);
        match answer {
            Ok(grouping) => {
                self.groups.from_cache.remove(&key);
                self.keep_grouping(key, grouping);
            }
            Err(e) => {
                self.groups.errors.insert(key, e.to_string());
            }
        }
    }

    fn keep_grouping(&mut self, key: Key, grouping: Grouping) {
        self.groups.errors.remove(&key);
        self.groups.built.retain(|k| *k != key);
        self.groups.built.push(key.clone());
        self.groups.cache.insert(key, Placed::new(grouping));
        self.refilter();
    }

    /// Whether the Groups of the current head and language came from the disk
    /// cache of an earlier run and have not been rebuilt since.
    pub fn groups_from_cache(&self) -> bool {
        self.groups.from_cache.contains(&self.current_key())
    }

    /// The status line, with the reason the Groups could not be built in
    /// front of it when that is the case.
    pub(super) fn groups_status(&self, rest: &str) -> String {
        match self.groups.errors.get(&self.current_key()) {
            Some(error) => format!("⚠ Groups: {error}  {rest}"),
            None => rest.to_string(),
        }
    }

    pub(super) fn groups_busy(&self) -> bool {
        !self.groups.pending.is_empty()
    }

    pub(super) fn groups_work(&self) -> Option<(usize, std::time::Duration)> {
        self.groups.pending.oldest()
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

    /// Whether the file panel shows Groups.
    pub(super) fn grouping_shown(&self) -> bool {
        self.grouping().is_some()
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

    /// Note for the title of the sidebar when the Groups shown are out of date
    /// or the Analyst is still building them.
    pub(super) fn groups_note(&self) -> String {
        let stale = match self.grouping() {
            Some(_) if self.groups_for_key().is_some_and(|(_, stale)| stale) => " · groups stale",
            _ => "",
        };
        match self.groups_busy() && !self.groups.tree {
            true => format!(
                "{stale} · {} grouping {}s",
                super::analysis::spinner(self.clock()),
                self.work_seconds("Groups")
            ),
            false if self.groups_from_cache() && self.grouping().is_some() => {
                format!("{stale} · groups cached")
            }
            false => stale.to_string(),
        }
    }

    /// `Space g`: switch the sidebar between Groups and one tree of all files;
    /// `Space i`: the description of the Group under the cursor.
    /// Returns whether the key was used.
    pub(super) fn groups_key(&mut self, key: KeyEvent) -> bool {
        if self.pending != Some(' ') {
            return false;
        }
        match key.code {
            KeyCode::Char('g') => {
                self.groups.tree = !self.groups.tree;
                self.refilter();
            }
            KeyCode::Char('i') => self.toggle_group_info(),
            _ => return false,
        }
        self.pending = None;
        self.count = None;
        true
    }

    /// Which Group file `path` is in, if the sidebar shows Groups. A file in
    /// two Groups belongs to the first; one in none, or one the Analyst never
    /// saw, belongs to the Group after the last, "Other".
    pub(super) fn group_of(&self, path: &str) -> Option<usize> {
        self.reading_place(path).map(|(group, _)| group)
    }

    /// The Group of `path` and its place in the reading order of the Group.
    fn reading_place(&self, path: &str) -> Option<(usize, usize)> {
        let placed = self.grouping()?;
        Some(
            placed
                .index
                .get(path)
                .copied()
                .unwrap_or((placed.grouping.groups.len(), usize::MAX)),
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
    pub(super) fn group_text(&self, n: usize) -> (String, String, bool) {
        match self.grouping().and_then(|p| p.grouping.groups.get(n)) {
            Some(g) => (g.name.clone(), g.description.clone(), g.mechanical),
            None => (
                "Other".into(),
                "files the Analyst did not place".into(),
                false,
            ),
        }
    }

    /// The files in the reading order of the Analyst when the sidebar shows
    /// Groups: Group by Group, and inside a Group as the Analyst listed them.
    pub(super) fn in_group_order(&self, mut files: Vec<ChangedFile>) -> Vec<ChangedFile> {
        if self.grouping().is_some() {
            files.sort_by_key(|f| self.reading_place(&f.path).unwrap_or((usize::MAX, 0)));
        }
        files
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

    /// The header row of Group `n`: its place in the reading order, name and
    /// progress.
    pub(super) fn render_group_row(&self, buf: &mut Buffer, x: u16, y: u16, width: usize, n: usize) {
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
}

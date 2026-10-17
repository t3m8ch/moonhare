//! The Analyst's Groups inside the session: built in the background, shown in
//! the sidebar in place of the plain file list.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use super::*;
use crate::analyst::{Analyst, AnalystError, GroupRequest, Grouping};

type Key = (String, String);

type Answer = (Key, Result<Grouping, AnalystError>);

pub(super) struct Groups {
    analyst: Option<Arc<dyn Analyst>>,
    cache: HashMap<Key, Grouping>,
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
            pull_request: self.pull_request.clone(),
            files: self.range_files.iter().map(|f| f.path.clone()).collect(),
        };
        let sender = self.groups.sender.clone();
        std::thread::spawn(move || {
            let answer = analyst.group(&request);
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
                self.groups.cache.insert(key, grouping);
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

    fn grouping(&self) -> Option<&Grouping> {
        if self.groups.tree {
            return None;
        }
        self.groups_for_key().map(|(grouping, _)| grouping)
    }

    /// The Groups of the current head, or failing that the newest ones built
    /// for another head in this language: they are stale.
    fn groups_for_key(&self) -> Option<(&Grouping, bool)> {
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
        let groups = &self.grouping()?.groups;
        Some(
            groups
                .iter()
                .position(|g| g.files.iter().any(|f| f == path))
                .unwrap_or(groups.len()),
        )
    }

    /// Name, description and Mechanical mark of Group `n`.
    fn group_text(&self, n: usize) -> (String, String, bool) {
        match self.grouping().and_then(|g| g.groups.get(n)) {
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

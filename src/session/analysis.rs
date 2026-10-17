//! The Analyst inside the session: the Summary built in the background, its
//! cache, and the `Space s` overlay.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use super::*;
use crate::analyst::{Analyst, AnalystError, Summary, SummaryRequest};

/// What a Summary is cached under: the head of the Range and the language.
type Key = (String, String);

type Answer = (Key, Result<Summary, AnalystError>);

pub(super) struct Analysis {
    analyst: Option<Arc<dyn Analyst>>,
    language: String,
    /// Head of the Range the Summary is built for.
    head: String,
    cache: HashMap<Key, Summary>,
    /// The cached keys, oldest first.
    built: Vec<Key>,
    /// The revision that names the head of the Range: it moves when the
    /// branch gets new commits.
    pub(super) head_rev: String,
    pending: HashSet<Key>,
    /// Why the last request for a key failed.
    errors: HashMap<Key, String>,
    sender: Sender<Answer>,
    receiver: Receiver<Answer>,
    /// The `Space s` overlay is open.
    overlay: bool,
}

impl Default for Analysis {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Analysis {
            analyst: None,
            language: DEFAULT_LANGUAGE.to_string(),
            head: String::new(),
            cache: HashMap::new(),
            built: Vec::new(),
            head_rev: "HEAD".to_string(),
            pending: HashSet::new(),
            errors: HashMap::new(),
            sender,
            receiver,
            overlay: false,
        }
    }
}

/// The language of the Summary unless the Reviewer picks another.
pub const DEFAULT_LANGUAGE: &str = "ru";

impl Session {
    /// Hand the session an Analyst; it starts building the Summary at once,
    /// in the background, while the Reviewer reads the diff.
    pub fn set_analyst(&mut self, analyst: Arc<dyn Analyst>) {
        self.analysis.analyst = Some(analyst);
        self.analysis.head = self.range.head.clone();
        self.request_summary(false);
    }

    /// Set the language of the Summary: a code such as `ru` or `en`. The
    /// Summary in that language is built unless it is already cached.
    pub fn set_language(&mut self, language: &str) {
        self.analysis.language = language.to_string();
        self.request_summary(false);
    }

    /// Where the head of the Range is now: new commits move it.
    pub(super) fn refresh_head(&mut self) {
        if let Ok(head) = self.repo.commit_id(&self.analysis.head_rev) {
            self.analysis.head = head;
        }
    }

    /// `:analyze`. Returns whether `command` was one of the Analyst's.
    pub(super) fn analysis_command(&mut self, command: &str) -> bool {
        let mut words = command.split_whitespace();
        match (words.next(), words.next(), words.next()) {
            (Some("analyze"), None, _) => {
                self.refresh_head();
                self.request_summary(true);
            }
            (Some("lang"), None, _) => {
                self.message = Some(format!("Summary language: {}", self.analysis.language));
            }
            (Some("lang"), Some(language), None) => {
                self.refresh_head();
                self.set_language(language);
            }
            (Some("lang"), _, _) => self.message = Some("Usage: :lang <code>, e.g. ru, en".into()),
            _ => return false,
        }
        true
    }

    /// Ask for the Summary of the current head and language unless there is
    /// one or it is on its way. `force` asks again even for a cached one.
    fn request_summary(&mut self, force: bool) {
        let Some(analyst) = self.analysis.analyst.clone() else {
            return;
        };
        let key = self.current_key();
        if (!force && self.analysis.cache.contains_key(&key))
            || !self.analysis.pending.insert(key.clone())
        {
            return;
        }
        self.analysis.errors.remove(&key);
        let request = SummaryRequest {
            repo: self.repo.root().to_path_buf(),
            base: self.range.base.clone(),
            head: key.0.clone(),
            language: key.1.clone(),
            pull_request: self.pull_request.clone(),
        };
        let sender = self.analysis.sender.clone();
        std::thread::spawn(move || {
            let answer = analyst.summarize(&request);
            let _ = sender.send((key, answer));
        });
    }

    fn take_answer(&mut self, (key, answer): Answer) {
        self.analysis.pending.remove(&key);
        match answer {
            Ok(summary) => {
                self.analysis.errors.remove(&key);
                self.analysis.built.retain(|k| *k != key);
                self.analysis.built.push(key.clone());
                self.analysis.cache.insert(key, summary);
            }
            Err(e) => {
                self.analysis.errors.insert(key, e.to_string());
            }
        }
    }

    /// The Analyst's state for the status line: a failure comes first so it
    /// is not cut off, a Summary in progress is a short note at the end.
    pub(super) fn analysis_status(&self, rest: &str) -> String {
        match self.analysis.errors.get(&self.current_key()) {
            Some(error) => format!("⚠ Analyst: {error}  {rest}"),
            None => rest.to_string(),
        }
    }

    /// Collect the answers that arrived. Called by the event loop.
    pub fn tick(&mut self) {
        while let Ok(answer) = self.analysis.receiver.try_recv() {
            self.take_answer(answer);
        }
    }

    /// Whether the Analyst still owes an answer.
    pub fn analyst_busy(&self) -> bool {
        !self.analysis.pending.is_empty()
    }

    /// Block until every request to the Analyst has been answered.
    pub fn wait_for_analyst(&mut self) {
        while !self.analysis.pending.is_empty() {
            match self.analysis.receiver.recv() {
                Ok(answer) => self.take_answer(answer),
                Err(_) => break,
            }
        }
    }

    /// Keys of the Summary overlay and the one that opens it; returns whether
    /// the key was used.
    pub(super) fn analysis_key(&mut self, key: KeyEvent) -> bool {
        if self.analysis.overlay {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                self.analysis.overlay = false;
            }
            return true;
        }
        if self.pending == Some(' ') && key.code == KeyCode::Char('s') {
            self.pending = None;
            self.count = None;
            self.refresh_head();
            self.analysis.overlay = true;
            return true;
        }
        false
    }

    /// The Summary the overlay shows and the head it was built for when that
    /// is not the current head (a stale one, from before new commits).
    fn current_summary(&self) -> Option<(&Summary, Option<&str>)> {
        let key = self.current_key();
        if let Some(summary) = self.analysis.cache.get(&key) {
            return Some((summary, None));
        }
        let stale = self
            .analysis
            .built
            .iter()
            .rev()
            .find(|(head, language)| *language == key.1 && *head != key.0)?;
        Some((&self.analysis.cache[stale], Some(stale.0.as_str())))
    }

    fn current_key(&self) -> Key {
        (self.analysis.head.clone(), self.analysis.language.clone())
    }

    pub(super) fn render_summary(&self, buf: &mut Buffer) {
        if !self.analysis.overlay {
            return;
        }
        let width = self.size.0.saturating_sub(4).min(100);
        let height = self.size.1.saturating_sub(4);
        let area = Rect::new(
            (self.size.0 - width) / 2,
            (self.size.1 - height) / 2,
            width,
            height,
        );
        for y in area.y..area.y + area.height {
            buf.set_string(area.x, y, " ".repeat(width as usize), Style::new().fg(FG));
        }
        let language = &self.analysis.language;
        let shown = self.current_summary();
        let title = match shown {
            Some((_, Some(built_for))) => {
                let short = &built_for[..7.min(built_for.len())];
                format!(" Summary [{language}] stale: built for {short}, :analyze to update ")
            }
            _ => format!(" Summary [{language}] "),
        };
        draw_box(buf, area, &title);
        let inner_w = width.saturating_sub(4) as usize;
        let text = match shown {
            Some((summary, _)) => summary.text.clone(),
            None if self.analysis.analyst.is_none() => {
                "No Analyst is configured: there is no Summary.".to_string()
            }
            None if self.analysis.errors.contains_key(&self.current_key()) => {
                format!("Analyst: {}", self.analysis.errors[&self.current_key()])
            }
            None if self.analysis.pending.contains(&self.current_key()) => {
                "The Summary is being built…".to_string()
            }
            None => "There is no Summary yet. :analyze builds it.".to_string(),
        };
        for (row, line) in wrap(&text, inner_w)
            .iter()
            .take(height.saturating_sub(2) as usize)
            .enumerate()
        {
            buf.set_stringn(
                area.x + 2,
                area.y + 1 + row as u16,
                line,
                inner_w,
                Style::new().fg(FG),
            );
        }
    }
}

/// Break `text` into lines of at most `width` characters, at spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let needed =
                line.chars().count() + word.chars().count() + usize::from(!line.is_empty());
            if needed > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}

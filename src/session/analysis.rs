//! The Analyst inside the session: the Summary built in the background, its
//! cache, and the `Space s` overlay.

use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use ratatui::text::Line;

use super::diagram::clear_area;
use super::*;
use crate::analyst::{
    Analyst, AnalystError, AnalystSettings, Summary, SummaryRequest, EFFORT_LEVELS,
};
use crate::analyst_cache::AnalystCache;

/// What a Summary is cached under: the head of the Range and the language.
type Key = (String, String);

type Answer = (Key, Result<Summary, AnalystError>);

/// Requests that have not been answered, and when each was sent.
pub(super) struct Pending<K>(HashMap<K, Duration>);

impl<K: Hash + Eq> Pending<K> {
    /// Note a request sent at `now`; false when the key is already on its way.
    pub(super) fn insert(&mut self, key: K, now: Duration) -> bool {
        match self.0.entry(key) {
            std::collections::hash_map::Entry::Occupied(_) => false,
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(now);
                true
            }
        }
    }

    pub(super) fn remove(&mut self, key: &K) {
        self.0.remove(key);
    }

    pub(super) fn contains(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many requests are on their way and when the oldest was sent.
    pub(super) fn oldest(&self) -> Option<(usize, Duration)> {
        Some((self.0.len(), *self.0.values().min()?))
    }
}

impl<K> Default for Pending<K> {
    fn default() -> Self {
        Pending(HashMap::new())
    }
}

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// The spinner's frame for the time since the session opened.
pub(super) fn spinner(clock: Duration) -> char {
    SPINNER[(clock.as_millis() / 100) as usize % SPINNER.len()]
}

pub(super) struct Analysis {
    pub(super) analyst: Option<Arc<dyn Analyst>>,
    pub(super) language: String,
    /// Effort and model for the requests to come.
    pub(super) settings: AnalystSettings,
    /// Head of the Range the Summary is built for.
    head: String,
    cache: HashMap<Key, Summary>,
    /// The cached keys, oldest first.
    built: Vec<Key>,
    /// The revision that names the head of the Range: it moves when the
    /// branch gets new commits.
    pub(super) head_rev: String,
    pending: Pending<Key>,
    /// Keys whose Summary was read from the disk cache, not asked for.
    from_cache: HashSet<Key>,
    /// Why the last request for a key failed.
    errors: HashMap<Key, String>,
    sender: Sender<Answer>,
    receiver: Receiver<Answer>,
    /// The `Space s` overlay is open.
    overlay: bool,
    /// How far the text of the overlay is scrolled.
    scroll: super::scroll::Scroll,
}

impl Default for Analysis {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Analysis {
            analyst: None,
            language: DEFAULT_LANGUAGE.to_string(),
            settings: AnalystSettings::default(),
            head: String::new(),
            cache: HashMap::new(),
            built: Vec::new(),
            head_rev: "HEAD".to_string(),
            pending: Pending::default(),
            from_cache: HashSet::new(),
            errors: HashMap::new(),
            sender,
            receiver,
            overlay: false,
            scroll: Default::default(),
        }
    }
}

/// The language of the Summary unless the Reviewer picks another.
pub const DEFAULT_LANGUAGE: &str = "ru";

impl Session {
    pub(super) fn summary_open(&self) -> bool {
        self.analysis.overlay
    }

    /// Hand the session an Analyst; it starts building the Summary at once,
    /// in the background, while the Reviewer reads the diff.
    pub fn set_analyst(&mut self, analyst: Arc<dyn Analyst>) {
        self.analysis.analyst = Some(analyst.clone());
        self.analysis.head = self.range.head.clone();
        // The pull request is context for the questions: wait for it.
        if !self.forge_busy() {
            self.start_analysis(analyst);
        }
    }

    pub(super) fn start_analysis(&mut self, analyst: Arc<dyn Analyst>) {
        self.request_summary(false);
        self.start_grouping(analyst);
    }

    /// Set the language of the Summary: a code such as `ru` or `en`. The
    /// Summary in that language is built unless it is already cached.
    pub fn set_language(&mut self, language: &str) {
        self.analysis.language = language.to_string();
        self.request_summary(false);
        self.request_grouping(false);
        self.refilter();
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
            (Some("analyze"), None, _) => self.reanalyze(),
            (Some("analyze"), Some("all"), None) => {
                if self.analyst_cache.is_empty() {
                    self.forget_analyst();
                } else {
                    self.confirm = Some(Confirm::ForgetAnalystCache);
                    self.message =
                        Some("Forget every cached Analyst answer of this Review? y / n".into());
                }
            }
            (Some("analyze"), _, _) => self.message = Some("Usage: :analyze [all]".into()),
            (Some("lang"), None, _) => {
                self.message = Some(format!("Summary language: {}", self.analysis.language));
            }
            (Some("lang"), Some(language), None) => {
                self.refresh_head();
                self.set_language(language);
            }
            (Some("effort"), arg, None) => self.effort_command(arg),
            (Some("model"), arg, None) => self.model_command(arg),
            (Some("effort" | "model"), _, _) => {
                self.message = Some("Usage: :effort <level|default>, :model <name|default>".into())
            }
            (Some("lang"), _, _) => self.message = Some("Usage: :lang <code>, e.g. ru, en".into()),
            _ => return false,
        }
        true
    }

    /// Build the Summary and the Groups again, whatever is cached.
    fn reanalyze(&mut self) {
        self.refresh_head();
        self.request_summary(true);
        self.request_grouping(true);
    }

    /// `y` forgets the Analyst's cache, anything else keeps it.
    pub(super) fn confirm_forget_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('y') {
            self.forget_analyst();
        }
    }

    /// `:analyze all`: forget every answer of this Review, on disk and in
    /// memory, and build the Summary and the Groups at once; Explanations
    /// and Diagrams are asked for again when next opened. An answer already
    /// on its way is a fresh one and is still kept when it comes.
    fn forget_analyst(&mut self) {
        if let Err(e) = self.analyst_cache.clear() {
            self.message = Some(e);
            return;
        }
        self.forget_diagrams();
        self.reanalyze();
        self.message = Some(if self.analysis.analyst.is_some() {
            "The Analyst's cache is forgotten: asking again".into()
        } else {
            "The Analyst's cache is forgotten (no Analyst is configured)".into()
        });
    }

    /// Set the reasoning effort of the requests to come: one of
    /// [`EFFORT_LEVELS`]. The settings do not touch a request already running.
    pub fn set_effort(&mut self, level: &str) -> Result<(), String> {
        if level == "default" {
            self.analysis.settings.effort = None;
        } else if EFFORT_LEVELS.contains(&level) {
            self.analysis.settings.effort = Some(level.to_string());
        } else {
            return Err(format!(
                "Unknown effort \"{level}\": use {} or default",
                EFFORT_LEVELS.join(", ")
            ));
        }
        Ok(())
    }

    /// Set the model of the requests to come: an alias or a full name.
    pub fn set_model(&mut self, model: &str) -> Result<(), String> {
        if model == "default" {
            self.analysis.settings.model = None;
        } else if model.is_empty() || model.starts_with('-') {
            return Err(format!("Not a model name: \"{model}\""));
        } else {
            self.analysis.settings.model = Some(model.to_string());
        }
        Ok(())
    }

    fn effort_command(&mut self, arg: Option<&str>) {
        self.message = Some(match arg {
            None => format!(
                "Effort: {}",
                self.analysis
                    .settings
                    .effort
                    .as_deref()
                    .unwrap_or("default")
            ),
            Some(level) => match self.set_effort(level) {
                Ok(()) => format!("Effort: {level} (from the next request)"),
                Err(e) => e,
            },
        });
    }

    fn model_command(&mut self, arg: Option<&str>) {
        self.message = Some(match arg {
            None => format!(
                "Model: {}",
                self.analysis.settings.model.as_deref().unwrap_or("default")
            ),
            Some(model) => match self.set_model(model) {
                Ok(()) => format!("Model: {model} (from the next request)"),
                Err(e) => e,
            },
        });
    }

    /// The settings that are not the defaults, for the status line.
    pub(super) fn settings_status(&self) -> String {
        let settings = &self.analysis.settings;
        let mut text = String::new();
        if let Some(effort) = &settings.effort {
            text.push_str(&format!("  effort:{effort}"));
        }
        if let Some(model) = &settings.model {
            text.push_str(&format!("  model:{model}"));
        }
        text.push_str(&self.theme_status());
        text
    }

    /// Ask for the Summary of the current head and language unless there is
    /// one or it is on its way. `force` asks again even for a cached one.
    fn request_summary(&mut self, force: bool) {
        let Some(analyst) = self.analysis.analyst.clone() else {
            return;
        };
        let key = self.current_key();
        let now = self.clock();
        if (!force && self.analysis.cache.contains_key(&key))
            || !self.analysis.pending.insert(key.clone(), now)
        {
            return;
        }
        self.analysis.errors.remove(&key);
        let cache = self.analyst_cache.clone();
        let cache_key = self.summary_cache_key(&key);
        if !force {
            if let Some(summary) = cache.load_summary(&cache_key) {
                self.analysis.pending.remove(&key);
                self.analysis.from_cache.insert(key.clone());
                self.keep_summary(key, summary);
                return;
            }
        }
        let request = SummaryRequest {
            repo: self.repo.root().to_path_buf(),
            base: self.range.base.clone(),
            head: key.0.clone(),
            language: key.1.clone(),
            settings: self.analysis.settings.clone(),
            pull_request: self.pull_request.clone(),
        };
        let sender = self.analysis.sender.clone();
        std::thread::spawn(move || {
            let answer = analyst.summarize(&request);
            if let Ok(summary) = &answer {
                cache.store_summary(&cache_key, summary);
            }
            let _ = sender.send((key, answer));
        });
    }

    fn take_answer(&mut self, (key, answer): Answer) {
        self.analysis.pending.remove(&key);
        match answer {
            Ok(summary) => {
                self.analysis.from_cache.remove(&key);
                self.keep_summary(key, summary);
            }
            Err(e) => {
                self.analysis.errors.insert(key, e.to_string());
            }
        }
    }

    fn keep_summary(&mut self, key: Key, summary: Summary) {
        self.analysis.errors.remove(&key);
        self.analysis.built.retain(|k| *k != key);
        self.analysis.built.push(key.clone());
        if let Some(mermaid) = &summary.diagram {
            self.remember_drawing(mermaid);
        }
        for mermaid in crate::markdown::fenced_blocks(&summary.text, "mermaid") {
            self.remember_drawing(&mermaid);
        }
        self.analysis.cache.insert(key, summary);
    }

    /// Where a Summary for `key` lives on disk. It depends on the Range and
    /// the language, not on effort or model.
    fn summary_cache_key(&self, (head, language): &Key) -> String {
        AnalystCache::key("summary", &[&self.range.base, head, language])
    }

    /// Whether the Summary of the current head and language came from the
    /// disk cache of an earlier run and has not been rebuilt since.
    pub fn summary_from_cache(&self) -> bool {
        self.analysis.from_cache.contains(&self.current_key())
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
        self.forge_tick();
        self.take_diffs();
        self.diagram_tick();
        while let Ok(answer) = self.analysis.receiver.try_recv() {
            self.take_answer(answer);
        }
        self.collect_groupings();
        self.take_explanations();
    }

    /// Whether the Analyst still owes an answer.
    pub fn analyst_busy(&self) -> bool {
        !self.analysis.pending.is_empty()
            || self.groups_busy()
            || self.diagrams_busy()
            || self.explanations_busy()
    }

    /// What the Analyst is working on: the kind of request, how many are on
    /// their way and the age of the oldest.
    fn analyst_work(&self) -> Vec<(&'static str, usize, Duration)> {
        [
            ("Summary", self.analysis.pending.oldest()),
            ("Groups", self.groups_work()),
            ("Explanation", self.explanations_work()),
            ("Diagram", self.diagrams_work()),
        ]
        .into_iter()
        .filter_map(|(what, work)| {
            work.map(|(n, since)| (what, n, self.clock().saturating_sub(since)))
        })
        .collect()
    }

    /// The status line with the Analyst's work in front of it: a spinner and
    /// what is being built, for as long as it takes.
    pub(super) fn busy_status(&self, rest: &str) -> String {
        let work = self.analyst_work();
        if work.is_empty() {
            return rest.to_string();
        }
        let list: Vec<String> = work
            .iter()
            .map(|(what, n, age)| match n {
                1 => format!("{what} {}s", age.as_secs()),
                n => format!("{what} ×{n} {}s", age.as_secs()),
            })
            .collect();
        format!(
            "{} Analyst: {}  {rest}",
            spinner(self.clock()),
            list.join(", ")
        )
    }

    /// Seconds the oldest request of the Analyst's work on `what` has taken.
    pub(super) fn work_seconds(&self, what: &str) -> u64 {
        self.analyst_work()
            .iter()
            .find(|(w, ..)| *w == what)
            .map_or(0, |(_, _, age)| age.as_secs())
    }

    /// Block until every request to the Analyst has been answered.
    pub fn wait_for_analyst(&mut self) {
        self.wait_for_forge();
        self.diagram_wait();
        while !self.analysis.pending.is_empty() {
            match self.analysis.receiver.recv() {
                Ok(answer) => self.take_answer(answer),
                Err(_) => break,
            }
        }
        self.wait_for_groupings();
        self.wait_for_explanations();
    }

    /// Keys of the Summary overlay and the one that opens it; returns whether
    /// the key was used.
    pub(super) fn analysis_key(&mut self, key: KeyEvent) -> bool {
        if self.analysis.overlay {
            let area = self.overlay_area();
            let (total, rows) = (self.summary_view().1.len(), scroll::text_rows(area));
            let mut scroll = self.analysis.scroll;
            if scroll.key(key, total, rows) {
                self.analysis.scroll = scroll;
            } else if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                self.analysis.overlay = false;
            } else if key.code == KeyCode::Char('y') {
                let source = self.summary_source().unwrap_or_default();
                self.copy_to_clipboard(&source, "the Summary");
            } else {
                self.summary_diagram_key(key);
            }
            return true;
        }
        if self.pending == Some(' ') && key.code == KeyCode::Char('s') {
            self.pending = None;
            self.count = None;
            self.refresh_head();
            self.analysis.overlay = true;
            self.analysis.scroll = Default::default();
            self.reset_picture_scroll();
            return true;
        }
        false
    }

    /// The Summary the overlay shows and the head it was built for when that
    /// is not the current head (a stale one, from before new commits).
    pub(super) fn current_summary(&self) -> Option<(&Summary, Option<&str>)> {
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

    pub(super) fn analyst_handle(&self) -> Option<Arc<dyn Analyst>> {
        self.analysis.analyst.clone()
    }

    pub(super) fn language_code(&self) -> String {
        self.analysis.language.clone()
    }

    /// Where the overlays are drawn: centred, leaving a margin.
    pub(super) fn overlay_area(&self) -> Rect {
        let width = self.size.0.saturating_sub(4).min(100);
        let height = self.size.1.saturating_sub(4);
        Rect::new(
            (self.size.0 - width) / 2,
            (self.size.1 - height) / 2,
            width,
            height,
        )
    }

    pub(super) fn current_key(&self) -> Key {
        (self.analysis.head.clone(), self.analysis.language.clone())
    }

    /// The Markdown of the Summary shown, with its diagram as a Mermaid block.
    fn summary_source(&self) -> Option<String> {
        let (summary, _) = self.current_summary()?;
        Some(match &summary.diagram {
            Some(mermaid) => format!(
                "{}\n\n```mermaid\n{}\n```\n",
                summary.text.trim_end(),
                mermaid.trim_end()
            ),
            None => summary.text.clone(),
        })
    }

    /// The title of the Summary overlay and its text, laid out for the box.
    pub(super) fn summary_view(&self) -> (String, Vec<Line<'static>>) {
        let inner_w = scroll::text_cols(self.overlay_area());
        let language = &self.analysis.language;
        let shown = self.current_summary();
        let title = match shown {
            Some((_, Some(built_for))) => {
                let short = &built_for[..7.min(built_for.len())];
                format!(" Summary [{language}] stale: built for {short}, :analyze to update ")
            }
            _ if self.summary_from_cache() => format!(" Summary [{language}] cached "),
            _ => format!(" Summary [{language}] "),
        };
        let text = match shown {
            Some((summary, _)) => {
                return (title, self.summary_lines(summary, inner_w));
            }
            None if self.analysis.analyst.is_none() => {
                "No Analyst is configured: there is no Summary.".to_string()
            }
            None if self.analysis.errors.contains_key(&self.current_key()) => {
                format!("Analyst: {}", self.analysis.errors[&self.current_key()])
            }
            None if self.analysis.pending.contains(&self.current_key()) => {
                format!(
                    "{} The Analyst is working: the Summary is being built… {}s",
                    spinner(self.clock()),
                    self.work_seconds("Summary")
                )
            }
            None => "There is no Summary yet. :analyze builds it.".to_string(),
        };
        (title, plain_lines(&text, inner_w))
    }

    /// The Summary drawn from its Markdown, Mermaid blocks as diagrams, and
    /// the diagram the Analyst attached under it.
    fn summary_lines(&self, summary: &Summary, width: usize) -> Vec<Line<'static>> {
        let pictures = |language: &str, code: &str| {
            (language == "mermaid" && self.has_drawing(code)).then(|| self.drawing_lines(code))
        };
        let mut lines = crate::markdown::render_with(self.theme(), &summary.text, width, &pictures);
        if let Some(mermaid) = &summary.diagram {
            lines.push(Line::default());
            lines.extend(self.drawing_lines(mermaid).into_iter().map(Line::from));
        }
        lines
    }

    pub(super) fn summary_scroll_mut(&mut self) -> Option<&mut scroll::Scroll> {
        self.analysis.overlay.then_some(&mut self.analysis.scroll)
    }

    pub(super) fn render_summary(&self, buf: &mut Buffer) {
        if !self.analysis.overlay {
            return;
        }
        let area = self.overlay_area();
        clear_area(self.theme(), buf, area);
        let (title, lines) = self.summary_view();
        draw_box(self.theme(), buf, area, &title);
        scroll::render_styled(self.theme(), buf, area, &lines, &self.analysis.scroll);
    }
}

/// A message that is not Markdown, wrapped to `width`.
pub(super) fn plain_lines(text: &str, width: usize) -> Vec<Line<'static>> {
    wrap(text, width).into_iter().map(Line::from).collect()
}

/// Break `text` into lines of at most `width` characters, at spaces; a word
/// wider than that is cut.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for mut word in paragraph.split_whitespace() {
            loop {
                let len = word.chars().count();
                let needed = line.chars().count() + len + usize::from(!line.is_empty());
                if needed <= width {
                    if !line.is_empty() {
                        line.push(' ');
                    }
                    line.push_str(word);
                    break;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                    continue;
                }
                let cut = word
                    .char_indices()
                    .nth(width)
                    .map_or(word.len(), |(i, _)| i);
                lines.push(word[..cut].to_string());
                word = &word[cut..];
            }
        }
        lines.push(line);
    }
    lines
}

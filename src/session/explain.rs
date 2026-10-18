//! "Explain this": the Analyst explains the hunk or the file under the cursor
//! on demand. `Space x` asks about the hunk, `Space X` about the file; the
//! answer opens in an overlay, `Esc` closes it or cancels the wait.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use ratatui::text::Line;

use super::analysis::{plain_lines, Pending};
use super::*;
use crate::analyst::{Analyst, AnalystError, ExplainRequest, ExplainTarget, Explanation};
use crate::analyst_cache::AnalystCache;

type Answer = (u64, Result<Explanation, AnalystError>);

enum State {
    Waiting,
    Done(String),
    Failed(String),
}

struct Overlay {
    /// The request this overlay waits for; answers to others are dropped.
    id: u64,
    title: String,
    /// The answer shown was read from the disk cache.
    cached: bool,
    /// `r` asked again: the answer shown stays until the new one comes.
    refreshing: bool,
    /// What was asked and where its answer is cached, for `r`.
    request: ExplainRequest,
    cache_key: String,
    state: State,
    scroll: scroll::Scroll,
}

pub(super) struct Explain {
    overlay: Option<Overlay>,
    next_id: u64,
    /// Requests whose threads have not answered yet, cancelled or not.
    in_flight: Pending<u64>,
    /// Why the last request failed, until the overlay closes.
    error: Option<String>,
    sender: Sender<Answer>,
    receiver: Receiver<Answer>,
}

impl Default for Explain {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Explain {
            overlay: None,
            next_id: 0,
            in_flight: Pending::default(),
            error: None,
            sender,
            receiver,
        }
    }
}

impl Session {
    pub(super) fn explain_open(&self) -> bool {
        self.explain.overlay.is_some()
    }

    /// Keys of the Explain overlay and the ones that open it; returns whether
    /// the key was used.
    pub(super) fn explain_key(&mut self, key: KeyEvent) -> bool {
        if let Some(mut scroll) = self.explain.overlay.as_ref().map(|o| o.scroll) {
            let (total, rows) = (
                self.explain_lines().len(),
                scroll::text_rows(self.overlay_area()),
            );
            if scroll.key(key, total, rows) {
                if let Some(overlay) = &mut self.explain.overlay {
                    overlay.scroll = scroll;
                }
            } else if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                self.explain.overlay = None;
                self.explain.error = None;
            } else if key.code == KeyCode::Char('y') {
                if let Some(State::Done(text)) = self.explain.overlay.as_ref().map(|o| &o.state) {
                    let text = text.clone();
                    self.copy_to_clipboard(&text, "the explanation");
                }
            } else if key.code == KeyCode::Char('r') {
                self.refresh_explanation();
            }
            return true;
        }
        if self.pending != Some(' ') {
            return false;
        }
        let file = match key.code {
            KeyCode::Char('x') => false,
            KeyCode::Char('X') => true,
            _ => return false,
        };
        self.pending = None;
        self.count = None;
        self.request_explanation(file);
        true
    }

    fn request_explanation(&mut self, whole_file: bool) {
        let Some(analyst) = self.analysis.analyst.clone() else {
            self.message = Some("No Analyst is configured: nothing to explain with".into());
            return;
        };
        let Some(path) = self.files.get(self.selected).map(|f| f.path.clone()) else {
            return;
        };
        let (target, what) = if whole_file {
            (ExplainTarget::File { path: path.clone() }, "file")
        } else {
            match self.hunk_under_cursor() {
                Some(text) => (
                    ExplainTarget::Hunk {
                        path: path.clone(),
                        text,
                    },
                    "hunk",
                ),
                None => {
                    self.message = Some("No hunk under the cursor to explain".into());
                    return;
                }
            }
        };
        let (base, head) = self.scope_ends();
        let request = ExplainRequest {
            repo: self.repo.root().to_path_buf(),
            base,
            head,
            language: self.analysis.language.clone(),
            settings: self.analysis.settings.clone(),
            pull_request: self.pull_request.clone(),
            target,
        };
        let title = format!(" Explain {what}: {path} [{}] ", request.language);
        let cache_key = self.explanation_cache_key(&request);
        self.explain.error = None;
        if let Some(explanation) = self.analyst_cache.load_explanation(&cache_key) {
            self.explain.overlay = Some(Overlay {
                id: self.next_explain_id(),
                title,
                cached: true,
                refreshing: false,
                request,
                cache_key,
                state: State::Done(explanation.text),
                scroll: Default::default(),
            });
            return;
        }
        let id = self.ask_explanation(analyst, &request, &cache_key);
        self.explain.overlay = Some(Overlay {
            id,
            title,
            cached: false,
            refreshing: false,
            request,
            cache_key,
            state: State::Waiting,
            scroll: Default::default(),
        });
    }

    fn next_explain_id(&mut self) -> u64 {
        self.explain.next_id += 1;
        self.explain.next_id
    }

    /// Send `request` to the Analyst; its answer is cached under `cache_key`
    /// and comes back under the returned id.
    fn ask_explanation(
        &mut self,
        analyst: Arc<dyn Analyst>,
        request: &ExplainRequest,
        cache_key: &str,
    ) -> u64 {
        let id = self.next_explain_id();
        let now = self.clock();
        self.explain.in_flight.insert(id, now);
        let (request, cache_key) = (request.clone(), cache_key.to_string());
        let cache = self.analyst_cache.clone();
        let sender = self.explain.sender.clone();
        std::thread::spawn(move || {
            let answer = analyst.explain(&request);
            if let Ok(explanation) = &answer {
                cache.store_explanation(&cache_key, explanation);
            }
            let _ = sender.send((id, answer));
        });
        id
    }

    /// `r` in the overlay: ask again whatever is cached, keeping the answer
    /// on screen until the new one comes.
    fn refresh_explanation(&mut self) {
        let Some(analyst) = self.analysis.analyst.clone() else {
            return;
        };
        let Some(overlay) = &self.explain.overlay else {
            return;
        };
        if matches!(overlay.state, State::Waiting) || overlay.refreshing {
            return;
        }
        let (request, cache_key) = (overlay.request.clone(), overlay.cache_key.clone());
        self.explain.error = None;
        let id = self.ask_explanation(analyst, &request, &cache_key);
        if let Some(overlay) = &mut self.explain.overlay {
            overlay.id = id;
            overlay.refreshing = true;
        }
    }

    /// An explanation is tied to the code it explains, not to the commits it
    /// was asked about: a hunk by its text, a file by its content at both
    /// ends of the Scope.
    fn explanation_cache_key(&self, request: &ExplainRequest) -> String {
        let blob = |rev: &str, path: &str| {
            self.repo
                .run(&["rev-parse", "--verify", &format!("{rev}:{path}")])
                .map(|id| id.trim().to_string())
                .unwrap_or_default()
        };
        match &request.target {
            ExplainTarget::Hunk { path, text } => {
                AnalystCache::key("explain-hunk", &[&request.language, path, text])
            }
            ExplainTarget::File { path } => AnalystCache::key(
                "explain-file",
                &[
                    &request.language,
                    path,
                    &blob(&request.base, path),
                    &blob(&request.head, path),
                ],
            ),
        }
    }

    /// The hunk the cursor is on, or the nearest one above it (the first one
    /// below when there is none above), as `-`/`+`/space prefixed lines.
    fn hunk_under_cursor(&self) -> Option<String> {
        let Some(Ok(loaded)) = &self.loaded else {
            return None;
        };
        let hunks = &self.view.hunks;
        let start = hunks
            .iter()
            .rev()
            .find(|&&h| h <= self.vp.cursor)
            .or(hunks.first())
            .copied()?;
        let mut text = String::new();
        for item in &self.view.items[start..] {
            let Item::Line(entry) = *item else { break };
            let (old, new) = self.entry_lines(entry);
            let line = |side: Side, i: Option<usize>| {
                i.map(|i| loaded.lines(side)[i].as_str()).unwrap_or("")
            };
            let (removed, added, context) = match self.view_layout {
                DiffLayout::Inline => match self.inline[entry].kind {
                    InlineKind::Removed => (old, None, None),
                    InlineKind::Added => (None, new, None),
                    InlineKind::Context => (None, None, new),
                },
                DiffLayout::SideBySide => {
                    let flag = |side, i: Option<usize>, other| {
                        i.is_some_and(|x| loaded.line_changed(side, x, other))
                    };
                    let old_changed = flag(Side::Old, old, new);
                    let new_changed = flag(Side::New, new, old);
                    if !old_changed && !new_changed {
                        (None, None, new)
                    } else {
                        (
                            old.filter(|_| old_changed),
                            new.filter(|_| new_changed),
                            None,
                        )
                    }
                }
            };
            if removed.is_none() && added.is_none() && context.is_some() {
                break;
            }
            if let Some(i) = removed {
                text.push_str(&format!("-{}\n", line(Side::Old, Some(i))));
            }
            if let Some(i) = added {
                text.push_str(&format!("+{}\n", line(Side::New, Some(i))));
            }
        }
        (!text.is_empty()).then_some(text)
    }

    pub(super) fn explanations_busy(&self) -> bool {
        !self.explain.in_flight.is_empty()
    }

    pub(super) fn explanations_work(&self) -> Option<(usize, std::time::Duration)> {
        self.explain.in_flight.oldest()
    }

    /// Collect the explanations that arrived. Called from `tick`.
    pub(super) fn take_explanations(&mut self) {
        while let Ok(answer) = self.explain.receiver.try_recv() {
            self.take_explanation(answer);
        }
    }

    /// Block until every request for an explanation has been answered.
    pub(super) fn wait_for_explanations(&mut self) {
        while !self.explain.in_flight.is_empty() {
            match self.explain.receiver.recv() {
                Ok(answer) => self.take_explanation(answer),
                Err(_) => break,
            }
        }
    }

    fn take_explanation(&mut self, (id, answer): Answer) {
        self.explain.in_flight.remove(&id);
        let Some(overlay) = self.explain.overlay.as_mut().filter(|o| o.id == id) else {
            return;
        };
        let refreshed = std::mem::take(&mut overlay.refreshing);
        match answer {
            Ok(explanation) => {
                overlay.cached = false;
                overlay.state = State::Done(explanation.text);
            }
            Err(e) => {
                self.explain.error = Some(e.to_string());
                // A failed refresh keeps the answer it was meant to replace.
                if !(refreshed && matches!(overlay.state, State::Done(_))) {
                    overlay.state = State::Failed(e.to_string());
                }
            }
        }
    }

    /// A failed explanation comes first in the status line, like a failed
    /// Summary.
    pub(super) fn explain_status(&self, rest: &str) -> String {
        match &self.explain.error {
            Some(error) => format!("⚠ Analyst: {error}  {rest}"),
            None => rest.to_string(),
        }
    }

    /// The text of the overlay, laid out for its box.
    pub(super) fn explain_lines(&self) -> Vec<Line<'static>> {
        let Some(overlay) = &self.explain.overlay else {
            return Vec::new();
        };
        let width = scroll::text_cols(self.overlay_area());
        let mut lines = Vec::new();
        if overlay.refreshing {
            lines = plain_lines(
                &format!(
                    "{} The Analyst is updating the explanation… {}s",
                    super::analysis::spinner(self.clock()),
                    self.work_seconds("Explanation")
                ),
                width,
            );
            lines.push(Line::default());
        }
        let text = match &overlay.state {
            State::Waiting => format!(
                "{} The Analyst is explaining… {}s. Esc cancels.",
                super::analysis::spinner(self.clock()),
                self.work_seconds("Explanation")
            ),
            State::Done(text) => {
                lines.extend(crate::markdown::render(self.theme(), text, width));
                return lines;
            }
            State::Failed(error) => format!("Analyst: {error}"),
        };
        lines.extend(plain_lines(&text, width));
        lines
    }

    pub(super) fn explain_scroll_mut(&mut self) -> Option<&mut scroll::Scroll> {
        self.explain.overlay.as_mut().map(|o| &mut o.scroll)
    }

    pub(super) fn render_explain(&self, buf: &mut Buffer) {
        let Some(overlay) = &self.explain.overlay else {
            return;
        };
        let area = self.overlay_area();
        clear_area(self.theme(), buf, area);
        let title = match (overlay.refreshing, overlay.cached) {
            (true, _) => format!("{}updating ", overlay.title),
            (false, true) => format!("{}cached ", overlay.title),
            (false, false) => overlay.title.clone(),
        };
        draw_box(self.theme(), buf, area, &title);
        scroll::render_styled(
            self.theme(),
            buf,
            area,
            &self.explain_lines(),
            &overlay.scroll,
        );
    }
}

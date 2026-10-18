//! Diagrams inside the session: `:diagram`, its background request, cache and
//! overlay.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use super::analysis::Pending;
use super::*;
use unicode_width::UnicodeWidthStr;

use crate::analyst::{Analyst, DiagramRequest, DiagramSubject};
use crate::analyst_cache::AnalystCache;

/// What a diagram is cached under: head of the Range, language, subject.
type Key = (String, String, DiagramSubject);

/// The diagram is drawn on the worker's thread, not the event loop's.
type Answer = (Key, Result<Drawn, String>);

/// A diagram that came back, drawn once.
struct Drawn {
    mermaid: String,
    lines: Result<Vec<String>, String>,
}

pub(super) struct Diagrams {
    cache: HashMap<Key, Drawn>,
    /// Why the last request for a key failed. A failure is not cached: the
    /// next `:diagram` asks again.
    errors: HashMap<Key, String>,
    pending: Pending<Key>,
    /// Keys whose diagram was read from the disk cache, not asked for.
    from_cache: HashSet<Key>,
    sender: Sender<Answer>,
    receiver: Receiver<Answer>,
    /// The overlay is open on this diagram.
    open: Option<Key>,
    /// Columns the picture is scrolled to the left.
    hscroll: usize,
    /// How far the picture is scrolled up.
    scroll: scroll::Scroll,
    /// Mermaid texts of Summaries, drawn once.
    drawings: HashMap<String, Result<Vec<String>, String>>,
}

impl Default for Diagrams {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Diagrams {
            cache: HashMap::new(),
            errors: HashMap::new(),
            pending: Pending::default(),
            from_cache: HashSet::new(),
            sender,
            receiver,
            open: None,
            hscroll: 0,
            scroll: Default::default(),
            drawings: HashMap::new(),
        }
    }
}

impl Session {
    pub(super) fn diagram_open(&self) -> bool {
        self.diagrams.open.is_some()
    }

    /// `:diagram`. Returns whether `command` was one of the diagram's.
    pub(super) fn diagram_command(&mut self, command: &str) -> bool {
        let (here, group) = match command {
            "diagram" => (false, false),
            "diagram here" => (true, false),
            "diagram group" => (false, true),
            _ => return false,
        };
        let Some(analyst) = self.analyst_handle() else {
            self.message = Some("No Analyst is configured: there are no diagrams.".into());
            return true;
        };
        let Some(file) = self.files.get(self.selected) else {
            self.message = Some("No file to draw.".into());
            return true;
        };
        let path = file.path.clone();
        let subject = match (here, self.new_cursor_line()) {
            _ if group => match self.group_subject(&path) {
                Some(subject) => subject,
                None => {
                    self.message = Some("The file is in no Group: no diagram for one.".into());
                    return true;
                }
            },
            (true, Some(line)) => DiagramSubject::Fragment { path, line },
            _ => DiagramSubject::File { path },
        };
        let key = (self.range.head.clone(), self.language_code(), subject);
        if !self.diagrams.cache.contains_key(&key) {
            self.ask_diagram(analyst, key.clone(), false);
        }
        self.diagrams.open = Some(key);
        self.diagrams.hscroll = 0;
        self.diagrams.scroll = Default::default();
        true
    }

    /// Ask for the diagram of `key` unless it is on its way. Unless `force`,
    /// one in the disk cache is taken instead; with it the entry is rewritten.
    fn ask_diagram(&mut self, analyst: Arc<dyn Analyst>, key: Key, force: bool) {
        let now = self.clock();
        if !self.diagrams.pending.insert(key.clone(), now) {
            return;
        }
        self.diagrams.errors.remove(&key);
        let request = DiagramRequest {
            repo: self.repo.root().to_path_buf(),
            base: self.range.base.clone(),
            head: key.0.clone(),
            language: key.1.clone(),
            settings: self.analysis.settings.clone(),
            subject: key.2.clone(),
        };
        let cache = self.analyst_cache.clone();
        let cache_key = diagram_cache_key(&request);
        if !force {
            if let Some(diagram) = cache.load_diagram(&cache_key) {
                self.diagrams.pending.remove(&key);
                self.diagrams.from_cache.insert(key.clone());
                self.diagrams.cache.insert(
                    key,
                    Drawn {
                        lines: crate::diagram::draw(&diagram.mermaid),
                        mermaid: diagram.mermaid,
                    },
                );
                return;
            }
        }
        let sender = self.diagrams.sender.clone();
        std::thread::spawn(move || {
            let answer = analyst
                .diagram(&request)
                .inspect(|diagram| cache.store_diagram(&cache_key, diagram))
                .map(|diagram| Drawn {
                    lines: crate::diagram::draw(&diagram.mermaid),
                    mermaid: diagram.mermaid,
                })
                .map_err(|e| e.to_string());
            let _ = sender.send((key_of(&request), answer));
        });
    }

    /// `r` in the overlay: draw the open diagram again, whatever is cached.
    /// The old picture stays on screen until the new one comes.
    fn refresh_diagram(&mut self) {
        let Some(key) = self.diagrams.open.clone() else {
            return;
        };
        let Some(analyst) = self.analyst_handle() else {
            self.message = Some(NO_ANALYST_TO_REFRESH.into());
            return;
        };
        self.ask_diagram(analyst, key, true);
    }

    /// `:analyze all`: the diagrams drawn so far are forgotten and asked for
    /// again when next opened.
    pub(super) fn forget_diagrams(&mut self) {
        self.diagrams.cache.clear();
        self.diagrams.errors.clear();
        self.diagrams.from_cache.clear();
    }

    /// Whether the diagram of this open overlay was read from the disk cache
    /// of an earlier run.
    pub fn diagram_from_cache(&self) -> bool {
        self.diagrams
            .open
            .as_ref()
            .is_some_and(|key| self.diagrams.from_cache.contains(key))
    }

    /// The line of the new version of the file under the cursor (1-based);
    /// for a removed line, the nearest line that exists in the new one.
    fn new_cursor_line(&self) -> Option<usize> {
        let items = &self.view.items;
        (self.vp.cursor..items.len())
            .chain((0..self.vp.cursor).rev())
            .find_map(|i| self.item_lines(*items.get(i)?).1)
            .map(|line| line + 1)
    }

    fn take_diagram(&mut self, (key, answer): Answer) {
        self.diagrams.pending.remove(&key);
        match answer {
            Ok(drawn) => {
                self.diagrams.from_cache.remove(&key);
                self.diagrams.errors.remove(&key);
                self.diagrams.cache.insert(key, drawn);
            }
            Err(e) => {
                self.diagrams.errors.insert(key, e);
            }
        }
    }

    pub(super) fn diagrams_busy(&self) -> bool {
        !self.diagrams.pending.is_empty()
    }

    pub(super) fn diagrams_work(&self) -> Option<(usize, std::time::Duration)> {
        self.diagrams.pending.oldest()
    }

    /// Collect the diagrams that arrived.
    pub(super) fn diagram_tick(&mut self) {
        while let Ok(answer) = self.diagrams.receiver.try_recv() {
            self.take_diagram(answer);
        }
    }

    /// Block until every diagram asked for has been answered.
    pub(super) fn diagram_wait(&mut self) {
        while !self.diagrams.pending.is_empty() {
            match self.diagrams.receiver.recv() {
                Ok(answer) => self.take_diagram(answer),
                Err(_) => break,
            }
        }
    }

    /// `h`, `l` scroll the picture of `lines` sideways and `y` copies `source`.
    fn picture_key(&mut self, code: KeyCode, source: Option<String>, lines: &[String]) {
        match code {
            KeyCode::Char('l') => {
                let widest = lines.iter().map(|l| l.width()).max().unwrap_or(0);
                let limit = widest.saturating_sub(self.overlay_text_width());
                self.diagrams.hscroll = (self.diagrams.hscroll + HSCROLL_STEP).min(limit);
            }
            KeyCode::Char('h') => {
                self.diagrams.hscroll = self.diagrams.hscroll.saturating_sub(HSCROLL_STEP)
            }
            KeyCode::Char('y') => {
                if let Some(source) = source {
                    self.copy_to_clipboard(&source, "the Mermaid source");
                }
            }
            _ => {}
        }
    }

    /// Keys for the diagram inside the Summary overlay.
    pub(super) fn summary_diagram_key(&mut self, code: KeyCode) {
        let Some(mermaid) = self
            .current_summary()
            .and_then(|(summary, _)| summary.diagram.clone())
        else {
            return;
        };
        let lines = match self.diagrams.drawings.get(&mermaid) {
            Some(Ok(lines)) => lines.clone(),
            _ => failure_lines("", &mermaid),
        };
        self.picture_key(code, Some(mermaid), &lines);
    }

    /// A Summary overlay was opened: its diagram starts unscrolled.
    pub(super) fn reset_picture_scroll(&mut self) {
        self.diagrams.hscroll = 0;
    }

    /// Draw the Mermaid text of a Summary now, so that frames need not.
    pub(super) fn remember_drawing(&mut self, mermaid: &str) {
        self.diagrams
            .drawings
            .insert(mermaid.to_string(), crate::diagram::draw(mermaid));
    }

    /// Whether the Mermaid text was drawn, or failed to be.
    pub(super) fn has_drawing(&self, mermaid: &str) -> bool {
        self.diagrams.drawings.contains_key(mermaid)
    }

    /// The lines under a Summary for its diagram, scrolled sideways, or the
    /// reason it is missing and the source.
    pub(super) fn drawing_lines(&self, mermaid: &str) -> Vec<String> {
        match self.diagrams.drawings.get(mermaid) {
            Some(Ok(lines)) => lines
                .iter()
                .map(|l| l.chars().skip(self.diagrams.hscroll).collect())
                .collect(),
            Some(Err(e)) => failure_lines(e, mermaid),
            None => Vec::new(),
        }
    }

    /// Keys of the diagram overlay; returns whether the key was used.
    pub(super) fn diagram_key(&mut self, key: KeyEvent) -> bool {
        if self.diagrams.open.is_none() {
            return false;
        }
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            self.diagrams.open = None;
            return true;
        }
        let source = self.diagrams.open.as_ref().and_then(|key| {
            self.diagrams
                .cache
                .get(key)
                .map(|drawn| drawn.mermaid.clone())
        });
        let lines = self.open_lines();
        let rows = scroll::text_rows(self.overlay_area());
        let mut scroll = self.diagrams.scroll;
        if scroll.key(key, lines.len(), rows) {
            self.diagrams.scroll = scroll;
            return true;
        }
        if key.code == KeyCode::Char('r') {
            self.refresh_diagram();
            return true;
        }
        self.picture_key(key.code, source, &lines);
        true
    }

    pub(super) fn diagram_scroll_mut(&mut self) -> Option<&mut scroll::Scroll> {
        self.diagrams
            .open
            .is_some()
            .then_some(&mut self.diagrams.scroll)
    }

    pub(super) fn render_diagram_overlay(&self, buf: &mut Buffer) {
        let Some(key) = &self.diagrams.open else {
            return;
        };
        let area = self.overlay_area();
        clear_area(self.theme(), buf, area);
        let what = match &key.2 {
            DiagramSubject::File { path } | DiagramSubject::Fragment { path, .. } => path,
            DiagramSubject::Group { name, .. } => name,
        };
        let cached = if self.diagrams.pending.contains(key) && self.diagrams.cache.contains_key(key)
        {
            " updating"
        } else if self.diagram_from_cache() {
            " cached"
        } else {
            ""
        };
        draw_box(
            self.theme(),
            buf,
            area,
            &format!(" Diagram: {what}{cached} "),
        );
        let lines: Vec<String> = (self.open_lines().iter())
            .map(|l| l.chars().skip(self.diagrams.hscroll).collect())
            .collect();
        scroll::render_lines(self.theme(), buf, area, &lines, &self.diagrams.scroll);
    }

    /// Width in cells of the text inside the overlay's border.
    fn overlay_text_width(&self) -> usize {
        self.overlay_area().width.saturating_sub(4) as usize
    }

    /// The lines the open overlay shows.
    pub(super) fn open_lines(&self) -> Vec<String> {
        let Some(key) = &self.diagrams.open else {
            return Vec::new();
        };
        let note = if self.diagrams.pending.contains(key) {
            Some(format!(
                "{} The Analyst is updating the diagram… {}s",
                super::analysis::spinner(self.clock()),
                self.work_seconds("Diagram")
            ))
        } else {
            (self.diagrams.errors.get(key)).map(|e| {
                format!(
                    "Analyst: {} (the previous diagram is kept)",
                    crate::diagram::sanitize(e)
                )
            })
        };
        let picture = match self.diagrams.cache.get(key) {
            Some(Drawn { lines: Ok(l), .. }) => l.clone(),
            Some(Drawn {
                lines: Err(e),
                mermaid,
            }) => failure_lines(e, mermaid),
            None => {
                return match self.diagrams.errors.get(key) {
                    Some(e) => vec![format!("Analyst: {}", crate::diagram::sanitize(e))],
                    None => vec!["The diagram is being drawn…".into()],
                }
            }
        };
        match note {
            Some(note) => [note, String::new()].into_iter().chain(picture).collect(),
            None => picture,
        }
    }
}

/// What `r` says in an Explanation or a Diagram with no Analyst.
pub(super) const NO_ANALYST_TO_REFRESH: &str = "No Analyst is configured: nothing to ask again";

/// What is shown instead of a picture that could not be drawn.
fn failure_lines(error: &str, mermaid: &str) -> Vec<String> {
    format!("Cannot draw the diagram: {error}\n\n{mermaid}")
        .lines()
        .map(crate::diagram::sanitize)
        .collect()
}

/// A diagram depends on the Range, the language and what it is drawn for.
fn diagram_cache_key(request: &DiagramRequest) -> String {
    let subject = match &request.subject {
        DiagramSubject::File { path } => format!("file\u{2}{path}"),
        DiagramSubject::Fragment { path, line } => format!("fragment\u{2}{path}\u{2}{line}"),
        DiagramSubject::Group { name, files } => {
            format!("group\u{2}{name}\u{2}{}", files.join("\u{3}"))
        }
    };
    AnalystCache::key(
        "diagram",
        &[&request.base, &request.head, &request.language, &subject],
    )
}

fn key_of(request: &DiagramRequest) -> Key {
    (
        request.head.clone(),
        request.language.clone(),
        request.subject.clone(),
    )
}

/// Blank the area an overlay is drawn over: text and colours of whatever was
/// drawn there before are gone.
pub(super) fn clear_area(theme: &Theme, buf: &mut Buffer, area: Rect) {
    let area = area.intersection(buf.area);
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, y)];
            cell.reset();
            cell.set_fg(theme.fg);
        }
    }
}

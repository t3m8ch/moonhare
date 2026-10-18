//! Diagrams inside the session: `:diagram`, its background request, cache and
//! overlay.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use super::analysis::Pending;
use super::pan::{self, Pan, Size};
use super::*;

use crate::analyst::{Analyst, DiagramRequest, DiagramSubject};
use crate::analyst_cache::AnalystCache;
use crate::diagram::{Density, Room};

/// What a diagram is cached under: head of the Range, language, subject.
type Key = (String, String, DiagramSubject);

/// The Mermaid text of a diagram that came back, or why it did not.
type Answer = (Key, Result<String, String>);

/// What a picture is drawn for: its Mermaid text (hashed), the room and the
/// density.
type PictureKey = (u64, Room, Density);

/// Pictures drawn so far are kept, up to this many.
const PICTURES: usize = 64;

/// Mermaid texts drawn as pictures, once for each room and density: a frame
/// draws nothing it drew before.
#[derive(Default)]
struct Pictures {
    drawn: RefCell<HashMap<PictureKey, Rc<Vec<String>>>>,
    /// Why a text could not be drawn: it is not tried again for another room,
    /// so a text that takes too long costs that time once.
    failed: RefCell<HashMap<u64, String>>,
}

impl Pictures {
    fn get(&self, mermaid: &str, room: Room, density: Density) -> Result<Rc<Vec<String>>, String> {
        let hash = {
            let mut hasher = DefaultHasher::new();
            mermaid.hash(&mut hasher);
            hasher.finish()
        };
        if let Some(e) = self.failed.borrow().get(&hash) {
            return Err(e.clone());
        }
        let key = (hash, room, density);
        if let Some(lines) = self.drawn.borrow().get(&key) {
            return Ok(lines.clone());
        }
        match crate::diagram::draw_in(mermaid, room, density) {
            Ok(lines) => {
                let lines = Rc::new(lines);
                let mut drawn = self.drawn.borrow_mut();
                if drawn.len() >= PICTURES {
                    drawn.clear();
                }
                drawn.insert(key, lines.clone());
                Ok(lines)
            }
            Err(e) => {
                self.failed.borrow_mut().insert(hash, e.clone());
                Err(e)
            }
        }
    }
}

pub(super) struct Diagrams {
    cache: HashMap<Key, String>,
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
    /// The diagram of the Summary is shown on the whole terminal: its
    /// Mermaid text.
    summary_full: Option<String>,
    /// The overlay covers the whole terminal, without its border.
    full: bool,
    /// Where the overlay's view of its picture is.
    view: Pan,
    /// How densely the overlay lays its picture out.
    density: Density,
    /// How far the diagrams of the Summary are moved sideways.
    inline: Pan,
    /// How densely the diagrams of the Summary are laid out.
    inline_density: Density,
    pictures: Pictures,
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
            summary_full: None,
            full: false,
            view: Pan::default(),
            density: Density::Fit,
            inline: Pan::default(),
            inline_density: Density::Fit,
            pictures: Pictures::default(),
        }
    }
}

impl Session {
    pub(super) fn diagram_open(&self) -> bool {
        self.diagrams.open.is_some() || self.diagrams.summary_full.is_some()
    }

    /// `f` in the Summary: its diagram (the one the Analyst attached, else
    /// the first in its text) on the whole terminal.
    pub(super) fn open_summary_diagram(&mut self) {
        let mermaid = self.current_summary().and_then(|(summary, _)| {
            (summary.diagram.clone()).or_else(|| {
                crate::markdown::fenced_blocks(&summary.text, "mermaid")
                    .into_iter()
                    .next()
            })
        });
        let Some(mermaid) = mermaid else {
            self.message = Some("The Summary has no diagram to show full screen".into());
            return;
        };
        self.diagrams.summary_full = Some(mermaid);
        self.diagrams.full = true;
        self.diagrams.view = Pan::default();
        self.diagrams.density = Density::Fit;
    }

    /// The Mermaid text of the diagram the overlay shows, if it came.
    fn shown_mermaid(&self) -> Option<&String> {
        match &self.diagrams.summary_full {
            Some(mermaid) => Some(mermaid),
            None => self.diagrams.cache.get(self.diagrams.open.as_ref()?),
        }
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
        self.diagrams.summary_full = None;
        self.diagrams.full = false;
        self.diagrams.view = Pan::default();
        self.diagrams.density = Density::Fit;
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
                self.diagrams.cache.insert(key, diagram.mermaid);
                return;
            }
        }
        let sender = self.diagrams.sender.clone();
        std::thread::spawn(move || {
            let answer = analyst
                .diagram(&request)
                .inspect(|diagram| cache.store_diagram(&cache_key, diagram))
                .map(|diagram| diagram.mermaid)
                .map_err(|e| e.to_string());
            let _ = sender.send((key_of(&request), answer));
        });
    }

    /// `r` in the overlay: draw the open diagram again, whatever is cached.
    /// The old picture stays on screen until the new one comes.
    fn refresh_diagram(&mut self) {
        if self.diagrams.summary_full.is_some() {
            self.message =
                Some("The diagram of the Summary is drawn again with it: :analyze".into());
            return;
        }
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

    /// Keys for the diagrams inside the Summary overlay: they move sideways.
    pub(super) fn summary_diagram_key(&mut self, key: KeyEvent) {
        if let Some(to) = self.density_key(key.code, self.diagrams.inline_density) {
            let to = match to {
                Some(Density::Overview) => {
                    self.message = Some(NO_INLINE_OVERVIEW.into());
                    return;
                }
                to => to,
            };
            let Some(to) = self.density_or_say(to) else {
                return;
            };
            let from = self.widest_summary_drawing();
            self.diagrams.inline_density = to;
            let to_lines = self.widest_summary_drawing();
            let view = self.summary_text_size();
            let left = pan::recentre(&self.diagrams.inline, (&from, view), (&to_lines, view));
            self.diagrams.inline = Pan::at(left.left(&to_lines, view), 0);
            return;
        }
        let sideways = matches!(
            key.code,
            KeyCode::Char('h' | 'l' | 'H' | 'L' | '0' | '$') | KeyCode::Left | KeyCode::Right
        );
        if !sideways {
            return;
        }
        let lines = self.widest_summary_drawing();
        let view = self.summary_text_size();
        self.diagrams.inline.key(key, &lines, view);
    }

    /// A notch of the wheel sideways over the Summary.
    pub(super) fn summary_diagram_wheel(&mut self, forward: bool) {
        let lines = self.widest_summary_drawing();
        let view = self.summary_text_size();
        self.diagrams.inline.wheel(forward, true, &lines, view);
    }

    /// The widest of the drawings of the Summary shown: how far they move.
    fn widest_summary_drawing(&self) -> Vec<String> {
        let Some((summary, _)) = self.current_summary() else {
            return Vec::new();
        };
        (summary.diagram.iter().cloned())
            .chain(crate::markdown::fenced_blocks(&summary.text, "mermaid"))
            .filter_map(|mermaid| self.summary_picture(&mermaid).ok())
            .map(|lines| lines.to_vec())
            .max_by_key(|lines| Size::of(lines).cols)
            .unwrap_or_default()
    }

    fn summary_text_size(&self) -> Size {
        let area = self.overlay_area();
        Size {
            cols: scroll::text_cols(area),
            rows: scroll::text_rows(area),
        }
    }

    /// A Summary overlay was opened: its diagram starts unscrolled.
    pub(super) fn reset_picture_scroll(&mut self) {
        self.diagrams.inline = Pan::at(0, 0);
        self.diagrams.inline_density = Density::Fit;
    }

    /// The picture of a Mermaid text of the Summary, fitted to its overlay.
    fn summary_picture(&self, mermaid: &str) -> Result<Rc<Vec<String>>, String> {
        let view = self.summary_text_size();
        let room = Room {
            width: view.cols,
            height: view.rows,
        };
        (self.diagrams.pictures).get(mermaid, room, self.diagrams.inline_density)
    }

    /// The density `+`, `-` or `=` asks for, from `now`: `None` when the key
    /// is not one of them, `Some(None)` past the last step.
    fn density_key(&self, code: KeyCode, now: Density) -> Option<Option<Density>> {
        match code {
            KeyCode::Char('+') => Some(now.roomier()),
            KeyCode::Char('-') => Some(now.denser()),
            KeyCode::Char('=') => Some(Some(Density::Fit)),
            _ => None,
        }
    }

    /// `to`, or says why there is no step further.
    fn density_or_say(&mut self, to: Option<Density>) -> Option<Density> {
        if to.is_none() {
            self.message = Some(
                "The diagram is as roomy (+) or as dense (-) as it gets; = fits it to the window"
                    .into(),
            );
        }
        to
    }

    /// The lines under a Summary for its diagram, moved sideways, or the
    /// reason it is missing and the source.
    pub(super) fn drawing_lines(&self, mermaid: &str) -> Vec<String> {
        match self.summary_picture(mermaid) {
            Ok(lines) => {
                let view = self.summary_text_size();
                let left = self.diagrams.inline.left(&lines, view);
                lines
                    .iter()
                    .map(|l| pan::slice(l, left, view.cols))
                    .collect()
            }
            Err(e) => failure_lines(&e, mermaid),
        }
    }

    /// Keys of the diagram overlay; returns whether the key was used.
    pub(super) fn diagram_key(&mut self, key: KeyEvent) -> bool {
        if !self.diagram_open() {
            return false;
        }
        let summary = self.diagrams.summary_full.is_some();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char(_) if ctrl => {}
            // Back from the whole terminal; from the Summary's diagram, to it.
            KeyCode::Esc | KeyCode::Char('f') if summary => {
                self.diagrams.summary_full = None;
                self.diagrams.full = false;
                return true;
            }
            KeyCode::Esc if self.diagrams.full => {
                self.diagrams.full = false;
                return true;
            }
            KeyCode::Char('f') => {
                self.change_room(|d| d.full = !d.full);
                return true;
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.diagrams.open = None;
                self.diagrams.summary_full = None;
                self.diagrams.full = false;
                return true;
            }
            _ => {}
        }
        let source = self.shown_mermaid().cloned();
        match key.code {
            KeyCode::Char('r') => self.refresh_diagram(),
            KeyCode::Char('y') => {
                if let Some(source) = source {
                    self.copy_to_clipboard(&source, "the Mermaid source");
                }
            }
            KeyCode::Enter if self.diagrams.density == Density::Overview => {
                self.diagrams.density = Density::Fit;
            }
            code => {
                if let Some(to) = self.density_key(code, self.diagrams.density) {
                    if let Some(to) = self.density_or_say(to) {
                        self.change_density(to);
                    }
                    return true;
                }
                let lines = self.open_lines();
                let view = self.diagram_view();
                // In the overview a step moves the frame by a cell of it.
                let steps = match self.overview_of(&lines) {
                    Some(overview)
                        if matches!(
                            code,
                            KeyCode::Char('h' | 'j' | 'k' | 'l')
                                | KeyCode::Left
                                | KeyCode::Right
                                | KeyCode::Up
                                | KeyCode::Down
                        ) =>
                    {
                        overview.scale.ceil() as usize
                    }
                    _ => 1,
                };
                for _ in 0..steps {
                    self.diagrams.view.key(key, &lines, view);
                }
            }
        }
        true
    }

    /// Change what decides the room of the picture (full screen or not),
    /// keeping the centre of the view on the picture laid out again.
    fn change_room(&mut self, change: impl FnOnce(&mut Diagrams)) {
        let (old, old_view) = (self.open_lines(), self.diagram_view());
        change(&mut self.diagrams);
        let (new, new_view) = (self.open_lines(), self.diagram_view());
        self.diagrams.view = pan::recentre(&self.diagrams.view, (&old, old_view), (&new, new_view));
    }

    /// Lay the open picture out at `to`, keeping the centre of the view.
    fn change_density(&mut self, to: Density) {
        let from = self.diagrams.density;
        if drawn_as(from) != drawn_as(to) {
            let (old, new) = (self.open_lines_at(from), self.open_lines_at(to));
            let view = self.diagram_view();
            self.diagrams.view = pan::recentre(&self.diagrams.view, (&old, view), (&new, view));
        }
        self.diagrams.density = to;
    }

    /// The overview of `lines` with the frame of the view, when the overlay
    /// shows the overview.
    fn overview_of(&self, lines: &[String]) -> Option<crate::diagram::Overview> {
        if self.diagrams.density != Density::Overview {
            return None;
        }
        let view = self.diagram_view();
        let pan = &self.diagrams.view;
        Some(crate::diagram::overview(
            lines,
            self.diagram_room(),
            (
                pan.left(lines, view),
                pan.top(lines, view),
                view.cols,
                view.rows,
            ),
        ))
    }

    fn diagram_view(&self) -> Size {
        let text = self.diagram_place().text;
        Size {
            cols: text.width as usize,
            rows: text.height as usize,
        }
    }

    fn diagram_room(&self) -> Room {
        let view = self.diagram_view();
        Room {
            width: view.cols,
            height: view.rows,
        }
    }

    /// A notch of the wheel over the diagram overlay.
    pub(super) fn diagram_wheel(&mut self, forward: bool, sideways: bool) {
        let lines = self.open_lines();
        let view = self.diagram_view();
        self.diagrams.view.wheel(forward, sideways, &lines, view);
    }

    /// Where the diagram overlay draws its picture: inside the border, or
    /// on the whole terminal above a line that says what it is.
    pub(super) fn diagram_place(&self) -> pan::Place {
        if self.diagrams.full {
            let (width, height) = self.size;
            let text = Rect::new(0, 0, width, height.saturating_sub(1));
            return pan::Place {
                text,
                edges: None,
                label: Some((Rect::new(0, height.saturating_sub(1), width, 1), 0)),
            };
        }
        let area = self.overlay_area();
        pan::Place {
            text: Rect::new(
                area.x + 2,
                area.y + 1,
                area.width.saturating_sub(4),
                area.height.saturating_sub(2),
            ),
            edges: Some(area),
            label: Some((
                Rect::new(
                    area.x,
                    area.y + area.height.saturating_sub(1),
                    area.width,
                    1,
                ),
                2,
            )),
        }
    }

    pub(super) fn render_diagram_overlay(&self, buf: &mut Buffer) {
        let title = match (&self.diagrams.summary_full, &self.diagrams.open) {
            (Some(_), _) => " Diagram: Summary ".to_string(),
            (None, Some(key)) => {
                let what = match &key.2 {
                    DiagramSubject::File { path } | DiagramSubject::Fragment { path, .. } => path,
                    DiagramSubject::Group { name, .. } => name,
                };
                let cached = if self.diagrams.pending.contains(key)
                    && self.diagrams.cache.contains_key(key)
                {
                    " updating"
                } else if self.diagram_from_cache() {
                    " cached"
                } else {
                    ""
                };
                format!(" Diagram: {what}{cached} ")
            }
            (None, None) => return,
        };
        let place = self.diagram_place();
        if self.diagrams.full {
            let (width, height) = self.size;
            clear_area(self.theme(), buf, Rect::new(0, 0, width, height));
            let line = match &self.message {
                Some(message) => format!(" {message}"),
                None => format!("{title}· f/Esc: назад"),
            };
            if let Some((row, _)) = place.label {
                buf.set_stringn(
                    row.x,
                    row.y,
                    &line,
                    row.width as usize,
                    Style::new().fg(self.theme().dim),
                );
            }
        } else {
            let area = self.overlay_area();
            clear_area(self.theme(), buf, area);
            draw_box(self.theme(), buf, area, &title);
        }
        let lines = self.open_lines();
        match self.overview_of(&lines) {
            Some(overview) => self.render_overview(buf, &place, &overview),
            None => pan::render(self.theme(), buf, &place, &lines, &self.diagrams.view),
        }
    }

    /// The overview in the overlay: dots, the frame of the view lit, and what
    /// the keys do on the border.
    fn render_overview(
        &self,
        buf: &mut Buffer,
        place: &pan::Place,
        overview: &crate::diagram::Overview,
    ) {
        let theme = self.theme();
        let text = place.text;
        for (i, line) in overview.lines.iter().enumerate().take(text.height as usize) {
            buf.set_stringn(
                text.x,
                text.y + i as u16,
                line,
                text.width as usize,
                Style::new().fg(theme.fg),
            );
        }
        let (x, y, w, h) = overview.frame;
        for row in y..(y + h).min(text.height as usize) {
            for col in x..(x + w).min(text.width as usize) {
                let cell = &mut buf[(text.x + col as u16, text.y + row as u16)];
                cell.set_bg(theme.selection_bg);
                cell.set_fg(theme.accent);
            }
        }
        if let Some((row, margin)) = place.label {
            let label = " обзор · hjkl: рамка · Enter: сюда · =: обычный масштаб ";
            let len = label.chars().count() as u16;
            if row.width >= len + 2 * margin {
                buf.set_stringn(
                    row.x + row.width - margin - len,
                    row.y,
                    label,
                    len as usize,
                    Style::new().fg(theme.dim),
                );
            }
        }
    }

    /// The lines the open overlay shows; in the overview, those it is made of.
    pub(super) fn open_lines(&self) -> Vec<String> {
        self.open_lines_at(self.diagrams.density)
    }

    /// The lines the open overlay shows at `density`.
    fn open_lines_at(&self, density: Density) -> Vec<String> {
        let density = drawn_as(density);
        if let Some(mermaid) = &self.diagrams.summary_full {
            return match (self.diagrams.pictures).get(mermaid, self.diagram_room(), density) {
                Ok(lines) => lines.to_vec(),
                Err(e) => failure_lines(&e, mermaid),
            };
        }
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
            Some(mermaid) => {
                match (self.diagrams.pictures).get(mermaid, self.diagram_room(), density) {
                    Ok(lines) => lines.to_vec(),
                    Err(e) => failure_lines(&e, mermaid),
                }
            }
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

/// The density a picture is drawn at: the overview is made of the fitted one.
fn drawn_as(density: Density) -> Density {
    match density {
        Density::Overview => Density::Fit,
        other => other,
    }
}

/// What `-` says in the Summary past its densest layout.
const NO_INLINE_OVERVIEW: &str =
    "The Summary shows no overview of its diagram: :diagram has one (-, -)";

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

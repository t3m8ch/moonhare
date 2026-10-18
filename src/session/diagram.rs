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
    /// Where the overlay's view of its picture is.
    view: Pan,
    /// How far the diagrams of the Summary are moved sideways.
    inline: Pan,
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
            view: Pan::default(),
            inline: Pan::default(),
            pictures: Pictures::default(),
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
        self.diagrams.view = Pan::default();
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
    }

    /// The picture of a Mermaid text of the Summary, fitted to its overlay.
    fn summary_picture(&self, mermaid: &str) -> Result<Rc<Vec<String>>, String> {
        let view = self.summary_text_size();
        let room = Room {
            width: view.cols,
            height: view.rows,
        };
        self.diagrams.pictures.get(mermaid, room, Density::Fit)
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
        if self.diagrams.open.is_none() {
            return false;
        }
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            self.diagrams.open = None;
            return true;
        }
        let source =
            (self.diagrams.open.as_ref()).and_then(|key| self.diagrams.cache.get(key).cloned());
        match key.code {
            KeyCode::Char('r') => self.refresh_diagram(),
            KeyCode::Char('y') => {
                if let Some(source) = source {
                    self.copy_to_clipboard(&source, "the Mermaid source");
                }
            }
            _ => {
                let lines = self.open_lines();
                let view = self.diagram_place().text;
                let view = Size {
                    cols: view.width as usize,
                    rows: view.height as usize,
                };
                self.diagrams.view.key(key, &lines, view);
            }
        }
        true
    }

    /// A notch of the wheel over the diagram overlay.
    pub(super) fn diagram_wheel(&mut self, forward: bool, sideways: bool) {
        let lines = self.open_lines();
        let text = self.diagram_place().text;
        let view = Size {
            cols: text.width as usize,
            rows: text.height as usize,
        };
        self.diagrams.view.wheel(forward, sideways, &lines, view);
    }

    /// Where the diagram overlay draws its picture.
    fn diagram_place(&self) -> pan::Place {
        let area = self.overlay_area();
        pan::Place {
            text: Rect::new(
                area.x + 2,
                area.y + 1,
                area.width.saturating_sub(4),
                area.height.saturating_sub(2),
            ),
            edges: area,
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
        pan::render(
            self.theme(),
            buf,
            &self.diagram_place(),
            &self.open_lines(),
            &self.diagrams.view,
        );
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
            Some(mermaid) => {
                let text = self.diagram_place().text;
                let room = Room {
                    width: text.width as usize,
                    height: text.height as usize,
                };
                match self.diagrams.pictures.get(mermaid, room, Density::Fit) {
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

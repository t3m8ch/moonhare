//! Diagrams inside the session: `:diagram`, its background request, cache and
//! overlay.

use std::cell::{Cell, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use super::analysis::Pending;
use super::pan::{self, Pan, Size};
use super::*;

use crate::analyst::{Analyst, DiagramRequest, DiagramSubject};
use crate::analyst_cache::AnalystCache;
use crate::diagram::{Density, Drawings, Renderer, Room};

/// What a diagram is cached under: head of the Range, language, subject.
type Key = (String, String, DiagramSubject);

/// The Mermaid text of a diagram that came back, or why it did not.
type Answer = (Key, Result<String, String>);

/// What a picture is drawn for: its Mermaid text (hashed), the room and the
/// density.
type PictureKey = (u64, Room, Density);

/// Pictures drawn so far are kept, up to this many.
const PICTURES: usize = 64;

/// Pictures waiting to be laid out, at most this many: the latest asked for
/// goes first and the oldest are dropped, so dragging the edge of the
/// terminal does not line up a picture for every size it passed.
const QUEUED: usize = 4;

/// A picture as a frame shows it.
enum Picture {
    Ready(Rc<Vec<String>>),
    /// Being laid out in the background; meanwhile, the last picture of the
    /// same text, at another size or density, if there is one.
    Drawing(Option<Rc<Vec<String>>>),
    /// Why it could not be drawn, for this room and density only.
    Failed(String),
}

/// A picture laid out on the renderer's thread.
type Laid = (PictureKey, Result<Vec<String>, String>);

/// A picture laid out and kept, or why it could not be.
type Kept = Result<Rc<Vec<String>>, String>;

/// Mermaid texts drawn as pictures, once for each room and density. A frame
/// never waits for one: a picture whose layouts are all drawn already is
/// laid out there and then (a resize, another density), any other is laid
/// out in the background, one at a time, and shown when it comes.
struct Pictures {
    drawings: Drawings,
    /// The pictures laid out so far, or why they could not be.
    drawn: RefCell<HashMap<PictureKey, Kept>>,
    /// The last picture laid out of each text.
    latest: RefCell<HashMap<u64, Rc<Vec<String>>>>,
    /// The pictures to lay out next, the latest asked for first.
    queue: RefCell<VecDeque<(PictureKey, String)>>,
    /// The picture being laid out now.
    running: Cell<Option<PictureKey>>,
    /// [`Drawings::generation`] when it started.
    started_at: Cell<u64>,
    /// The pictures that were too slow to lay out, and the generation of
    /// the drawings then: once a layout given up on comes, they are laid out
    /// again, so "too long" goes away by itself.
    too_slow: RefCell<HashMap<PictureKey, u64>>,
    /// How long laying out one picture may take.
    budget: Duration,
    sender: Sender<Laid>,
    receiver: Receiver<Laid>,
}

impl Default for Pictures {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Pictures {
            drawings: Drawings::default(),
            drawn: RefCell::default(),
            latest: RefCell::default(),
            queue: RefCell::default(),
            running: Cell::new(None),
            started_at: Cell::new(0),
            too_slow: RefCell::default(),
            budget: crate::diagram::RENDER_TIMEOUT,
            sender,
            receiver,
        }
    }
}

impl Pictures {
    /// `mermaid` drawn for `room` at `density`, or asked for.
    fn get(&self, mermaid: &str, room: Room, density: Density) -> Picture {
        let hash = {
            let mut hasher = DefaultHasher::new();
            mermaid.hash(&mut hasher);
            hasher.finish()
        };
        let key = (hash, room, density);
        let since = self.too_slow.borrow().get(&key).copied();
        if since.is_some_and(|since| self.drawings.generation() > since) {
            self.too_slow.borrow_mut().remove(&key);
            self.drawn.borrow_mut().remove(&key);
        }
        if let Some(done) = self.drawn.borrow().get(&key) {
            return match done {
                Ok(lines) => Picture::Ready(lines.clone()),
                Err(e) => Picture::Failed(e.clone()),
            };
        }
        if let Some(done) = self.drawings.fit_now(mermaid, room, density) {
            return self.keep(key, done);
        }
        let asked = self.running.get() == Some(key)
            || self.queue.borrow().iter().any(|(queued, _)| *queued == key);
        if !asked {
            let mut queue = self.queue.borrow_mut();
            queue.push_front((key, mermaid.to_string()));
            queue.truncate(QUEUED);
        }
        self.start();
        Picture::Drawing(self.latest.borrow().get(&hash).cloned())
    }

    /// Keep what `key` was laid out as.
    fn keep(&self, key: PictureKey, done: Result<Vec<String>, String>) -> Picture {
        let mut drawn = self.drawn.borrow_mut();
        if drawn.len() >= PICTURES {
            drawn.clear();
        }
        let done = done.map(Rc::new);
        drawn.insert(key, done.clone());
        match done {
            Ok(lines) => {
                self.latest.borrow_mut().insert(key.0, lines.clone());
                Picture::Ready(lines)
            }
            Err(e) => Picture::Failed(e),
        }
    }

    /// Start laying out the next picture, unless one is being laid out.
    fn start(&self) {
        if self.running.get().is_some() {
            return;
        }
        let Some((key, mermaid)) = self.queue.borrow_mut().pop_front() else {
            return;
        };
        self.running.set(Some(key));
        self.started_at.set(self.drawings.generation());
        let sender = self.sender.clone();
        let (_, room, density) = key;
        let budget = self.budget;
        self.drawings
            .fit_later(mermaid, room, density, budget, move |done| {
                let _ = sender.send((key, done));
            });
    }

    /// Keep a picture that was laid out and start the next.
    fn take(&self, (key, done): Laid) {
        if done.as_ref().is_err_and(|e| e == crate::diagram::TOO_SLOW) {
            (self.too_slow.borrow_mut()).insert(key, self.started_at.get());
        }
        self.keep(key, done);
        if self.running.get() == Some(key) {
            self.running.set(None);
        }
        self.start();
    }

    /// Collect the pictures laid out; returns whether any came.
    fn tick(&self) -> bool {
        let mut came = false;
        while let Ok(laid) = self.receiver.try_recv() {
            self.take(laid);
            came = true;
        }
        came
    }

    /// Whether a picture is being laid out, or one too slow waits for a
    /// layout still being drawn.
    fn busy(&self) -> bool {
        self.running.get().is_some()
            || !self.queue.borrow().is_empty()
            || (!self.too_slow.borrow().is_empty() && self.drawings.drawing_now())
    }

    /// Block until every picture asked for is laid out (a layout given up
    /// on is waited for too: the next frame lays its picture out again).
    fn wait(&self) {
        loop {
            if self.running.get().is_some() {
                match self.receiver.recv() {
                    Ok(laid) => self.take(laid),
                    Err(_) => break,
                }
            } else if self.busy() {
                std::thread::sleep(Duration::from_millis(5));
            } else {
                break;
            }
        }
    }

    /// Draw with `renderer` from now on, forgetting what was drawn.
    fn set_renderer(&mut self, renderer: Renderer) {
        self.wait();
        *self = Pictures {
            drawings: Drawings::with(renderer),
            ..Pictures::default()
        };
    }
}

/// The SVG files `o` wrote, removed in time: the viewer reads one after `o`
/// returns, so a file goes at the next `o` once it is a minute old, and
/// with the session.
#[derive(Default, Clone)]
struct Svgs(Arc<Mutex<Vec<(PathBuf, SystemTime)>>>);

/// How long an SVG is kept for its viewer at least.
const SVG_KEPT: Duration = Duration::from_secs(60);

/// SVGs of earlier sessions that are older than this are removed too.
const SVG_LEFT_OVER: Duration = Duration::from_secs(24 * 60 * 60);

/// The prefix and suffix of the name of an SVG `o` writes; its page beside
/// it ends in `.html` instead.
const SVG_NAME: (&str, &str) = ("moonhare-diagram-", ".svg");

impl Svgs {
    /// Remember `path`, written now.
    fn add(&self, path: PathBuf) {
        let mut svgs = self.0.lock().unwrap_or_else(|e| e.into_inner());
        svgs.push((path, SystemTime::now()));
    }

    /// Remove the files older than [`SVG_KEPT`], and the ones earlier
    /// sessions left in the temporary directory for over a day.
    fn sweep(&self) {
        let now = SystemTime::now();
        let old = |at: SystemTime, age: Duration| now.duration_since(at).is_ok_and(|d| d > age);
        let mut svgs = self.0.lock().unwrap_or_else(|e| e.into_inner());
        svgs.retain(|(path, at)| {
            let keep = !old(*at, SVG_KEPT);
            if !keep {
                let _ = std::fs::remove_file(path);
            }
            keep
        });
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let ours = name.ends_with(SVG_NAME.1) || name.ends_with(".html");
            if !(name.starts_with(SVG_NAME.0) && ours) {
                continue;
            }
            let modified = entry.metadata().and_then(|m| m.modified());
            if modified.is_ok_and(|at| old(at, SVG_LEFT_OVER)) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

impl Svgs {
    /// Remove every file: the session ends.
    fn remove_all(&self) {
        let mut svgs = self.0.lock().unwrap_or_else(|e| e.into_inner());
        for (path, _) in svgs.drain(..) {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for Diagrams {
    fn drop(&mut self) {
        self.svgs.remove_all();
    }
}

/// The program that shows a file: `open` on macOS, `xdg-open` elsewhere,
/// found on `PATH`.
pub fn system_viewer() -> OsString {
    if cfg!(target_os = "macos") {
        "open".into()
    } else {
        "xdg-open".into()
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
    /// What `o` has to say, from its thread: the SVG opened, or why not.
    viewer: (Sender<String>, Receiver<String>),
    /// The program `o` hands the SVG to.
    viewer_program: OsString,
    /// How many `o` are drawing their SVG now.
    svgs_drawing: Arc<AtomicUsize>,
    /// The SVG files `o` wrote.
    svgs: Svgs,
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
    /// Pictures drawn so far, kept for the next frames.
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
            viewer: channel(),
            viewer_program: system_viewer(),
            svgs_drawing: Arc::default(),
            svgs: Svgs::default(),
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
        let Some(mermaid) = self.summary_mermaid() else {
            self.message = Some("The Summary has no diagram to show full screen".into());
            return;
        };
        self.diagrams.summary_full = Some(mermaid);
        self.diagrams.full = true;
        self.diagrams.view = Pan::default();
        self.diagrams.density = Density::Fit;
    }

    /// `o` in the Summary: its diagram in the system viewer.
    pub(super) fn open_summary_diagram_outside(&mut self) {
        match self.summary_mermaid() {
            Some(mermaid) => self.open_outside(&mermaid),
            None => self.message = Some("The Summary has no diagram to open".into()),
        }
    }

    /// The diagram of the Summary: the one the Analyst attached, else the
    /// first in its text.
    fn summary_mermaid(&self) -> Option<String> {
        self.current_summary().and_then(|(summary, _)| {
            (summary.diagram.clone()).or_else(|| {
                crate::markdown::fenced_blocks(&summary.text, "mermaid")
                    .into_iter()
                    .next()
            })
        })
    }

    /// Draw `mermaid` as SVG into a temporary file and hand the system viewer
    /// ([`system_viewer`]) a page that shows it as an image ([`write_page`]),
    /// at any zoom.
    /// The work is done on a thread of its own: what came of it is said
    /// when it is done.
    fn open_outside(&mut self, mermaid: &str) {
        self.diagrams.svgs.sweep();
        self.message = Some("Drawing the diagram as SVG…".into());
        let mermaid = mermaid.to_string();
        let viewer = self.diagrams.viewer_program.clone();
        let sender = self.diagrams.viewer.0.clone();
        let svgs = self.diagrams.svgs.clone();
        let drawing = self.diagrams.svgs_drawing.clone();
        drawing.fetch_add(1, Ordering::SeqCst);
        let spawned = std::thread::Builder::new()
            .name("moonhare-svg".into())
            .spawn(move || {
                let said = show_outside(&mermaid, &viewer, &svgs);
                drawing.fetch_sub(1, Ordering::SeqCst);
                let _ = sender.send(said.message);
                if let Some(mut child) = said.child {
                    let name = viewer.to_string_lossy().into_owned();
                    let failure = match child.wait() {
                        Ok(status) if status.success() => return,
                        Ok(status) => format!("{name} failed, {status}"),
                        Err(e) => format!("{name} failed: {e}"),
                    };
                    let _ = sender.send(format!("{failure}; the SVG is {}", said.path));
                }
            });
        if let Err(e) = spawned {
            self.diagrams.svgs_drawing.fetch_sub(1, Ordering::SeqCst);
            self.message = Some(format!("Cannot draw the diagram as SVG: {e}"));
        }
    }

    /// Draw with `viewer` instead of the system viewer (for tests, which
    /// never start a real one).
    pub fn set_viewer(&mut self, viewer: impl Into<OsString>) {
        self.diagrams.viewer_program = viewer.into();
    }

    /// Draw pictures of diagrams with `renderer` instead of `mmdflux` (for
    /// tests that need a slow one).
    pub fn set_diagram_renderer(&mut self, renderer: Renderer) {
        self.diagrams.pictures.set_renderer(renderer);
    }

    /// Whether a picture of a diagram or an SVG for `o` is being drawn.
    pub fn drawing_busy(&self) -> bool {
        self.diagrams.pictures.busy() || self.diagrams.svgs_drawing.load(Ordering::SeqCst) > 0
    }

    /// Whether a picture of a diagram is being drawn.
    pub fn pictures_busy(&self) -> bool {
        self.diagrams.pictures.busy()
    }

    /// Block until the pictures of diagrams asked for are drawn.
    pub fn wait_for_pictures(&mut self) {
        self.diagrams.pictures.wait();
    }

    /// Whether a frame may show a diagram now: the diagram overlay or the
    /// Summary is open.
    pub fn shows_diagrams(&self) -> bool {
        self.diagram_open() || self.summary_open()
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
            room: self.diagram_room_windowed(),
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
        self.diagrams.pictures.tick();
        while let Ok(said) = self.diagrams.viewer.1.try_recv() {
            self.message = Some(said);
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
        if let Some(to) = self.density_key(key, self.diagrams.inline_density) {
            let to = match to {
                Some(Density::Minimap) => {
                    self.message = Some(NO_INLINE_MINIMAP.into());
                    return;
                }
                to => to,
            };
            let Some(to) = self.density_or_say(to) else {
                return;
            };
            let from = self.widest_summary_drawing();
            self.diagrams.inline_density = to;
            if !from.is_empty() {
                let view = self.summary_text_size();
                self.diagrams.inline = pan::recentre(&self.diagrams.inline, &from, view);
            }
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
            .filter_map(|mermaid| match self.summary_picture(&mermaid) {
                Picture::Ready(lines) | Picture::Drawing(Some(lines)) => Some(lines.to_vec()),
                _ => None,
            })
            .max_by_key(|lines| Size::of(lines).cols)
            .unwrap_or_default()
    }

    /// The cells inside the border of the Summary overlay.
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
    fn summary_picture(&self, mermaid: &str) -> Picture {
        let view = self.summary_text_size();
        let room = Room {
            width: view.cols,
            height: view.rows,
        };
        (self.diagrams.pictures).get(mermaid, room, self.diagrams.inline_density)
    }

    /// The density `+`, `-` or `=` asks for, from `now`: `None` when the key
    /// is not one of them (with Ctrl, none is), `Some(None)` past the last
    /// step.
    fn density_key(&self, key: KeyEvent, now: Density) -> Option<Option<Density>> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return None;
        }
        match key.code {
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
            Picture::Ready(lines) | Picture::Drawing(Some(lines)) => {
                let view = self.summary_text_size();
                let left = self.diagrams.inline.left(&lines, view);
                lines
                    .iter()
                    .map(|l| pan::slice(l, left, view.cols))
                    .collect()
            }
            Picture::Drawing(None) => vec![self.drawing_note()],
            Picture::Failed(e) => failure_lines(&e, mermaid),
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
            KeyCode::Char('r') if !ctrl => self.refresh_diagram(),
            KeyCode::Char('y') if !ctrl => {
                if let Some(source) = source {
                    self.copy_to_clipboard(&source, "the Mermaid source");
                }
            }
            KeyCode::Char('o') if !ctrl => match source {
                Some(source) => self.open_outside(&source),
                None => self.message = Some("The diagram has not come yet".into()),
            },
            KeyCode::Enter if self.diagrams.density == Density::Minimap => {
                self.diagrams.density = Density::Fit;
            }
            code => {
                if let Some(to) = self.density_key(key, self.diagrams.density) {
                    if let Some(to) = self.density_or_say(to) {
                        self.change_density(to);
                    }
                    return true;
                }
                // Nothing to move over while the first picture is drawn.
                if matches!(
                    self.open_state(self.diagrams.density),
                    Some(Picture::Drawing(None))
                ) {
                    return true;
                }
                let lines = self.open_lines();
                let view = self.diagram_view();
                // In the minimap a step moves the frame by a cell of it.
                let steps = match self.minimap_of(&lines) {
                    Some(minimap)
                        if matches!(
                            code,
                            KeyCode::Char('h' | 'j' | 'k' | 'l')
                                | KeyCode::Left
                                | KeyCode::Right
                                | KeyCode::Up
                                | KeyCode::Down
                        ) =>
                    {
                        minimap.scale.ceil() as usize
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
        let (old, old_view) = (
            self.open_picture(self.diagrams.density),
            self.diagram_view(),
        );
        change(&mut self.diagrams);
        if let Some(old) = old {
            self.diagrams.view = pan::recentre(&self.diagrams.view, &old, old_view);
        }
    }

    /// Lay the open picture out at `to`, keeping the centre of the view.
    fn change_density(&mut self, to: Density) {
        let from = self.diagrams.density;
        if drawn_as(from) != drawn_as(to) {
            if let Some(old) = self.open_picture(from) {
                self.diagrams.view = pan::recentre(&self.diagrams.view, &old, self.diagram_view());
            }
        }
        self.diagrams.density = to;
    }

    /// The minimap of `lines` with the frame of the view, when the overlay
    /// shows the minimap.
    fn minimap_of(&self, lines: &[String]) -> Option<crate::diagram::Minimap> {
        if self.diagrams.density != Density::Minimap {
            return None;
        }
        let view = self.diagram_view();
        let pan = &self.diagrams.view;
        Some(crate::diagram::minimap(
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

    /// The cells the diagram overlay shows its picture in.
    fn diagram_view(&self) -> Size {
        let text = self.diagram_place().text;
        Size {
            cols: text.width as usize,
            rows: text.height as usize,
        }
    }

    /// The room of the overlay in its window, whether or not it is full
    /// screen now: what a diagram is asked for.
    fn diagram_room_windowed(&self) -> Room {
        let area = self.overlay_area();
        Room {
            width: scroll::text_cols(area),
            height: scroll::text_rows(area),
        }
    }

    /// [`Self::diagram_view`] as the room a picture is fitted to.
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

    /// The diagram overlay: in its box, or on the whole terminal.
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
        let title = if self.diagrams.pictures.busy() {
            format!(
                "{title}{} drawing… ",
                super::analysis::spinner(self.clock())
            )
        } else {
            title
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
        match self.minimap_of(&lines) {
            Some(minimap) => self.render_minimap(buf, &place, &minimap),
            None => pan::render(self.theme(), buf, &place, &lines, &self.diagrams.view),
        }
    }

    /// The minimap in the overlay: dots, the frame of the view lit, and what
    /// the keys do on the border.
    fn render_minimap(
        &self,
        buf: &mut Buffer,
        place: &pan::Place,
        minimap: &crate::diagram::Minimap,
    ) {
        let theme = self.theme();
        let text = place.text;
        for (i, line) in minimap.lines.iter().enumerate().take(text.height as usize) {
            buf.set_stringn(
                text.x,
                text.y + i as u16,
                line,
                text.width as usize,
                Style::new().fg(theme.fg),
            );
        }
        let (x, y, w, h) = minimap.frame;
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

    /// The lines of the picture of `mermaid` in the overlay at `density`:
    /// the picture, the one shown before while it is drawn, or why it could
    /// not be drawn.
    fn picture_lines(&self, mermaid: &str, density: Density) -> Vec<String> {
        match (self.diagrams.pictures).get(mermaid, self.diagram_room(), density) {
            Picture::Ready(lines) | Picture::Drawing(Some(lines)) => lines.to_vec(),
            Picture::Drawing(None) => vec![self.drawing_note()],
            Picture::Failed(e) => failure_lines(&e, mermaid),
        }
    }

    /// The picture the overlay shows at `density`, if it shows one (and not
    /// a note that it is being drawn or why it could not be).
    fn open_picture(&self, density: Density) -> Option<Rc<Vec<String>>> {
        match self.open_state(density)? {
            Picture::Ready(lines) | Picture::Drawing(Some(lines)) => Some(lines),
            _ => None,
        }
    }

    /// The picture of the diagram the overlay is open on, at `density`, if
    /// its Mermaid text came.
    fn open_state(&self, density: Density) -> Option<Picture> {
        let mermaid = match &self.diagrams.summary_full {
            Some(mermaid) => mermaid,
            None => self.diagrams.cache.get(self.diagrams.open.as_ref()?)?,
        };
        Some((self.diagrams.pictures).get(mermaid, self.diagram_room(), drawn_as(density)))
    }

    /// What is shown while a picture is drawn and none was before.
    fn drawing_note(&self) -> String {
        format!(
            "{} Drawing the diagram…",
            super::analysis::spinner(self.clock())
        )
    }

    /// The lines the open overlay shows; in the minimap, those it is made of.
    pub(super) fn open_lines(&self) -> Vec<String> {
        self.open_lines_at(self.diagrams.density)
    }

    /// The lines the open overlay shows at `density`.
    fn open_lines_at(&self, density: Density) -> Vec<String> {
        let density = drawn_as(density);
        if let Some(mermaid) = &self.diagrams.summary_full {
            return self.picture_lines(mermaid, density);
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
            Some(mermaid) => self.picture_lines(mermaid, density),
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

/// What `o` came to on its thread: what to say, and the viewer started on
/// the file, if it was.
struct Shown {
    message: String,
    child: Option<std::process::Child>,
    path: String,
}

/// Draw `mermaid` as SVG, write it and start `viewer` on it.
fn show_outside(mermaid: &str, viewer: &OsString, svgs: &Svgs) -> Shown {
    let failed = |message: String| Shown {
        message,
        child: None,
        path: String::new(),
    };
    let svg = match crate::diagram::svg(mermaid) {
        Ok(svg) => svg,
        Err(e) => return failed(format!("Cannot draw the diagram as SVG: {e}")),
    };
    let path = match write_svg(&svg) {
        Ok(path) => path,
        Err(e) => return failed(format!("Cannot write the SVG of the diagram: {e}")),
    };
    svgs.add(path.clone());
    let path = match write_page(&path) {
        Ok(page) => page,
        Err(e) => return failed(format!("Cannot write the page of the diagram: {e}")),
    };
    svgs.add(path.clone());
    let shown = path.display().to_string();
    let name = viewer.to_string_lossy().into_owned();
    let child = std::process::Command::new(viewer)
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match child {
        Ok(child) => Shown {
            message: format!("Opened in the viewer: {shown}"),
            child: Some(child),
            path: shown,
        },
        Err(e) => {
            let why = match e.kind() {
                std::io::ErrorKind::NotFound => format!("{name} is not installed"),
                _ => format!("{name}: {e}"),
            };
            failed(format!(
                "Cannot open the diagram: {why}; the SVG is {shown}"
            ))
        }
    }
}

/// Write `svg` to a temporary file that outlives `o`: the viewer reads it
/// after `o` returns ([`Svgs`] removes it in time).
fn write_svg(svg: &str) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;
    let mut file = tempfile::Builder::new()
        .prefix(SVG_NAME.0)
        .suffix(SVG_NAME.1)
        .tempfile()?;
    file.write_all(svg.as_bytes())?;
    let (_, path) = file.keep().map_err(|e| e.error)?;
    Ok(path)
}

/// The page `o` hands the viewer, beside the SVG at `svg`: the SVG is shown
/// as an image (`<img>`), and a browser never runs a script of an image or
/// loads anything it points to; the page's own policy allows nothing but
/// local images besides. Whatever got past [`crate::diagram::check_svg`]
/// stays inert.
fn write_page(svg: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let name = svg.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        return Err(std::io::Error::other("the SVG has an unusual name"));
    }
    let page = svg.with_extension("html");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&page)?;
    write!(file, "{}", page_of(name))?;
    Ok(page)
}

/// The page that shows the SVG named `name` beside it as an image.
fn page_of(name: &str) -> String {
    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{SVG_PAGE_POLICY}\">\n\
         <title>moonhare: diagram</title>\n\
         <style>body {{ margin: 1em; background: #fff; }} img {{ max-width: 100%; }}</style>\n\
         </head><body><img src=\"{name}\" alt=\"diagram\"></body></html>\n"
    )
}

/// What the page of a diagram may load: local images, its own style.
const SVG_PAGE_POLICY: &str = "default-src 'none'; img-src file: data:; style-src 'unsafe-inline'";

/// The density a picture is drawn at: the minimap is made of the fitted one.
fn drawn_as(density: Density) -> Density {
    match density {
        Density::Minimap => Density::Fit,
        other => other,
    }
}

/// What `-` says in the Summary past its densest layout.
const NO_INLINE_MINIMAP: &str = "No minimap in the Summary: f has one (-, -)";

/// What `r` says in an Explanation or a Diagram with no Analyst.
pub(super) const NO_ANALYST_TO_REFRESH: &str = "No Analyst is configured: nothing to ask again";

/// What is shown instead of a picture that could not be drawn.
fn failure_lines(error: &str, mermaid: &str) -> Vec<String> {
    format!("Cannot draw the diagram: {error}\n\n{mermaid}")
        .lines()
        .map(crate::diagram::sanitize)
        .collect()
}

/// The version of the diagram's prompt: a part of its key, so a diagram
/// asked for with an older prompt is asked again, while Summaries, Groups and
/// Explanations kept on the disk stay valid. Bump it when that prompt changes.
const DIAGRAM_PROMPT: &str = "p2";

/// A diagram depends on the Range, the language, what it is drawn for and the
/// version of its prompt.
fn diagram_cache_key(request: &DiagramRequest) -> String {
    diagram_cache_key_of(request, DIAGRAM_PROMPT)
}

/// [`diagram_cache_key`] for the prompt of version `prompt`; before there
/// was one, `""`.
fn diagram_cache_key_of(request: &DiagramRequest, prompt: &str) -> String {
    let subject = match &request.subject {
        DiagramSubject::File { path } => format!("file\u{2}{path}"),
        DiagramSubject::Fragment { path, line } => format!("fragment\u{2}{path}\u{2}{line}"),
        DiagramSubject::Group { name, files } => {
            format!("group\u{2}{name}\u{2}{}", files.join("\u{3}"))
        }
    };
    let mut parts = vec![
        request.base.as_str(),
        &request.head,
        &request.language,
        &subject,
    ];
    if !prompt.is_empty() {
        parts.insert(0, prompt);
    }
    AnalystCache::key("diagram", &parts)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyst::Diagram;

    #[test]
    fn a_picture_too_slow_to_lay_out_comes_once_its_layout_is_drawn() {
        let renderer: Renderer = Arc::new(|source: &str, _| {
            std::thread::sleep(Duration::from_millis(300));
            crate::diagram::draw(source).map(|lines| lines.join("\n"))
        });
        let mut pictures = Pictures::default();
        pictures.set_renderer(renderer);
        pictures.budget = Duration::from_millis(30);
        let mermaid = "graph TD\n    A[Collect] --> B[Render]";
        let room = Room {
            width: 80,
            height: 20,
        };

        assert!(matches!(
            pictures.get(mermaid, room, Density::Fit),
            Picture::Drawing(None)
        ));
        while pictures.running.get().is_some() {
            pictures.tick();
            std::thread::sleep(Duration::from_millis(5));
        }
        match pictures.get(mermaid, room, Density::Fit) {
            Picture::Failed(e) => assert_eq!(e, crate::diagram::TOO_SLOW),
            _ => panic!("not too slow"),
        }
        // The layout given up on is still drawn: until it comes, the
        // pictures are busy, and then the picture is laid out again.
        assert!(pictures.busy());
        pictures.wait();
        let lines = match pictures.get(mermaid, room, Density::Fit) {
            Picture::Ready(lines) => lines,
            Picture::Drawing(_) => {
                pictures.wait();
                match pictures.get(mermaid, room, Density::Fit) {
                    Picture::Ready(lines) => lines,
                    _ => panic!("not drawn"),
                }
            }
            Picture::Failed(e) => panic!("still failed: {e}"),
        };
        assert!(lines.iter().any(|l| l.contains("Collect")), "{lines:?}");
    }

    #[test]
    fn a_diagram_kept_from_the_older_prompt_is_asked_again() {
        let dir = tempfile::tempdir().unwrap();
        let cache = AnalystCache::new(dir.path());
        let request = DiagramRequest {
            repo: dir.path().to_path_buf(),
            base: "base".into(),
            head: "head".into(),
            language: "en".into(),
            subject: DiagramSubject::File { path: "a".into() },
            settings: Default::default(),
            room: Room {
                width: 80,
                height: 24,
            },
        };
        let old = Diagram {
            mermaid: "graph LR\n A --> B".into(),
        };
        // Under the key a diagram had before the prompt was versioned.
        cache.store_diagram(&diagram_cache_key_of(&request, ""), &old);
        assert!(cache.load_diagram(&diagram_cache_key(&request)).is_none());

        cache.store_diagram(&diagram_cache_key(&request), &old);
        assert_eq!(cache.load_diagram(&diagram_cache_key(&request)), Some(old));
    }
}

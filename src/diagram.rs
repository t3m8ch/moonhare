//! Drawing Mermaid text as box-drawing characters. `mmdflux` does the work;
//! nothing here uses images or graphics protocols of the terminal.
//!
//! The text comes from an LLM that read code under review, so it is
//! untrusted: it is refused when long, drawn without the statements that
//! style or script it ([`without_styling`]), rendered on threads of their own
//! that are given up on after a while, and a crash of the renderer costs the
//! picture, not the session. An SVG with anything active in it is not opened
//! ([`check_svg`]).
//!
//! A diagram is fitted to the room it is shown in ([`draw_in`]): `mmdflux`
//! decides the width from the graph alone, so several layouts are tried, from
//! the diagram as written to denser ones ([`candidates`]), and the first that
//! fits is shown. The layouts are made by rewriting the Mermaid text that is
//! drawn (a flowchart turned top-down, long labels wrapped, long messages of a
//! sequence diagram shortened) and by closing up the gaps of the picture; the
//! Mermaid source the Reviewer copies is never changed. The layouts drawn are
//! kept ([`Drawings`]), so a resize or another density lays a diagram out
//! again without drawing it again; the session lays pictures out in the
//! background and a frame never waits for one.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use mmdflux::{render_diagram, OutputFormat, RenderConfig};
use unicode_width::UnicodeWidthChar;

/// Longest Mermaid source that is drawn, in bytes.
pub const MAX_SOURCE: usize = 20_000;

/// How long the renderer may take before it is given up on.
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(5);

/// The name of the renderer's thread, whose panics are kept quiet.
const RENDER_THREAD: &str = "moonhare-render";

/// The cells a picture is shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Room {
    /// Columns.
    pub width: usize,
    /// Rows.
    pub height: usize,
}

/// How densely a diagram is laid out: the zoom of a terminal, whose cells
/// cannot shrink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Density {
    /// The whole diagram in Braille dots, without text: see [`minimap`].
    Minimap,
    /// Gaps closed up and labels wrapped short, whatever the room.
    Compact,
    /// The first layout that fits the room.
    #[default]
    Fit,
    /// The diagram as written, labels whole, gaps widened.
    Roomy,
}

impl Density {
    /// One step denser (`-`), if there is one.
    pub fn denser(self) -> Option<Density> {
        match self {
            Density::Minimap => None,
            Density::Compact => Some(Density::Minimap),
            Density::Fit => Some(Density::Compact),
            Density::Roomy => Some(Density::Fit),
        }
    }

    /// One step roomier (`+`), if there is one.
    pub fn roomier(self) -> Option<Density> {
        match self {
            Density::Minimap => Some(Density::Compact),
            Density::Compact => Some(Density::Fit),
            Density::Fit => Some(Density::Roomy),
            Density::Roomy => None,
        }
    }
}

/// What is done to the gaps of a drawn picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gaps {
    /// Left as `mmdflux` drew them.
    AsDrawn,
    /// Runs of blank or line-only columns and rows cut down to this many.
    Squeeze(usize),
    /// Those runs of columns made twice as long.
    Stretch,
}

/// One layout of a diagram: the Mermaid text that is drawn and what is done
/// to the picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The Mermaid text handed to `mmdflux`, rewritten to fit.
    pub source: String,
    /// Labels of edges are not wrapped at all (`mmdflux` wraps long ones).
    pub whole_labels: bool,
    /// What is done to the gaps of the drawn picture.
    pub gaps: Gaps,
}

impl Candidate {
    fn new(source: &str, gaps: Gaps) -> Candidate {
        Candidate {
            source: source.to_string(),
            whole_labels: false,
            gaps,
        }
    }
}

/// What draws one Mermaid text as text: `mmdflux`, or a stand-in in tests.
/// The flag asks for the labels of edges whole, not wrapped.
pub type Renderer = Arc<dyn Fn(&str, bool) -> Result<String, String> + Send + Sync>;

/// `mmdflux`, drawing as text.
fn mmdflux_text(source: &str, whole_labels: bool) -> Result<String, String> {
    let mut config = RenderConfig::default();
    if whole_labels {
        config.layout.edge_label_max_width = None;
    }
    render_diagram(source, OutputFormat::Text, &config).map_err(|e| e.to_string())
}

/// The diagram as lines of text, as `mmdflux` draws it, or why it could not
/// be drawn.
pub fn draw(mermaid: &str) -> Result<Vec<String>, String> {
    draw_with(mermaid, |text| mmdflux_text(text, false))
}

/// [`draw`] with another renderer.
pub fn draw_with(
    mermaid: &str,
    render: impl FnOnce(&str) -> Result<String, String> + Send + 'static,
) -> Result<Vec<String>, String> {
    sandboxed(&without_styling(mermaid), RENDER_TIMEOUT, move |text| {
        render(&text).map(|text| text.lines().map(|l| sanitize(l.trim_end())).collect())
    })
}

/// The diagram laid out for `room` at `density` (see [`Density`]): the
/// first of the [`candidates`] that fits the width and the height, else the
/// shortest that fits the width, else the narrowest; [`Density::Compact`]
/// takes the narrowest of its own. [`Density::Minimap`] is
/// drawn as [`Density::Fit`]; [`minimap`] makes dots of that. Here and now,
/// within [`RENDER_TIMEOUT`]: see [`Drawings`] for drawing in the background.
pub fn draw_in(mermaid: &str, room: Room, density: Density) -> Result<Vec<String>, String> {
    Drawings::default().fit(mermaid, room, density, RENDER_TIMEOUT)
}

/// The diagram as SVG, for a viewer outside the terminal: drawn without the
/// statements that style or script it ([`without_styling`]), and refused
/// when the picture has anything active in it all the same ([`check_svg`]).
pub fn svg(mermaid: &str) -> Result<String, String> {
    let svg = sandboxed(&without_styling(mermaid), RENDER_TIMEOUT, |text| {
        render_diagram(&text, OutputFormat::Svg, &RenderConfig::default())
            .map_err(|e| sanitize(&e.to_string()))
    })?;
    check_svg(&svg)?;
    Ok(svg)
}

/// Run `work` on the Mermaid text on a thread of its own, given up on after
/// `budget`; a text over [`MAX_SOURCE`] is refused.
fn sandboxed<T: Send + 'static>(
    mermaid: &str,
    budget: Duration,
    work: impl FnOnce(String) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    too_long(mermaid)?;
    silence_renderer_panics();
    let text = mermaid.to_string();
    let (sender, receiver) = channel();
    std::thread::Builder::new()
        .name(RENDER_THREAD.into())
        .spawn(move || {
            let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(text)));
            let _ = sender.send(done);
        })
        .map_err(|e| format!("cannot start the renderer: {e}"))?;
    match receiver.recv_timeout(budget) {
        Ok(Ok(Ok(done))) => Ok(done),
        Ok(Ok(Err(e))) => Err(sanitize(&e)),
        Ok(Err(_)) => Err(CRASHED.into()),
        Err(_) => Err(TOO_SLOW.into()),
    }
}

/// What a renderer that panicked costs.
const CRASHED: &str = "the renderer crashed on this diagram";

/// What a diagram that took too long to draw shows.
const TOO_SLOW: &str = "the diagram took too long to draw";

/// A text over [`MAX_SOURCE`] is not drawn.
fn too_long(mermaid: &str) -> Result<(), String> {
    if mermaid.len() > MAX_SOURCE {
        return Err(format!(
            "the source is too long to draw ({} bytes, the limit is {MAX_SOURCE})",
            mermaid.len()
        ));
    }
    Ok(())
}

/// Layouts drawn are kept, up to this many, for every room and density.
const DRAWINGS: usize = 256;

/// What a layout is kept under: its Mermaid text (hashed) and whether its
/// labels are whole.
type DrawingKey = (u64, bool);

/// The drawing of one layout, as far as [`Drawings`] got.
enum Got {
    Lines(Arc<Vec<String>>),
    Failed(String),
    /// Not drawn in time (or still being drawn for an earlier request).
    Skipped,
    /// Not drawn yet, and not to be drawn now.
    Missing,
}

/// The layouts of diagrams drawn so far, by the text `mmdflux` was given,
/// shared by every room and density: a resize or another density lays a
/// diagram out again from layouts already drawn. A layout is drawn on a
/// thread of its own; one that takes too long is given up on, but is kept
/// when it is done all the same, and is not drawn twice at once.
#[derive(Clone)]
pub struct Drawings {
    renderer: Renderer,
    shared: Arc<Mutex<Shared>>,
}

/// What [`Drawings`] keep between threads.
#[derive(Default)]
struct Shared {
    /// Layouts drawn, or why they could not be.
    drawn: HashMap<DrawingKey, Result<Arc<Vec<String>>, String>>,
    /// Layouts being drawn now.
    drawing: HashSet<DrawingKey>,
}

impl Default for Drawings {
    fn default() -> Self {
        Drawings::with(Arc::new(mmdflux_text))
    }
}

impl Drawings {
    /// Drawings made by `renderer` instead of `mmdflux`.
    pub fn with(renderer: Renderer) -> Drawings {
        Drawings {
            renderer,
            shared: Arc::default(),
        }
    }

    /// The diagram laid out for `room` at `density`, if every layout that
    /// takes is drawn already: what a frame can show without waiting.
    pub fn fit_now(
        &self,
        mermaid: &str,
        room: Room,
        density: Density,
    ) -> Option<Result<Vec<String>, String>> {
        choose(mermaid, room, density, |candidate| {
            match self.kept(candidate) {
                Some(Ok(lines)) => Got::Lines(lines),
                Some(Err(e)) => Got::Failed(e),
                None => Got::Missing,
            }
        })
    }

    /// The diagram laid out for `room` at `density` (see [`draw_in`]),
    /// drawing the layouts missing within `budget`. The layouts not drawn
    /// in time are passed over: the best of those drawn is taken, and only
    /// when none is there is the diagram too slow to draw.
    pub fn fit(
        &self,
        mermaid: &str,
        room: Room,
        density: Density,
        budget: Duration,
    ) -> Result<Vec<String>, String> {
        let deadline = Instant::now() + budget;
        choose(mermaid, room, density, |candidate| {
            let left = deadline.saturating_duration_since(Instant::now());
            self.draw(candidate, left)
        })
        .unwrap_or_else(|| Err(TOO_SLOW.into()))
    }

    /// [`Self::fit`] within [`RENDER_TIMEOUT`] on a thread of its own; `done`
    /// gets the picture there.
    pub fn fit_later(
        &self,
        mermaid: String,
        room: Room,
        density: Density,
        done: impl FnOnce(Result<Vec<String>, String>) + Send + 'static,
    ) {
        silence_renderer_panics();
        let drawings = self.clone();
        // `done` is called once: on the thread, or here if it cannot start.
        let slot = Arc::new(Mutex::new(Some(done)));
        let on_thread = slot.clone();
        let spawned = std::thread::Builder::new()
            .name(RENDER_THREAD.into())
            .spawn(move || {
                let picture = drawings.fit(&mermaid, room, density, RENDER_TIMEOUT);
                let done = on_thread.lock().unwrap_or_else(|e| e.into_inner()).take();
                if let Some(done) = done {
                    done(picture);
                }
            });
        if let Err(e) = spawned {
            let done = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(done) = done {
                done(Err(format!("cannot start the renderer: {e}")));
            }
        }
    }

    /// The layout of `candidate` drawn before, or why it could not be.
    fn kept(&self, candidate: &Candidate) -> Option<Result<Arc<Vec<String>>, String>> {
        let shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        shared.drawn.get(&drawing_key(candidate)).cloned()
    }

    /// The layout of `candidate`, drawn unless it was before, waiting for it
    /// no longer than `budget`.
    fn draw(&self, candidate: &Candidate, budget: Duration) -> Got {
        let key = drawing_key(candidate);
        {
            let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            match shared.drawn.get(&key) {
                Some(Ok(lines)) => return Got::Lines(lines.clone()),
                Some(Err(e)) => return Got::Failed(e.clone()),
                None => {}
            }
            if budget.is_zero() || !shared.drawing.insert(key) {
                return Got::Skipped;
            }
        }
        silence_renderer_panics();
        let (sender, receiver) = channel();
        let (renderer, shared) = (self.renderer.clone(), self.shared.clone());
        let (source, whole) = (candidate.source.clone(), candidate.whole_labels);
        let spawned = std::thread::Builder::new()
            .name(RENDER_THREAD.into())
            .spawn(move || {
                let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    renderer(&source, whole)
                }));
                let done = match done {
                    Ok(Ok(text)) => Ok(Arc::new(
                        text.lines().map(|l| sanitize(l.trim_end())).collect(),
                    )),
                    Ok(Err(e)) => Err(sanitize(&e)),
                    Err(_) => Err(CRASHED.to_string()),
                };
                let mut kept = shared.lock().unwrap_or_else(|e| e.into_inner());
                kept.drawing.remove(&key);
                if kept.drawn.len() >= DRAWINGS {
                    kept.drawn.clear();
                }
                kept.drawn.insert(key, done.clone());
                drop(kept);
                let _ = sender.send(done);
            });
        if let Err(e) = spawned {
            let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            shared.drawing.remove(&key);
            return Got::Failed(format!("cannot start the renderer: {e}"));
        }
        match receiver.recv_timeout(budget) {
            Ok(Ok(lines)) => Got::Lines(lines),
            Ok(Err(e)) => Got::Failed(e),
            Err(_) => Got::Skipped,
        }
    }
}

/// What the drawing of `candidate` is kept under.
fn drawing_key(candidate: &Candidate) -> DrawingKey {
    let mut hasher = DefaultHasher::new();
    candidate.source.hash(&mut hasher);
    (hasher.finish(), candidate.whole_labels)
}

/// The layout of `mermaid` for `room` at `density` (see [`draw_in`]) from
/// the drawings `get` gives; `None` as soon as one is [`Got::Missing`].
fn choose(
    mermaid: &str,
    room: Room,
    density: Density,
    mut get: impl FnMut(&Candidate) -> Got,
) -> Option<Result<Vec<String>, String>> {
    if let Err(e) = too_long(mermaid) {
        return Some(Err(e));
    }
    let mermaid = without_styling(mermaid);
    let mut error = None;
    let mut skipped = false;
    let mut fits_width = None;
    let mut narrowest: Option<(usize, usize, Vec<String>)> = None;
    for candidate in candidates(&mermaid, density) {
        let drawn = match get(&candidate) {
            Got::Lines(lines) => lines,
            Got::Failed(e) => {
                error.get_or_insert(e);
                continue;
            }
            Got::Skipped => {
                skipped = true;
                continue;
            }
            Got::Missing => return None,
        };
        let lines = with_gaps(&drawn, candidate.gaps);
        let (width, height) = (width_of(&lines), lines.len());
        if density != Density::Compact && width <= room.width {
            if height <= room.height {
                return Some(Ok(lines));
            }
            if fits_width
                .as_ref()
                .is_none_or(|shortest: &Vec<String>| height < shortest.len())
            {
                fits_width = Some(lines.clone());
            }
        }
        if narrowest
            .as_ref()
            .is_none_or(|(w, h, _)| (width, height) < (*w, *h))
        {
            narrowest = Some((width, height, lines));
        }
    }
    Some(
        fits_width
            .or(narrowest.map(|(.., lines)| lines))
            .ok_or_else(|| match (error, skipped) {
                (Some(e), _) => e,
                (None, true) => TOO_SLOW.to_string(),
                (None, false) => "nothing to draw".into(),
            }),
    )
}

/// The layouts tried for `mermaid` at `density`, the diagram as written first.
pub fn candidates(mermaid: &str, density: Density) -> Vec<Candidate> {
    if density == Density::Roomy {
        return vec![Candidate {
            source: mermaid.to_string(),
            whole_labels: true,
            gaps: Gaps::Stretch,
        }];
    }
    let compact = density == Density::Compact;
    let mut list = Vec::new();
    match mmdflux::detect_diagram(mermaid) {
        Some("flowchart") => {
            let turned = turned_top_down(mermaid);
            let upright = turned.as_deref().unwrap_or(mermaid);
            if !compact {
                list.push(Candidate::new(mermaid, Gaps::AsDrawn));
                list.push(Candidate::new(mermaid, Gaps::Squeeze(2)));
                if let Some(turned) = &turned {
                    list.push(Candidate::new(turned, Gaps::AsDrawn));
                    list.push(Candidate::new(turned, Gaps::Squeeze(2)));
                }
                list.push(Candidate::new(&wrap_labels(upright, 24), Gaps::Squeeze(2)));
            }
            for width in [16, 10] {
                list.push(Candidate::new(
                    &wrap_labels(upright, width),
                    Gaps::Squeeze(1),
                ));
            }
        }
        Some("sequence") => {
            if !compact {
                list.push(Candidate::new(mermaid, Gaps::AsDrawn));
                list.push(Candidate::new(mermaid, Gaps::Squeeze(2)));
                list.push(Candidate::new(
                    &shorten_messages(mermaid, 32),
                    Gaps::Squeeze(1),
                ));
            }
            for width in [20, 12] {
                list.push(Candidate::new(
                    &shorten_messages(mermaid, width),
                    Gaps::Squeeze(1),
                ));
            }
        }
        _ => {
            if !compact {
                list.push(Candidate::new(mermaid, Gaps::AsDrawn));
                list.push(Candidate::new(mermaid, Gaps::Squeeze(2)));
            }
            list.push(Candidate::new(mermaid, Gaps::Squeeze(1)));
        }
    }
    list.dedup();
    list
}

/// Draw one layout, here and now: no thread, no time limit.
pub fn render(candidate: &Candidate) -> Result<Vec<String>, String> {
    let text = mmdflux_text(&candidate.source, candidate.whole_labels)?;
    let lines: Vec<String> = text.lines().map(|l| sanitize(l.trim_end())).collect();
    Ok(with_gaps(&lines, candidate.gaps))
}

/// `lines` with their gaps as `gaps` says.
fn with_gaps(lines: &[String], gaps: Gaps) -> Vec<String> {
    match gaps {
        Gaps::AsDrawn => lines.to_vec(),
        Gaps::Squeeze(keep) => regap(lines, Some(keep.max(1))),
        Gaps::Stretch => regap(lines, None),
    }
}

/// The width in cells of the widest of `lines`.
fn width_of(lines: &[String]) -> usize {
    lines
        .iter()
        .map(|l| unicode_width::UnicodeWidthStr::width(l.as_str()))
        .max()
        .unwrap_or(0)
}

/// The first line of the Mermaid text that is not blank, a comment or front
/// matter: where the kind and the direction of the diagram are.
fn header_index(lines: &[&str]) -> Option<usize> {
    let mut in_front_matter = false;
    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line == "---" {
            in_front_matter = !in_front_matter;
            continue;
        }
        if in_front_matter || line.is_empty() || line.starts_with("%%") {
            continue;
        }
        return Some(i);
    }
    None
}

/// A left-to-right (or right-to-left) flowchart drawn top-down; `None` when
/// it already is.
fn turned_top_down(mermaid: &str) -> Option<String> {
    let mut lines: Vec<&str> = mermaid.lines().collect();
    let at = header_index(&lines)?;
    let header = lines[at];
    let mut words = header.split_whitespace();
    let kind = words.next()?;
    let direction = words.next()?;
    let direction = direction.trim_end_matches(';');
    if !matches!(kind, "graph" | "flowchart") || !matches!(direction, "LR" | "RL") {
        return None;
    }
    let indent = &header[..header.len() - header.trim_start().len()];
    let rest: Vec<&str> = words.collect();
    let turned = format!("{indent}{kind} TD {}", rest.join(" "));
    lines[at] = turned.trim_end();
    let mut text = lines.join("\n");
    if mermaid.ends_with('\n') {
        text.push('\n');
    }
    Some(text)
}

/// Statements of a flowchart that carry no labels to wrap.
const NO_LABELS: &[&str] = &[
    "%%",
    "classDef",
    "class ",
    "style",
    "linkStyle",
    "click",
    "direction",
    "subgraph",
    "end",
];

/// The flowchart with the labels of its nodes (`[…]`, `(…)`, `{…}`) and edges
/// (`|…|`) longer than `width` broken into lines with `<br/>`.
fn wrap_labels(mermaid: &str, width: usize) -> String {
    let lines: Vec<&str> = mermaid.lines().collect();
    let header = header_index(&lines);
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let statement = line.trim_start();
            if Some(i) <= header || NO_LABELS.iter().any(|w| statement.starts_with(w)) {
                line.to_string()
            } else {
                wrap_line(line, width)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One statement of a flowchart with its labels wrapped at `width`.
fn wrap_line(line: &str, width: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        out.push(c);
        i += 1;
        let close: &[char] = match c {
            '[' | '(' | '{' => &[']', ')', '}'],
            '|' => &['|'],
            _ => continue,
        };
        // The rest of a run of openers, as in `((`, `([`, `{{`.
        if c != '|' {
            while i < chars.len() && matches!(chars[i], '[' | '(' | '{') {
                out.push(chars[i]);
                i += 1;
            }
        }
        let (quote, stop): (bool, &[char]) = if chars.get(i) == Some(&'"') {
            out.push('"');
            i += 1;
            (true, &['"'])
        } else {
            (false, close)
        };
        let Some(len) = chars[i..].iter().position(|ch| stop.contains(ch)) else {
            out.extend(&chars[i..]);
            break;
        };
        let label: String = chars[i..i + len].iter().collect();
        out.push_str(&wrap_words(&label, width));
        i += len;
        if quote {
            out.push('"');
            i += 1;
        } else if c == '|' {
            out.push('|');
            i += 1;
        }
    }
    out
}

/// `label` broken with `<br/>` into lines of at most `width` characters, at
/// spaces; one that already has markup is left alone.
fn wrap_words(label: &str, width: usize) -> String {
    if label.chars().count() <= width || label.contains('<') {
        return label.to_string();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in label.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    lines.push(line);
    lines.join("<br/>")
}

/// Statements of a sequence diagram whose text after `:` is not a message.
const NOT_MESSAGES: &[&str] = &[
    "%%",
    "participant",
    "actor",
    "autonumber",
    "title",
    "box",
    "create",
    "destroy",
    "activate",
    "deactivate",
];

/// The sequence diagram with its messages, notes and participant names
/// longer than `width` cut short with `…`: `mmdflux` draws a message on one
/// line, and the longest one sets the gap between all participants.
fn shorten_messages(mermaid: &str, width: usize) -> String {
    let lines: Vec<&str> = mermaid.lines().collect();
    let header = header_index(&lines);
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let statement = line.trim_start();
            if Some(i) <= header {
                return line.to_string();
            }
            if statement.starts_with("participant") || statement.starts_with("actor") {
                return match line.split_once(" as ") {
                    Some((head, name)) => format!("{head} as {}", cut(name.trim(), width)),
                    None => line.to_string(),
                };
            }
            if NOT_MESSAGES.iter().any(|w| statement.starts_with(w)) {
                return line.to_string();
            }
            match line.split_once(':') {
                Some((head, text)) => format!("{head}: {}", cut(text.trim(), width)),
                None => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `text` cut to `width` characters, the last of them `…`.
fn cut(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut short: String = text.chars().take(width.saturating_sub(1)).collect();
    short.truncate(short.trim_end().len());
    short.push('…');
    short
}

/// Characters a column may hold and still be a gap: blank or a line across.
const ACROSS: &[char] = &[' ', '─', '━', '┄', '┅', '┈', '┉', '╌', '╍', '═'];

/// Characters a row may hold and still be a gap: blank or a line down.
const DOWN: &[char] = &[' ', '│', '┃', '┆', '┇', '┊', '┋', '╎', '╏', '║'];

/// The picture with every run of gap columns and rows (each the same as the
/// one before it, holding nothing but blanks and lines) cut down to `keep`
/// (counting the one the run repeats), or, when `keep` is `None`, the runs
/// of columns doubled.
/// Nothing but the length of lines and gaps changes. A picture with a
/// character wider or narrower than a cell is left as it is.
fn regap(lines: &[String], keep: Option<usize>) -> Vec<String> {
    let grid: Vec<Vec<char>> = lines.iter().map(|l| l.chars().collect()).collect();
    if grid.iter().flatten().any(|c| c.width() != Some(1)) {
        return lines.to_vec();
    }
    let width = grid.iter().map(Vec::len).max().unwrap_or(0);
    let cell = |row: &Vec<char>, x: usize| row.get(x).copied().unwrap_or(' ');
    let column_repeats = |x: usize| {
        x > 0
            && grid
                .iter()
                .all(|row| ACROSS.contains(&cell(row, x)) && cell(row, x) == cell(row, x - 1))
    };
    let columns = copies(width, keep, column_repeats);
    let row_repeats = |y: usize| {
        y > 0
            && (0..width).all(|x| {
                DOWN.contains(&cell(&grid[y], x)) && cell(&grid[y], x) == cell(&grid[y - 1], x)
            })
    };
    // Rows are not stretched: a picture taller by half its gaps is only
    // longer to scroll through.
    let rows = match keep {
        Some(_) => copies(grid.len(), keep, row_repeats),
        None => vec![1; grid.len()],
    };
    let mut out = Vec::new();
    for (y, row) in grid.iter().enumerate() {
        let line: String = (0..width)
            .flat_map(|x| std::iter::repeat_n(cell(row, x), columns[x]))
            .collect();
        let line = line.trim_end().to_string();
        out.extend(std::iter::repeat_n(line, rows[y]));
    }
    out
}

/// How many times each of `len` columns (or rows) is kept: one that
/// `repeats` the one before it is dropped past `keep` in a run, or doubled.
fn copies(len: usize, keep: Option<usize>, repeats: impl Fn(usize) -> bool) -> Vec<usize> {
    let mut run = 1;
    (0..len)
        .map(|i| {
            if !repeats(i) {
                run = 1;
                return 1;
            }
            run += 1;
            match keep {
                Some(keep) => usize::from(run <= keep),
                None => 2,
            }
        })
        .collect()
}

/// A picture shrunk to fit a room, in Braille dots: the shape of the boxes
/// and the lines between them, no text.
#[derive(Debug, Clone, PartialEq)]
pub struct Minimap {
    /// The dots, a line for each row of the room.
    pub lines: Vec<String>,
    /// Where the part of the picture that `view` names is: the first column
    /// and row in cells, the columns and rows.
    pub frame: (usize, usize, usize, usize),
    /// Columns (and rows) of the picture per cell of the minimap.
    pub scale: f64,
}

/// Lines across, which fill their cell from side to side.
const STROKE_ACROSS: &str = "─━┄┅┈┉╌╍═┬┴┼╤╧╪";
/// Lines down, which fill their cell from top to bottom.
const STROKE_DOWN: &str = "│┃┆┇┊┋╎╏║├┤┼╟╢╫";

/// `lines` drawn small enough for `room`, each cell of it 2×4 dots, with the
/// frame of `view` (first column and row of the picture shown, columns and
/// rows) on it. A picture that fits is drawn a character to a cell.
pub fn minimap(lines: &[String], room: Room, view: (usize, usize, usize, usize)) -> Minimap {
    let width = width_of(lines);
    let height = lines.len();
    let scale = (width as f64 / room.width.max(1) as f64)
        .max(height as f64 / room.height.max(1) as f64)
        .max(1.0);
    let cells = |n: usize| ((n as f64 / scale).ceil() as usize).max(1);
    let (cols, rows) = (
        cells(width).min(room.width.max(1)),
        cells(height).min(room.height.max(1)),
    );
    let mut dots = vec![vec![false; cols * 2]; rows * 4];
    for (y, line) in lines.iter().enumerate() {
        let mut x = 0;
        for c in line.chars() {
            let w = c.width().unwrap_or(0);
            if is_drawing(c) {
                let span = |at: usize, per: f64, whole: bool, len: usize| {
                    let from = at as f64 * per / scale;
                    let to = (at + 1) as f64 * per / scale;
                    let (a, b) = if whole {
                        (
                            from.floor() as usize,
                            (to.ceil() as usize).max(from.floor() as usize + 1),
                        )
                    } else {
                        let mid = ((from + to) / 2.0).floor() as usize;
                        (mid, mid + 1)
                    };
                    a.min(len)..b.min(len)
                };
                let across = span(x, 2.0, STROKE_ACROSS.contains(c), cols * 2);
                let down = span(y, 4.0, STROKE_DOWN.contains(c), rows * 4);
                for dy in down {
                    for dx in across.clone() {
                        dots[dy][dx] = true;
                    }
                }
            }
            x += w;
        }
    }
    let lines = (0..rows)
        .map(|row| {
            let line: String = (0..cols)
                .map(|col| {
                    let bits = BRAILLE.iter().fold(0u32, |bits, &(dx, dy, bit)| {
                        if dots[row * 4 + dy][col * 2 + dx] {
                            bits | bit
                        } else {
                            bits
                        }
                    });
                    if bits == 0 {
                        ' '
                    } else {
                        char::from_u32(0x2800 + bits).unwrap_or(' ')
                    }
                })
                .collect();
            line.trim_end().to_string()
        })
        .collect();
    let (left, top, shown_cols, shown_rows) = view;
    let x = ((left as f64 / scale).floor() as usize).min(cols.saturating_sub(1));
    let y = ((top as f64 / scale).floor() as usize).min(rows.saturating_sub(1));
    let w = (((left + shown_cols) as f64 / scale).ceil() as usize).clamp(x + 1, cols) - x;
    let h = (((top + shown_rows) as f64 / scale).ceil() as usize).clamp(y + 1, rows) - y;
    Minimap {
        lines,
        frame: (x, y, w, h),
        scale,
    }
}

/// The dot of a Braille cell at (column, row) and its bit.
const BRAILLE: [(usize, usize, u32); 8] = [
    (0, 0, 0x01),
    (0, 1, 0x02),
    (0, 2, 0x04),
    (1, 0, 0x08),
    (1, 1, 0x10),
    (1, 2, 0x20),
    (0, 3, 0x40),
    (1, 3, 0x80),
];

/// Whether `c` is part of a box, a line or an arrow rather than of text.
fn is_drawing(c: char) -> bool {
    matches!(c, '\u{2500}'..='\u{259f}' | '\u{25a0}'..='\u{25ff}' | '\u{2190}'..='\u{21ff}' | '<' | '>')
}

/// Keep the panic message of the renderer's thread off the terminal that the
/// TUI owns; other threads panic as before.
fn silence_renderer_panics() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if std::thread::current().name() != Some(RENDER_THREAD) {
                previous(info);
            }
        }));
    });
}

/// `text` with control characters (an escape sequence in it would reach the
/// terminal) replaced by a visible placeholder. Tabs become spaces.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\t' => ' ',
            c if c.is_control() => '\u{fffd}',
            c => c,
        })
        .collect()
}

/// Statements that only style a diagram or make it do something: they mean
/// nothing to a picture in the terminal, and `mmdflux` writes their values
/// into the SVG as they are, so a value can open an attribute or a tag of
/// its own (`style A fill:#f00"/><script>…`). The Mermaid text comes from
/// the Analyst, which read code under review: it is not trusted.
const STYLING: &[&str] = &[
    "style",
    "classDef",
    "class",
    "linkStyle",
    "click",
    "callback",
];

/// `mermaid` without its styling: the statements of [`STYLING`], front
/// matter (`---` … `---`, where a theme is configured) and directives
/// (`%%{init: …}%%`). The rest is left as it is.
pub fn without_styling(mermaid: &str) -> String {
    let mut out = Vec::new();
    let mut lines = mermaid.lines().peekable();
    // Front matter only opens the text.
    while lines.peek().is_some_and(|l| l.trim().is_empty()) {
        lines.next();
    }
    if lines.peek().is_some_and(|l| l.trim() == "---") {
        lines.next();
        for line in lines.by_ref() {
            if line.trim() == "---" {
                break;
            }
        }
    }
    let mut in_directive = false;
    for line in lines {
        let trimmed = line.trim();
        if in_directive || trimmed.starts_with("%%{") {
            in_directive = !trimmed.ends_with("}%%");
            continue;
        }
        // Statements of a flowchart may share a line, `;` between them.
        let mut kept = Vec::new();
        for statement in line.split(';') {
            let word = statement.split_whitespace().next().unwrap_or("");
            if STYLING.contains(&word) {
                break;
            }
            kept.push(statement);
        }
        let kept = kept.join(";");
        if kept.trim().is_empty() && !trimmed.is_empty() {
            continue;
        }
        out.push(kept);
    }
    out.join("\n")
}

/// `Ok` when `svg` has nothing in it that runs or reaches out: no script,
/// no handler of events (`on…=`), no links (`href`, `src`), no `url(…)`
/// but to a part of itself (`url(#arrowhead)`), no foreign content and no
/// declarations (`<!…>`, `<?…?>`). Only the tags are looked at: the text in
/// between is escaped by `mmdflux`.
pub fn check_svg(svg: &str) -> Result<(), String> {
    const TAGS: &[&str] = &[
        "script",
        "foreignobject",
        "iframe",
        "object",
        "embed",
        "style",
        "a",
        "image",
        "use",
        "animate",
        "set",
        "handler",
        "listener",
    ];
    const NAMES: &[&str] = &["href", "src", "xlink:href", "action", "formaction"];
    const VALUES: &[&str] = &[
        "javascript:",
        "data:",
        "expression(",
        "@import",
        "vbscript:",
    ];
    let refuse = |why: &str| Err(format!("the SVG has {why} in it: it is not opened"));
    let lower = svg.to_ascii_lowercase();
    let mut rest = lower.as_str();
    while let Some(at) = rest.find('<') {
        let after = &rest[at + 1..];
        let end = after.find('>').unwrap_or(after.len());
        let tag = &after[..end];
        rest = &after[end..];
        if tag.starts_with('!') || tag.starts_with('?') {
            return refuse("a declaration");
        }
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        if TAGS.contains(&name) {
            return refuse(&format!("a <{name}>"));
        }
        // Attribute names: a word right before `=`.
        for (i, _) in tag.match_indices('=') {
            let before = tag[..i].trim_end();
            let start = before
                .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == ':' || c == '-' || c == '_'))
                .map_or(0, |p| p + 1);
            let attribute = &before[start..];
            if attribute.starts_with("on") || NAMES.contains(&attribute) {
                return refuse(&format!("an attribute {attribute}"));
            }
        }
        if let Some(value) = VALUES.iter().find(|v| tag.contains(*v)) {
            return refuse(value);
        }
        let mut urls = tag.match_indices("url(");
        if urls.any(|(i, _)| {
            !tag[i + 4..]
                .trim_start_matches(['\'', '"', ' '])
                .starts_with('#')
        }) {
            return refuse("a url(…) outside of it");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn squeezing_cuts_runs_of_gap_columns_and_keeps_everything_else() {
        let picture = strings(&[
            "┌───┐        ┌───┐",
            "│ A │───────►│ B │",
            "└───┘        └───┘",
        ]);
        assert_eq!(
            regap(&picture, Some(1)),
            strings(&["┌───┐  ┌───┐", "│ A │─►│ B │", "└───┘  └───┘"])
        );
        assert_eq!(
            regap(&picture, Some(2)),
            strings(&["┌───┐   ┌───┐", "│ A │──►│ B │", "└───┘   └───┘"])
        );
    }

    #[test]
    fn rows_of_lines_down_are_squeezed_and_never_stretched() {
        let picture = strings(&[
            "┌───┐",
            "│ A │",
            "└─┬─┘",
            "  │",
            "  │",
            "  │",
            "  ▼",
            "┌───┐",
        ]);
        assert_eq!(
            regap(&picture, Some(1)),
            strings(&["┌───┐", "│ A │", "└─┬─┘", "  │", "  ▼", "┌───┐"])
        );
        assert_eq!(regap(&picture, None), picture);
    }

    #[test]
    fn a_picture_with_wide_characters_is_left_alone() {
        let picture = strings(&["│ 図 │      │", "│ 図 │      │"]);
        assert_eq!(regap(&picture, Some(1)), picture);
    }

    #[test]
    fn a_left_to_right_flowchart_is_turned_top_down() {
        assert_eq!(
            turned_top_down("%% c\nflowchart LR\n  A --> B").as_deref(),
            Some("%% c\nflowchart TD\n  A --> B")
        );
        assert_eq!(turned_top_down("graph TD\n  A --> B"), None);
        assert_eq!(turned_top_down("sequenceDiagram\n  A->>B: x"), None);
    }

    #[test]
    fn labels_of_nodes_and_edges_are_wrapped_and_nothing_else() {
        let wrapped = wrap_labels(
            "graph TD\n  A[one two three four] -->|five six seven| B((\"eight nine ten\"))\n  classDef long fill:#f00,stroke:#0f0",
            9,
        );
        assert_eq!(
            wrapped,
            "graph TD\n  A[one two<br/>three<br/>four] -->|five six<br/>seven| B((\"eight<br/>nine ten\"))\n  classDef long fill:#f00,stroke:#0f0"
        );
    }

    #[test]
    fn long_messages_and_names_of_a_sequence_diagram_are_cut_short() {
        assert_eq!(
            shorten_messages(
                "sequenceDiagram\n  participant A as A rather long name\n  A->>B: a message far too long\n  Note over A: short",
                10
            ),
            "sequenceDiagram\n  participant A as A rather…\n  A->>B: a message…\n  Note over A: short"
        );
    }

    #[test]
    fn the_minimap_draws_boxes_and_lines_in_dots_without_text() {
        let picture = strings(&["┌──┐", "│AB│", "└──┘"]);
        let small = minimap(
            &picture,
            Room {
                width: 10,
                height: 10,
            },
            (0, 0, 4, 3),
        );
        // A character to a cell: the box is its outline of dots.
        assert_eq!(small.scale, 1.0);
        assert_eq!(small.lines.len(), 3);
        assert!(small.lines.iter().all(|l| l
            .chars()
            .all(|c| c == ' ' || ('\u{2800}'..='\u{28ff}').contains(&c))));
        // The letters inside leave their cells blank.
        assert_eq!(small.lines[1].chars().nth(1), Some(' '));
        assert_eq!(small.frame, (0, 0, 4, 3));
    }

    #[test]
    fn a_large_picture_is_shrunk_into_the_room_with_the_frame_of_the_view() {
        let picture: Vec<String> = (0..40).map(|_| "─".repeat(200)).collect();
        let small = minimap(
            &picture,
            Room {
                width: 50,
                height: 20,
            },
            (100, 20, 50, 10),
        );
        assert_eq!(small.scale, 4.0);
        assert_eq!(small.lines.len(), 10);
        assert!(small.lines.iter().all(|l| l.chars().count() == 50));
        assert_eq!(small.frame, (25, 5, 13, 3));
    }
}

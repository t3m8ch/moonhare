//! Diagrams inside the session: `:diagram`, its background request, cache and
//! overlay.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};

use super::*;
use unicode_width::UnicodeWidthStr;

use crate::analyst::{DiagramRequest, DiagramSubject};

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
    pending: HashSet<Key>,
    sender: Sender<Answer>,
    receiver: Receiver<Answer>,
    /// The overlay is open on this diagram.
    open: Option<Key>,
    /// Columns the picture is scrolled to the left.
    hscroll: usize,
    /// Rows the picture is scrolled up.
    vscroll: usize,
    /// `g` was typed and waits for a second one.
    g_typed: bool,
    /// Mermaid texts of Summaries, drawn once.
    drawings: HashMap<String, Result<Vec<String>, String>>,
}

impl Default for Diagrams {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Diagrams {
            cache: HashMap::new(),
            errors: HashMap::new(),
            pending: HashSet::new(),
            sender,
            receiver,
            open: None,
            hscroll: 0,
            vscroll: 0,
            g_typed: false,
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
        if !self.diagrams.cache.contains_key(&key) && self.diagrams.pending.insert(key.clone()) {
            self.diagrams.errors.remove(&key);
            let request = DiagramRequest {
                repo: self.repo.root().to_path_buf(),
                base: self.range.base.clone(),
                head: key.0.clone(),
                language: key.1.clone(),
                settings: self.analysis.settings.clone(),
                subject: key.2.clone(),
            };
            let sender = self.diagrams.sender.clone();
            std::thread::spawn(move || {
                let answer = analyst
                    .diagram(&request)
                    .map(|diagram| Drawn {
                        lines: crate::diagram::draw(&diagram.mermaid),
                        mermaid: diagram.mermaid,
                    })
                    .map_err(|e| e.to_string());
                let _ = sender.send((key_of(&request), answer));
            });
        }
        self.diagrams.open = Some(key);
        self.diagrams.hscroll = 0;
        self.diagrams.vscroll = 0;
        true
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
                self.diagrams.errors.remove(&key);
                self.diagrams.cache.insert(key, drawn);
            }
            Err(e) => {
                self.diagrams.errors.insert(key, e);
            }
        }
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
        let rows = self.overlay_area().height.saturating_sub(2) as usize;
        let last = lines.len().saturating_sub(rows);
        let g_typed = std::mem::take(&mut self.diagrams.g_typed);
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.diagrams.vscroll = (self.diagrams.vscroll + 1).min(last)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.diagrams.vscroll = self.diagrams.vscroll.saturating_sub(1)
            }
            KeyCode::Char('G') => self.diagrams.vscroll = last,
            KeyCode::Char('g') if g_typed => self.diagrams.vscroll = 0,
            KeyCode::Char('g') => self.diagrams.g_typed = true,
            _ => {}
        }
        self.picture_key(key.code, source, &lines);
        true
    }

    pub(super) fn render_diagram_overlay(&self, buf: &mut Buffer) {
        let Some(key) = &self.diagrams.open else {
            return;
        };
        let area = self.overlay_area();
        clear_area(buf, area);
        let what = match &key.2 {
            DiagramSubject::File { path } | DiagramSubject::Fragment { path, .. } => path,
            DiagramSubject::Group { name, .. } => name,
        };
        draw_box(buf, area, &format!(" Diagram: {what} "));
        let inner_w = self.overlay_text_width();
        let rows = area.height.saturating_sub(2) as usize;
        for (row, line) in self
            .open_lines()
            .iter()
            .skip(self.diagrams.vscroll)
            .take(rows)
            .enumerate()
        {
            let visible: String = line.chars().skip(self.diagrams.hscroll).collect();
            buf.set_stringn(
                area.x + 2,
                area.y + 1 + row as u16,
                visible,
                inner_w,
                Style::new().fg(FG),
            );
        }
    }

    /// Width in cells of the text inside the overlay's border.
    fn overlay_text_width(&self) -> usize {
        self.overlay_area().width.saturating_sub(4) as usize
    }

    /// The lines the open overlay shows.
    fn open_lines(&self) -> Vec<String> {
        let Some(key) = &self.diagrams.open else {
            return Vec::new();
        };
        match self.diagrams.cache.get(key) {
            Some(Drawn { lines: Ok(l), .. }) => l.clone(),
            Some(Drawn {
                lines: Err(e),
                mermaid,
            }) => failure_lines(e, mermaid),
            None => match self.diagrams.errors.get(key) {
                Some(e) => vec![format!("Analyst: {}", crate::diagram::sanitize(e))],
                None => vec!["The diagram is being drawn…".into()],
            },
        }
    }
}

/// What is shown instead of a picture that could not be drawn.
fn failure_lines(error: &str, mermaid: &str) -> Vec<String> {
    format!("Cannot draw the diagram: {error}\n\n{mermaid}")
        .lines()
        .map(crate::diagram::sanitize)
        .collect()
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
pub(super) fn clear_area(buf: &mut Buffer, area: Rect) {
    let area = area.intersection(buf.area);
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, y)];
            cell.reset();
            cell.set_fg(FG);
        }
    }
}

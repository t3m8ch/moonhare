//! Diagrams inside the session: `:diagram`, its background request, cache and
//! overlay.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};

use super::*;
use crate::analyst::{AnalystError, Diagram, DiagramRequest, DiagramSubject};

/// What a diagram is cached under: head of the Range, language, subject.
type Key = (String, String, DiagramSubject);

type Answer = (Key, Result<Diagram, AnalystError>);

/// A diagram that came back, drawn once.
struct Drawn {
    mermaid: String,
    lines: Result<Vec<String>, String>,
}

pub(super) struct Diagrams {
    cache: HashMap<Key, Result<Drawn, String>>,
    pending: HashSet<Key>,
    sender: Sender<Answer>,
    receiver: Receiver<Answer>,
    /// The overlay is open on this diagram.
    open: Option<Key>,
    /// Columns the picture is scrolled to the left.
    hscroll: usize,
    /// Text the Reviewer asked to copy, until the terminal layer takes it.
    clipboard: Option<String>,
    /// Mermaid texts of Summaries, drawn once.
    drawings: HashMap<String, Result<Vec<String>, String>>,
}

impl Default for Diagrams {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Diagrams {
            cache: HashMap::new(),
            pending: HashSet::new(),
            sender,
            receiver,
            open: None,
            hscroll: 0,
            clipboard: None,
            drawings: HashMap::new(),
        }
    }
}

impl Session {
    /// `:diagram`. Returns whether `command` was one of the diagram's.
    pub(super) fn diagram_command(&mut self, command: &str) -> bool {
        let here = match command {
            "diagram" => false,
            "diagram here" => true,
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
        let subject = match (here, self.cursor_line()) {
            (true, Some(line)) => DiagramSubject::Fragment { path, line },
            _ => DiagramSubject::File { path },
        };
        let key = (self.range.head.clone(), self.language_code(), subject);
        if !self.diagrams.cache.contains_key(&key) && self.diagrams.pending.insert(key.clone()) {
            let request = DiagramRequest {
                repo: self.repo.root().to_path_buf(),
                base: self.range.base.clone(),
                head: key.0.clone(),
                language: key.1.clone(),
                subject: key.2.clone(),
            };
            let sender = self.diagrams.sender.clone();
            std::thread::spawn(move || {
                let answer = analyst.diagram(&request);
                let _ = sender.send((key_of(&request), answer));
            });
        }
        self.diagrams.open = Some(key);
        self.diagrams.hscroll = 0;
        true
    }

    /// The line of the new version of the file under the cursor (1-based);
    /// for a removed line, the nearest line that exists in the new one.
    fn cursor_line(&self) -> Option<usize> {
        let items = &self.view.items;
        (self.vp.cursor..items.len())
            .chain((0..self.vp.cursor).rev())
            .find_map(|i| self.item_lines(*items.get(i)?).1)
            .map(|line| line + 1)
    }

    fn take_diagram(&mut self, (key, answer): Answer) {
        self.diagrams.pending.remove(&key);
        let entry = match answer {
            Ok(Diagram { mermaid }) => Ok(Drawn {
                lines: crate::diagram::draw(&mermaid),
                mermaid,
            }),
            Err(e) => Err(e.to_string()),
        };
        self.diagrams.cache.insert(key, entry);
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
                let widest = lines.iter().map(|l| width(l)).max().unwrap_or(0);
                let limit = widest.saturating_sub(self.overlay_text_width());
                self.diagrams.hscroll = (self.diagrams.hscroll + HSCROLL_STEP).min(limit);
            }
            KeyCode::Char('h') => {
                self.diagrams.hscroll = self.diagrams.hscroll.saturating_sub(HSCROLL_STEP)
            }
            KeyCode::Char('y') => {
                if let Some(source) = source {
                    self.diagrams.clipboard = Some(source);
                    self.message = Some("Copied the Mermaid source".into());
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

    /// The text the Reviewer asked to copy since the last call. The terminal
    /// layer puts it on the clipboard.
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.diagrams.clipboard.take()
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
            self.diagrams
                .open
                .as_ref()
                .and_then(|key| match self.diagrams.cache.get(key) {
                    Some(Ok(drawn)) => Some(drawn.mermaid.clone()),
                    _ => None,
                });
        let lines = self.open_lines();
        self.picture_key(key.code, source, &lines);
        true
    }

    pub(super) fn render_diagram_overlay(&self, buf: &mut Buffer) {
        let Some(key) = &self.diagrams.open else {
            return;
        };
        let area = self.overlay_area();
        clear_area(buf, area);
        let (DiagramSubject::File { path } | DiagramSubject::Fragment { path, .. }) = &key.2;
        draw_box(buf, area, &format!(" Diagram: {path} "));
        let inner_w = self.overlay_text_width();
        let rows = area.height.saturating_sub(2) as usize;
        for (row, line) in self.open_lines().iter().take(rows).enumerate() {
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
            Some(Ok(Drawn { lines: Ok(l), .. })) => l.clone(),
            Some(Ok(Drawn {
                lines: Err(e),
                mermaid,
            })) => failure_lines(e, mermaid),
            Some(Err(e)) => vec![format!("Analyst: {e}")],
            None => vec!["The diagram is being drawn…".into()],
        }
    }
}

/// What is shown instead of a picture that could not be drawn.
fn failure_lines(error: &str, mermaid: &str) -> Vec<String> {
    format!("Cannot draw the diagram: {error}\n\n{mermaid}")
        .lines()
        .map(String::from)
        .collect()
}

fn width(line: &str) -> usize {
    line.chars().count()
}

fn key_of(request: &DiagramRequest) -> Key {
    (
        request.head.clone(),
        request.language.clone(),
        request.subject.clone(),
    )
}

/// Blank the area an overlay is drawn over.
pub(super) fn clear_area(buf: &mut Buffer, area: Rect) {
    for y in area.y..area.y + area.height {
        buf.set_string(
            area.x,
            y,
            " ".repeat(area.width as usize),
            Style::new().fg(FG),
        );
    }
}

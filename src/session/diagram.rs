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
        }
    }
}

impl Session {
    /// `:diagram`. Returns whether `command` was one of the diagram's.
    pub(super) fn diagram_command(&mut self, command: &str) -> bool {
        if command != "diagram" {
            return false;
        }
        let Some(analyst) = self.analyst_handle() else {
            self.message = Some("No Analyst is configured: there are no diagrams.".into());
            return true;
        };
        let Some(file) = self.files.get(self.selected) else {
            self.message = Some("No file to draw.".into());
            return true;
        };
        let subject = DiagramSubject::File {
            path: file.path.clone(),
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
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.diagrams.open = None,
            KeyCode::Char('l') => {
                let widest = self.open_lines().iter().map(|l| width(l)).max().unwrap_or(0);
                let limit = widest.saturating_sub(self.overlay_text_width());
                self.diagrams.hscroll = (self.diagrams.hscroll + HSCROLL_STEP).min(limit);
            }
            KeyCode::Char('y') => {
                let source = self.diagrams.open.as_ref().and_then(|key| {
                    match self.diagrams.cache.get(key) {
                        Some(Ok(drawn)) => Some(drawn.mermaid.clone()),
                        _ => None,
                    }
                });
                if let Some(source) = source {
                    self.diagrams.clipboard = Some(source);
                    self.message = Some("Copied the Mermaid source".into());
                }
            }
            KeyCode::Char('h') => {
                self.diagrams.hscroll = self.diagrams.hscroll.saturating_sub(HSCROLL_STEP)
            }
            _ => {}
        }
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
            })) => format!("Cannot draw the diagram: {e}\n\n{mermaid}")
                .lines()
                .map(String::from)
                .collect(),
            Some(Err(e)) => vec![format!("Analyst: {e}")],
            None => vec!["The diagram is being drawn…".into()],
        }
    }
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

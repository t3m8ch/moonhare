//! Opening a pull request by number: the Forge is asked in the background and
//! a loading screen is shown meanwhile, so the terminal is never left empty
//! and the Reviewer can still leave.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::style::{Modifier, Style};

use crate::forge::{Forge, PullRequest};
use crate::git::Repo;
use crate::session::{Error, Session};

type Fetched = Result<(PullRequest, String), String>;

pub struct Opening {
    repo: PathBuf,
    number: u64,
    size: (u16, u16),
    receiver: Receiver<Fetched>,
    clock: Duration,
    cancelled: bool,
}

impl Opening {
    /// Start asking `forge` for pull request `number` of the clone containing
    /// `repo_path`, and fetching its head.
    pub fn pull_request(
        repo_path: &Path,
        forge: Arc<dyn Forge>,
        number: u64,
        width: u16,
        height: u16,
    ) -> Result<Opening, Error> {
        let root = Repo::open(repo_path)?.root().to_path_buf();
        let (sender, receiver) = channel();
        let fetch_root = root.clone();
        std::thread::spawn(move || {
            let fetched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                forge
                    .pull_request(number)
                    .and_then(|pr| {
                        let reference = forge.fetch_pull_request_head(&fetch_root, number)?;
                        Ok((pr, reference))
                    })
                    .map_err(|e| e.to_string())
            }))
            .unwrap_or_else(|_| Err("the Forge request crashed".to_string()));
            let _ = sender.send(fetched);
        });
        Ok(Opening {
            repo: root,
            number,
            size: (width, height),
            receiver,
            clock: Duration::ZERO,
            cancelled: false,
        })
    }

    /// The session, once the Forge has answered; `None` while it has not.
    pub fn poll(&mut self) -> Option<Result<Session, Error>> {
        let fetched = match self.receiver.try_recv() {
            Ok(fetched) => fetched,
            Err(std::sync::mpsc::TryRecvError::Empty) => return None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("the Forge request ended without an answer".to_string())
            }
        };
        Some(match fetched {
            Ok((pr, reference)) => Session::open_fetched_pull_request(
                &self.repo,
                pr,
                &reference,
                self.size.0,
                self.size.1,
            ),
            Err(e) => Err(Error(e)),
        })
    }

    /// `q` and `Ctrl-c` leave; everything else is ignored.
    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Key(k) if k.kind != KeyEventKind::Release => match k.code {
                KeyCode::Char('q') if k.modifiers.is_empty() => self.cancelled = true,
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.cancelled = true
                }
                _ => {}
            },
            Event::Resize(w, h) => self.size = (w, h),
            _ => {}
        }
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled
    }

    /// Time passing, for the spinner.
    pub fn elapse(&mut self, elapsed: Duration) {
        self.clock += elapsed;
    }

    pub fn render(&self, buf: &mut Buffer) {
        let area = buf.area;
        let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let spinner = frames[(self.clock.as_millis() / 100) as usize % frames.len()];
        let text = format!(
            "{spinner} Loading pull request #{} · {}s",
            self.number,
            self.clock.as_secs()
        );
        let y = area.y + area.height / 2;
        let x = area.x + area.width.saturating_sub(text.chars().count() as u16) / 2;
        buf.set_stringn(x, y, &text, area.width as usize, Style::new());
        let hint = "q or Ctrl-c to quit";
        let x = area.x + area.width.saturating_sub(hint.len() as u16) / 2;
        buf.set_stringn(
            x,
            y + 1,
            hint,
            area.width as usize,
            Style::new().add_modifier(Modifier::DIM),
        );
    }
}

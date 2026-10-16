//! Test infrastructure for the review session seam: a real temporary git
//! repository ([`Fixture`]) and a terminal-less driver ([`Harness`]) that feeds
//! key, mouse and resize events to a [`Session`] and reads back the frame as text.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use moonhare::git::Target;
use moonhare::session::{Frame, Session};
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use tempfile::TempDir;

/// A real git repository in a temporary directory, built from the test.
pub struct Fixture {
    dir: TempDir,
}

impl Fixture {
    /// A fresh repository whose default branch is `main`, with no commits.
    pub fn new() -> Self {
        let fx = Fixture {
            dir: tempfile::tempdir().unwrap(),
        };
        fx.git(&["init", "-q", "-b", "main"]);
        fx.git(&["config", "user.name", "Test"]);
        fx.git(&["config", "user.email", "test@example.com"]);
        fx.git(&["config", "commit.gpgsign", "false"]);
        fx
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.path())
            // Isolate from the developer's own git configuration.
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn write(&self, path: &str, content: &str) -> &Self {
        self.write_bytes(path, content.as_bytes())
    }

    pub fn write_bytes(&self, path: &str, content: &[u8]) -> &Self {
        let full: PathBuf = self.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
        self
    }

    pub fn remove(&self, path: &str) -> &Self {
        std::fs::remove_file(self.path().join(path)).unwrap();
        self
    }

    /// Stage everything and commit.
    pub fn commit(&self, message: &str) -> &Self {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
        self
    }

    /// Create a branch at HEAD and switch to it.
    pub fn branch(&self, name: &str) -> &Self {
        self.git(&["checkout", "-q", "-b", name]);
        self
    }

    pub fn checkout(&self, name: &str) -> &Self {
        self.git(&["checkout", "-q", name]);
        self
    }

    /// Everything a read-only tool must leave alone: refs, HEAD, working copy
    /// status and file contents.
    pub fn state(&self) -> String {
        let mut files = String::new();
        let mut stack = vec![self.path().to_path_buf()];
        while let Some(d) = stack.pop() {
            let mut entries: Vec<_> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap()).collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                if e.file_name() == ".git" {
                    continue;
                }
                if e.path().is_dir() {
                    stack.push(e.path());
                } else {
                    files.push_str(&format!(
                        "{}={:?}\n",
                        e.path().display(),
                        std::fs::read(e.path()).unwrap()
                    ));
                }
            }
        }
        format!(
            "refs:\n{}\nHEAD: {}\nstatus:\n{}\nfiles:\n{}",
            self.git(&["for-each-ref"]),
            self.git(&["symbolic-ref", "-q", "HEAD"]),
            self.git(&["status", "--porcelain"]),
            files
        )
    }
}

/// `main` holds one commit; a `feature` branch of three commits touches
/// `README.md`, `src/a.rs` (added, then changed) and `src/b.rs`.
pub fn feature_branch() -> Fixture {
    let fx = Fixture::new();
    fx.write("README.md", "# demo\n").commit("initial");
    fx.branch("feature");
    fx.write("src/a.rs", "fn a() {\n    1\n}\n").commit("add a");
    fx.write("src/b.rs", "fn b() {\n    2\n}\n")
        .write("README.md", "# demo\n\nmore\n")
        .commit("add b, touch readme");
    fx.write("src/a.rs", "fn a() {\n    10\n}\n")
        .commit("change a");
    fx
}

/// Drives a [`Session`] without a real terminal.
pub struct Harness {
    session: Session,
}

impl Harness {
    pub fn open(fx: &Fixture, width: u16, height: u16) -> Self {
        Harness {
            session: Session::open(fx.path(), width, height).expect("session opens"),
        }
    }

    pub fn from_session(session: Session) -> Self {
        Harness { session }
    }

    pub fn try_open_target(
        fx: &Fixture,
        target: &Target,
        width: u16,
        height: u16,
    ) -> Result<Self, moonhare::session::Error> {
        Session::open_target(fx.path(), target, width, height).map(|session| Harness { session })
    }

    pub fn open_target(fx: &Fixture, target: &Target, width: u16, height: u16) -> Self {
        Self::try_open_target(fx, target, width, height).expect("session opens")
    }

    /// Feed keys written in Vim notation: plain characters plus `<Esc>`,
    /// `<CR>`, `<Tab>`, `<Space>`, `<lt>`, `<C-d>` and the like.
    pub fn keys(&mut self, notation: &str) -> &mut Self {
        for ev in parse_keys(notation) {
            self.session.handle(ev);
        }
        self
    }

    pub fn resize(&mut self, width: u16, height: u16) -> &mut Self {
        self.session.handle(Event::Resize(width, height));
        self
    }

    pub fn click(&mut self, column: u16, row: u16) -> &mut Self {
        self.mouse(MouseEventKind::Down(MouseButton::Left), column, row)
    }

    pub fn wheel_down(&mut self, column: u16, row: u16) -> &mut Self {
        self.mouse(MouseEventKind::ScrollDown, column, row)
    }

    fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) -> &mut Self {
        self.session.handle(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
        self
    }

    pub fn frame(&mut self) -> Frame {
        self.session.frame()
    }

    /// The frame as text, rows joined by newlines, trailing blanks trimmed.
    pub fn screen(&mut self) -> String {
        self.frame().text()
    }

    /// The session under test, for calls that are not terminal events.
    pub fn session(&mut self) -> &mut Session {
        &mut self.session
    }

    pub fn quit_requested(&self) -> bool {
        self.session.should_quit()
    }
}

pub fn parse_keys(notation: &str) -> Vec<Event> {
    let key = |code, mods| Event::Key(KeyEvent::new(code, mods));
    let chars: Vec<char> = notation.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            if let Some(len) = chars[i..].iter().position(|&c| c == '>') {
                let name: String = chars[i + 1..i + len].iter().collect();
                let ev = match name.as_str() {
                    "Esc" => Some(key(KeyCode::Esc, KeyModifiers::NONE)),
                    "CR" => Some(key(KeyCode::Enter, KeyModifiers::NONE)),
                    "Tab" => Some(key(KeyCode::Tab, KeyModifiers::NONE)),
                    "BS" => Some(key(KeyCode::Backspace, KeyModifiers::NONE)),
                    "Space" => Some(key(KeyCode::Char(' '), KeyModifiers::NONE)),
                    "lt" => Some(key(KeyCode::Char('<'), KeyModifiers::NONE)),
                    n if n.starts_with("C-") && n.chars().count() == 3 => Some(key(
                        KeyCode::Char(n.chars().nth(2).unwrap()),
                        KeyModifiers::CONTROL,
                    )),
                    _ => None,
                };
                if let Some(ev) = ev {
                    out.push(ev);
                    i += len + 1;
                    continue;
                }
            }
        }
        out.push(key(KeyCode::Char(chars[i]), KeyModifiers::NONE));
        i += 1;
    }
    out
}

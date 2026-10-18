//! Startup does not wait for the Forge: the first frame comes from git alone,
//! and the pull request arrives in the background.

mod support;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use moonhare::forge::{Forge, ForgeError, PullRequest};
use moonhare::git::Target;
use moonhare::opening::Opening;
use moonhare::session::Session;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use support::{feature_branch, Harness};

/// A Forge whose answers wait until the test lets them go.
#[derive(Clone, Default)]
struct Gate(Arc<AtomicBool>);

impl Gate {
    fn open(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn pass(&self) {
        let start = Instant::now();
        while !self.0.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

struct SlowForge {
    gate: Gate,
    answer: Result<Option<PullRequest>, ForgeError>,
}

fn pr() -> PullRequest {
    PullRequest {
        number: 12,
        title: "Add a and b".into(),
        body: "Needed".into(),
        base_branch: "main".into(),
    }
}

impl Forge for SlowForge {
    fn pull_request_for_branch(&self, _: &str) -> Result<Option<PullRequest>, ForgeError> {
        self.gate.pass();
        self.answer.clone()
    }

    fn pull_request(&self, _: u64) -> Result<PullRequest, ForgeError> {
        self.gate.pass();
        self.answer
            .clone()
            .and_then(|p| p.ok_or_else(|| ForgeError::NotFound("no such pull request".into())))
    }

    fn fetch_pull_request_head(&self, repo: &Path, number: u64) -> Result<String, ForgeError> {
        let reference = format!("refs/moonhare/pr/{number}");
        let head = std::process::Command::new("git")
            .args(["rev-parse", "feature"])
            .current_dir(repo)
            .output()
            .unwrap();
        let head = String::from_utf8(head.stdout).unwrap();
        std::process::Command::new("git")
            .args(["update-ref", &reference, head.trim()])
            .current_dir(repo)
            .status()
            .unwrap();
        Ok(reference)
    }
}

fn open(answer: Result<Option<PullRequest>, ForgeError>) -> (Harness, Gate, Duration) {
    let fx = Box::leak(Box::new(feature_branch()));
    let gate = Gate::default();
    let forge = Arc::new(SlowForge {
        gate: gate.clone(),
        answer,
    });
    let started = Instant::now();
    let session = Session::open_target_with_forge(fx.path(), &Target::Current, forge, 100, 30)
        .expect("session opens");
    let took = started.elapsed();
    (Harness::from_session(session), gate, took)
}

#[test]
fn the_first_frame_does_not_wait_for_the_forge() {
    let (mut h, gate, took) = open(Ok(Some(pr())));

    assert!(took < Duration::from_secs(5), "{took:?}");
    let screen = h.screen();
    assert!(screen.contains("README.md"), "{screen}");
    assert!(screen.contains("PR: loading"), "{screen}");
    assert!(h.session().pull_request().is_none());

    // The interface answers while the request is on its way.
    h.keys("]f");
    assert!(h.screen().contains("src/a.rs"));

    gate.open();
    h.session().wait_for_forge();
    assert_eq!(h.session().pull_request().map(|p| p.number), Some(12));
    assert!(!h.screen().contains("PR: loading"));
}

#[test]
fn a_failing_forge_shows_the_warning_once_it_answers() {
    let (mut h, gate, _) = open(Err(ForgeError::Unavailable("gh is not installed".into())));
    assert!(!h.screen().contains("gh is not installed"));

    gate.open();
    h.session().wait_for_forge();
    let screen = h.screen();
    assert!(screen.contains("⚠ gh is not installed"), "{screen}");
    assert!(!screen.contains("PR: loading"), "{screen}");
}

#[test]
fn the_busy_forge_keeps_the_session_alive_for_the_event_loop() {
    let (mut h, gate, _) = open(Ok(None));
    assert!(h.session().forge_busy());
    gate.open();
    h.session().wait_for_forge();
    assert!(!h.session().forge_busy());
}

fn text(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn opening(answer: Result<Option<PullRequest>, ForgeError>) -> (Opening, Gate) {
    let fx = Box::leak(Box::new(feature_branch()));
    let gate = Gate::default();
    let forge = Arc::new(SlowForge {
        gate: gate.clone(),
        answer,
    });
    (
        Opening::pull_request(fx.path(), forge, 12, 100, 30).unwrap(),
        gate,
    )
}

#[test]
fn opening_by_number_shows_a_loading_screen_that_can_be_left() {
    let (mut opening, gate) = opening(Ok(Some(pr())));
    assert!(opening.poll().is_none());
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 30));
    opening.render(&mut buf);
    let screen = text(&buf);
    assert!(screen.contains("Loading pull request #12"), "{screen}");

    assert!(!opening.cancelled());
    opening.handle(key(KeyCode::Char('x'), KeyModifiers::NONE));
    assert!(!opening.cancelled());
    opening.handle(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(opening.cancelled());
    gate.open();
}

#[test]
fn q_also_leaves_the_loading_screen() {
    let (mut opening, gate) = opening(Ok(Some(pr())));
    opening.handle(key(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(opening.cancelled());
    gate.open();
}

#[test]
fn the_loading_screen_turns_into_the_session() {
    let (mut opening, gate) = opening(Ok(Some(pr())));
    gate.open();
    let started = Instant::now();
    let session = loop {
        if let Some(result) = opening.poll() {
            break result.expect("opens");
        }
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(session.pull_request().map(|p| p.number), Some(12));
}

#[test]
fn a_failure_while_loading_is_reported() {
    let (mut opening, gate) = opening(Err(ForgeError::Unavailable("gh is not installed".into())));
    gate.open();
    let started = Instant::now();
    let err = loop {
        if let Some(result) = opening.poll() {
            break result.err().expect("fails");
        }
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(err.to_string().contains("gh is not installed"), "{err}");
}

/// A Forge whose request panics.
struct CrashingForge;

impl Forge for CrashingForge {
    fn pull_request_for_branch(&self, _: &str) -> Result<Option<PullRequest>, ForgeError> {
        panic!("forge crashed")
    }

    fn pull_request(&self, _: u64) -> Result<PullRequest, ForgeError> {
        panic!("forge crashed")
    }

    fn fetch_pull_request_head(&self, _: &Path, _: u64) -> Result<String, ForgeError> {
        panic!("forge crashed")
    }
}

#[test]
fn a_crashing_forge_ends_the_wait_with_a_warning() {
    let fx = Box::leak(Box::new(feature_branch()));
    let session = Session::open_target_with_forge(
        fx.path(),
        &Target::Current,
        Arc::new(CrashingForge),
        100,
        30,
    )
    .expect("opens");
    let mut h = Harness::from_session(session);
    h.session().wait_for_forge();
    assert!(!h.session().forge_busy());
    assert!(h.screen().contains("⚠"), "{}", h.screen());
}

#[test]
fn a_crashing_forge_fails_the_loading_screen_instead_of_spinning() {
    let fx = Box::leak(Box::new(feature_branch()));
    let mut opening =
        Opening::pull_request(fx.path(), Arc::new(CrashingForge), 12, 100, 30).unwrap();
    let started = Instant::now();
    let err = loop {
        if let Some(result) = opening.poll() {
            break result.err().expect("fails");
        }
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(!err.to_string().is_empty());
}

#[test]
fn the_loading_screen_fits_any_terminal_size() {
    let (opening, gate) = opening(Ok(Some(pr())));
    for (width, height) in [
        (1, 1),
        (40, 1),
        (300, 1),
        (40, 2),
        (300, 2),
        (2, 3),
        (1, 50),
    ] {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        opening.render(&mut buf);
        let screen = text(&buf);
        assert!(
            screen.contains("Loading") || width < 30,
            "{width}x{height}:\n{screen}"
        );
    }
    let mut buf = Buffer::empty(Rect::new(0, 0, 40, 0));
    opening.render(&mut buf);
    gate.open();
}

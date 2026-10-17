//! A panic of the Forge's thread is a warning on the screen, not a restore of
//! the terminal (#65).

mod support;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use moonhare::forge::{Forge, ForgeError, PullRequest};
use moonhare::git::Target;
use moonhare::opening::Opening;
use moonhare::session::{silence_worker_panics, Session};
use support::{feature_branch, Harness};

static HOOK_CALLS: AtomicUsize = AtomicUsize::new(0);

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

/// One test, since the hook is global to the process.
#[test]
fn a_panicking_forge_leaves_the_terminal_alone_and_ends_the_wait() {
    // Stands in for ratatui's hook, which restores the terminal.
    std::panic::set_hook(Box::new(|_| {
        HOOK_CALLS.fetch_add(1, Ordering::SeqCst);
    }));
    silence_worker_panics();

    let fx = feature_branch();
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
    let screen = h.screen();
    assert!(screen.contains("⚠"), "{screen}");
    assert!(!screen.contains("PR: loading"), "{screen}");

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

    assert_eq!(HOOK_CALLS.load(Ordering::SeqCst), 0);
}

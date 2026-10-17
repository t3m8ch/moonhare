//! A panic of the diff engine in a worker is an error on the screen, not a
//! restore of the terminal (#64).

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use moonhare::session::{silence_worker_panics, DiffEngine};
use support::{feature_branch, Harness};

static HOOK_CALLS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn a_panicking_engine_shows_an_error_and_leaves_the_terminal_alone() {
    // Stands in for ratatui's hook, which restores the terminal.
    std::panic::set_hook(Box::new(|_| {
        HOOK_CALLS.fetch_add(1, Ordering::SeqCst);
    }));
    silence_worker_panics();

    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    let engine: DiffEngine = Arc::new(|_, _, _| panic!("difftastic blew up"));
    h.session().set_diff_engine(engine);
    let screen = h.screen();
    assert!(screen.contains("The diff engine failed"), "{screen}");
    assert!(!screen.contains("Computing diff"), "{screen}");
    assert_eq!(HOOK_CALLS.load(Ordering::SeqCst), 0);
}

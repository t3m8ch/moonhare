//! Moving to another file never waits for its diff: the diff is computed in
//! the background and a placeholder stands in until it is ready (#64).

mod support;

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use moonhare::diff::{diff_file, Limits};
use moonhare::session::DiffEngine;
use support::{feature_branch, Fixture, Harness};

/// A door the fake engine waits at until the test opens it.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }

    fn pass(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.changed.wait(open).unwrap();
        }
    }
}

/// An engine that is the real one, except that paths accepted by `slow` wait
/// at the gate first. Every call is noted in `calls`.
fn engine(
    gate: Arc<Gate>,
    calls: Arc<Mutex<Vec<String>>>,
    slow: impl Fn(&str) -> bool + Send + Sync + 'static,
) -> DiffEngine {
    Arc::new(move |path, old, new| {
        calls.lock().unwrap().push(path.to_string());
        if slow(path) {
            gate.pass();
        }
        diff_file(path, old, new, Limits::default())
    })
}

#[test]
fn a_slow_diff_does_not_hold_up_the_key_and_the_placeholder_gives_way_to_the_diff() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    let gate = Arc::new(Gate::default());
    let calls = Arc::new(Mutex::new(Vec::new()));
    h.session()
        .set_diff_engine(engine(gate.clone(), calls, |path| path == "src/b.rs"));
    h.settle();

    let started = Instant::now();
    h.keys_now("]f]f");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the key waited for the diff"
    );
    let waiting = h.screen_now();
    assert!(waiting.contains("Computing diff"), "{waiting}");
    assert!(!waiting.contains("fn b()"), "{waiting}");
    assert!(waiting.contains("> A src/b.rs"), "{waiting}");

    gate.open();
    let ready = h.screen();
    assert!(!ready.contains("Computing diff"), "{ready}");
    assert!(ready.contains("fn b()"), "{ready}");
}

/// A branch of 30 files, `src/f00.rs` to `src/f29.rs`, each saying its name.
fn many_files() -> Fixture {
    let fx = Fixture::new();
    fx.write("README.md", "x\n").commit("initial");
    fx.branch("feature");
    for i in 0..30 {
        fx.write(&format!("src/f{i:02}.rs"), &format!("fn f{i:02}() {{}}\n"));
    }
    fx.commit("add files");
    fx
}

#[test]
fn paging_fast_through_the_list_queues_nothing_and_shows_the_file_it_stopped_on() {
    let fx = many_files();
    let mut h = Harness::open(&fx, 160, 30);
    let gate = Arc::new(Gate::default());
    let calls = Arc::new(Mutex::new(Vec::new()));
    h.session()
        .set_diff_engine(engine(gate.clone(), calls.clone(), |_| true));

    for _ in 0..29 {
        h.keys_now("]f");
    }
    let waiting = h.screen_now();
    assert!(waiting.contains("Computing diff"), "{waiting}");
    assert!(!waiting.contains("fn f"), "a wrong file shows:\n{waiting}");

    gate.open();
    let ready = h.screen();
    assert!(ready.contains("fn f29()"), "{ready}");
    assert!(ready.contains("file 30/30"), "{ready}");

    // Only what was already running, plus the file and its neighbours.
    let done = calls.lock().unwrap().len();
    assert!(
        done <= 10,
        "{done} diffs computed: {:?}",
        calls.lock().unwrap()
    );
}

#[test]
fn a_diff_that_is_ready_shows_at_once_on_the_way_back() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]f");
    let gate = Arc::new(Gate::default());
    h.session().set_diff_engine(engine(
        gate.clone(),
        Arc::new(Mutex::new(Vec::new())),
        |_| true,
    ));
    gate.open();
    h.settle().keys("[f");
    gate.open();
    // README.md is in the cache now: no placeholder even if the engine stalls.
    let screen = h.screen_now();
    assert!(!screen.contains("Computing diff"), "{screen}");
}

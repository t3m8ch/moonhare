//! The Analyst's status: the status line says whether the Analyst works and on
//! what, with a fake whose answers wait for the test to let them go.

mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use moonhare::analyst::{
    Analyst, AnalystError, ExplainRequest, Explanation, Group, GroupRequest, Grouping, Summary,
    SummaryRequest,
};
use moonhare::session::Session;
use support::{feature_branch, Fixture, Harness};

/// A gate the fake's answers wait at until the test opens it.
#[derive(Clone, Default)]
struct Gate(Arc<AtomicBool>);

impl Gate {
    fn open(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn pass(&self) {
        while !self.0.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// An Analyst that answers each kind of request only after its gate opens. A
/// kind it has no gate for fails at once.
#[derive(Default)]
struct SlowAnalyst {
    summary: Option<Gate>,
    groups: Option<Gate>,
    explanation: Option<Gate>,
}

impl Analyst for SlowAnalyst {
    fn summarize(&self, _: &SummaryRequest) -> Result<Summary, AnalystError> {
        let Some(gate) = &self.summary else {
            return Err(AnalystError::Failed("no summary today".into()));
        };
        gate.pass();
        Ok(Summary {
            text: "All done".into(),
            ..Summary::default()
        })
    }

    fn group(&self, request: &GroupRequest) -> Result<Grouping, AnalystError> {
        let Some(gate) = &self.groups else {
            return Err(AnalystError::Failed("no groups today".into()));
        };
        gate.pass();
        Ok(Grouping {
            groups: vec![Group {
                name: "Everything".into(),
                description: "all of it".into(),
                mechanical: false,
                files: request.files.clone(),
            }],
        })
    }

    fn explain(&self, _: &ExplainRequest) -> Result<Explanation, AnalystError> {
        let Some(gate) = &self.explanation else {
            return Err(AnalystError::Failed("no explanations today".into()));
        };
        gate.pass();
        Ok(Explanation {
            text: "Because".into(),
        })
    }
}

fn review_with(fx: &Fixture, analyst: SlowAnalyst, width: u16) -> Harness {
    let mut session = Session::open(fx.path(), width, 30).expect("session opens");
    session.set_analyst(Arc::new(analyst));
    Harness::from_session(session)
}

/// Let the event loop collect what has arrived until the screen shows `text`.
fn until_shown(h: &mut Harness, text: &str) {
    let start = Instant::now();
    loop {
        h.session().tick();
        if h.screen().contains(text) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "never showed {text:?}:\n{}",
            h.screen()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn gates() -> (Gate, Gate, Gate) {
    (Gate::default(), Gate::default(), Gate::default())
}

#[test]
fn the_status_line_shows_what_the_analyst_is_building_until_it_answers() {
    let fx = feature_branch();
    let (summary, groups, _) = gates();
    let mut h = review_with(
        &fx,
        SlowAnalyst {
            summary: Some(summary.clone()),
            groups: Some(groups.clone()),
            ..SlowAnalyst::default()
        },
        100,
    );
    let screen = h.screen();
    assert!(
        screen.contains("Analyst: Summary 0s, Groups 0s"),
        "{screen}"
    );

    h.wait(42_000);
    assert!(
        h.screen().contains("Analyst: Summary 42s, Groups 42s"),
        "{}",
        h.screen()
    );

    summary.open();
    groups.open();
    h.session().wait_for_analyst();
    assert!(!h.screen().contains("Analyst:"), "{}", h.screen());
}

#[test]
fn the_indicator_moves_with_time() {
    let fx = feature_branch();
    let (summary, _, _) = gates();
    let mut h = review_with(
        &fx,
        SlowAnalyst {
            summary: Some(summary.clone()),
            ..SlowAnalyst::default()
        },
        100,
    );
    let before = h.screen();
    h.wait(100);
    assert_ne!(before, h.screen());
    summary.open();
    h.session().wait_for_analyst();
}

#[test]
fn every_request_in_flight_is_listed_and_a_cancelled_explanation_still_counts() {
    let fx = feature_branch();
    let (summary, groups, explanation) = gates();
    let mut h = review_with(
        &fx,
        SlowAnalyst {
            summary: Some(summary.clone()),
            groups: Some(groups.clone()),
            explanation: Some(explanation.clone()),
        },
        100,
    );
    h.keys("j<Space>X<Esc>");
    let screen = h.screen();
    assert!(
        screen.contains("Analyst: Summary 0s, Groups 0s, Explanation 0s"),
        "{screen}"
    );

    summary.open();
    until_shown(&mut h, "Analyst: Groups 0s, Explanation 0s");
    groups.open();
    explanation.open();
    h.session().wait_for_analyst();
    assert!(!h.screen().contains("Analyst:"), "{}", h.screen());
}

#[test]
fn a_failure_and_the_indicator_show_together() {
    let fx = feature_branch();
    let (summary, _, _) = gates();
    let mut h = review_with(
        &fx,
        SlowAnalyst {
            summary: Some(summary.clone()),
            ..SlowAnalyst::default()
        },
        100,
    );
    until_shown(&mut h, "⚠ Groups: no groups today");
    let screen = h.screen();
    assert!(screen.contains("Analyst: Summary"), "{screen}");
    summary.open();
    h.session().wait_for_analyst();
}

#[test]
fn a_narrow_status_line_keeps_the_indicator_and_the_failure() {
    let fx = feature_branch();
    let (summary, _, _) = gates();
    let mut h = review_with(
        &fx,
        SlowAnalyst {
            summary: Some(summary.clone()),
            ..SlowAnalyst::default()
        },
        60,
    );
    until_shown(&mut h, "⚠ Groups");
    let screen = h.screen();
    assert!(screen.contains("Analyst: Summary"), "{screen}");
    assert!(screen.contains("⚠ Groups"), "{screen}");
    summary.open();
    h.session().wait_for_analyst();
}

#[test]
fn the_summary_overlay_and_the_sidebar_say_the_analyst_works() {
    let fx = feature_branch();
    let (summary, groups, _) = gates();
    let mut h = review_with(
        &fx,
        SlowAnalyst {
            summary: Some(summary.clone()),
            groups: Some(groups.clone()),
            ..SlowAnalyst::default()
        },
        100,
    );
    let screen = h.screen();
    assert!(screen.contains("grouping"), "{screen}");

    h.keys("<Space>s");
    h.wait(7_000);
    let screen = h.screen();
    assert!(
        screen.contains("The Analyst is working: the Summary is being built… 7s"),
        "{screen}"
    );

    summary.open();
    groups.open();
    h.session().wait_for_analyst();
    let screen = h.screen();
    assert!(screen.contains("All done"), "{screen}");
    h.keys("<Esc>");
    assert!(!h.screen().contains("grouping"), "{}", h.screen());
}

#[test]
fn the_event_loop_keeps_polling_while_any_request_is_in_flight() {
    let fx = feature_branch();
    let (_, groups, _) = gates();
    let mut h = review_with(
        &fx,
        SlowAnalyst {
            groups: Some(groups.clone()),
            ..SlowAnalyst::default()
        },
        100,
    );
    until_shown(&mut h, "⚠ Analyst: no summary today");
    assert!(h.session().analyst_busy());
    groups.open();
    h.session().wait_for_analyst();
    assert!(!h.session().analyst_busy());
}

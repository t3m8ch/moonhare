//! "Explain this": the Analyst explains the hunk or file under the cursor on
//! demand, with a fake standing in for the Analyst.

mod support;

use std::sync::{Arc, Mutex};

use moonhare::analyst::{
    Analyst, AnalystError, ExplainRequest, ExplainTarget, Explanation, Summary, SummaryRequest,
};
use moonhare::session::Session;
use support::{Fixture, Harness};

type Answer = Box<dyn Fn(&ExplainRequest) -> Result<Explanation, AnalystError> + Send + Sync>;

/// An Analyst that explains from memory and remembers what it was asked.
struct FakeAnalyst {
    answer: Answer,
    requests: Mutex<Vec<ExplainRequest>>,
}

impl FakeAnalyst {
    fn with(
        answer: impl Fn(&ExplainRequest) -> Result<Explanation, AnalystError> + Send + Sync + 'static,
    ) -> Arc<FakeAnalyst> {
        Arc::new(FakeAnalyst {
            answer: Box::new(answer),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn answering(text: &str) -> Arc<FakeAnalyst> {
        let text = text.to_string();
        Self::with(move |_| Ok(Explanation { text: text.clone() }))
    }

    fn requests(&self) -> Vec<ExplainRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Analyst for FakeAnalyst {
    fn summarize(&self, _: &SummaryRequest) -> Result<Summary, AnalystError> {
        Ok(Summary {
            text: "-".into(),
            ..Summary::default()
        })
    }

    fn explain(&self, request: &ExplainRequest) -> Result<Explanation, AnalystError> {
        self.requests.lock().unwrap().push(request.clone());
        (self.answer)(request)
    }
}

/// `main` has `src/a.rs` and `src/b.rs`; the branch changes a constant in
/// `src/a.rs` and adds `src/c.rs`.
fn modified_file() -> Fixture {
    let fx = Fixture::new();
    fx.write("src/a.rs", "fn a() {\n    1\n}\n")
        .write("src/b.rs", "fn b() {}\n")
        .commit("initial");
    fx.branch("feature");
    fx.write("src/a.rs", "fn a() {\n    10\n}\n")
        .write("src/c.rs", "fn c() {}\n")
        .commit("change a, add c");
    fx
}

fn review_with(fx: &Fixture, analyst: &Arc<FakeAnalyst>) -> Harness {
    let mut session = Session::open(fx.path(), 100, 30).expect("session opens");
    session.set_analyst(analyst.clone());
    Harness::from_session(session)
}

#[test]
fn space_x_explains_the_hunk_under_the_cursor_in_an_overlay() {
    let fx = modified_file();
    let analyst = FakeAnalyst::answering("Changes the constant");
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();

    // The first file is src/a.rs, whose hunk replaces `1` with `10`.
    h.keys("<Space>x");
    h.session().wait_for_analyst();

    let requests = analyst.requests();
    assert_eq!(requests.len(), 1);
    let ExplainTarget::Hunk { path, text } = &requests[0].target else {
        panic!("not a hunk: {:?}", requests[0].target);
    };
    assert_eq!(path, "src/a.rs");
    assert!(
        text.contains("+    10") && text.contains("-    1"),
        "{text}"
    );
    assert_eq!(requests[0].base, fx.git(&["rev-parse", "main"]).trim());
    assert_eq!(requests[0].head, fx.git(&["rev-parse", "feature"]).trim());
    let screen = h.screen();
    assert!(screen.contains("Changes the constant"), "{screen}");

    h.keys("<Esc>");
    assert!(!h.screen().contains("Changes the constant"));
}

#[test]
fn space_capital_x_explains_the_whole_file_under_the_cursor() {
    let fx = modified_file();
    let analyst = FakeAnalyst::answering("A tiny function");
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();

    h.keys("]f<Space>X");
    h.session().wait_for_analyst();

    let requests = analyst.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].target,
        ExplainTarget::File {
            path: "src/c.rs".into()
        }
    );
    let screen = h.screen();
    assert!(screen.contains("A tiny function"), "{screen}");
    assert!(screen.contains("src/c.rs"), "{screen}");
}

#[test]
fn the_explanation_is_asked_for_in_the_language_of_the_setting() {
    let fx = modified_file();
    let analyst = FakeAnalyst::with(|r| {
        Ok(Explanation {
            text: format!("Explained in {}", r.language),
        })
    });
    let mut session = Session::open(fx.path(), 100, 30).unwrap();
    session.set_language("en");
    session.set_analyst(analyst.clone());
    let mut h = Harness::from_session(session);
    h.keys("<Space>x");
    h.session().wait_for_analyst();
    assert_eq!(analyst.requests()[0].language, "en");
    assert!(h.screen().contains("Explained in en"), "{}", h.screen());
}

#[test]
fn while_waiting_the_overlay_says_so_and_escape_cancels_the_request() {
    let fx = modified_file();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let gate = Mutex::new(gate);
    let analyst = FakeAnalyst::with(move |_| {
        gate.lock().unwrap().recv().unwrap();
        Ok(Explanation {
            text: "Too late".into(),
        })
    });
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();

    h.keys("<Space>x");
    let screen = h.screen();
    assert!(screen.contains("explaining"), "{screen}");
    assert!(screen.contains("Esc"), "{screen}");

    h.keys("<Esc>");
    assert!(!h.screen().contains("explaining"));
    release.send(()).unwrap();
    h.session().wait_for_analyst();
    let screen = h.screen();
    assert!(!screen.contains("Too late"), "{screen}");
    h.keys("]f");
    assert!(h.screen().contains("src/c.rs"));
}

#[test]
fn an_analyst_failure_is_in_the_status_and_the_session_keeps_working() {
    let fx = modified_file();
    let analyst = FakeAnalyst::with(|_| {
        Err(AnalystError::NotLoggedIn(
            "claude is not logged in: run `claude /login`".into(),
        ))
    });
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();

    h.keys("<Space>x");
    h.session().wait_for_analyst();
    let screen = h.screen();
    let status = screen.lines().rev().nth(1).unwrap();
    assert!(status.contains("claude /login"), "{status}");

    h.keys("<Esc>]f");
    assert!(h.screen().contains("src/c.rs"));
    h.keys("<Space>X");
    h.session().wait_for_analyst();
    assert_eq!(analyst.requests().len(), 2, "the Reviewer may ask again");
}

#[test]
fn without_an_analyst_the_key_says_so_in_the_status() {
    let fx = modified_file();
    let mut h = Harness::open(&fx, 100, 30);
    h.keys("<Space>x");
    let screen = h.screen();
    assert!(
        screen.lines().rev().nth(1).unwrap().contains("No Analyst"),
        "{screen}"
    );
}

#[test]
fn a_file_without_a_hunk_has_nothing_to_explain() {
    let fx = Fixture::new();
    fx.write("a.bin", "x").commit("initial");
    fx.branch("feature");
    fx.write_bytes("a.bin", &[0, 1, 2, 0]).commit("binary");
    let analyst = FakeAnalyst::answering("never");
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    h.keys("<Space>x");
    h.session().wait_for_analyst();
    assert!(analyst.requests().is_empty());
    let screen = h.screen();
    assert!(
        screen.lines().rev().nth(1).unwrap().contains("No hunk"),
        "{screen}"
    );
}

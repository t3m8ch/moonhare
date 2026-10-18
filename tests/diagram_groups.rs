//! `:diagram group`: the diagram of the Group the cursor is in, with a fake
//! standing in for the Analyst that answers a fixed Mermaid text.

mod support;

use std::sync::{Arc, Mutex};

use moonhare::analyst::{
    Analyst, AnalystError, Diagram, DiagramRequest, DiagramSubject, Group, GroupRequest, Grouping,
    Summary, SummaryRequest,
};
use moonhare::session::Session;
use support::{feature_branch, Fixture, Harness};

const FLOW: &str = "graph LR\n    A[Collect] --> B[Render]";

struct FakeAnalyst {
    mermaid: String,
    /// Fails the first this-many diagram requests.
    failures: Mutex<usize>,
    requests: Mutex<Vec<DiagramRequest>>,
}

impl FakeAnalyst {
    fn drawing(mermaid: &str, failures: usize) -> Arc<FakeAnalyst> {
        Arc::new(FakeAnalyst {
            mermaid: mermaid.into(),
            failures: Mutex::new(failures),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn requests(&self) -> Vec<DiagramRequest> {
        self.requests.lock().unwrap().clone()
    }
}

fn group(name: &str, files: &[&str]) -> Group {
    Group {
        name: name.into(),
        description: format!("{name} files"),
        mechanical: false,
        files: files.iter().map(|f| f.to_string()).collect(),
    }
}

impl Analyst for FakeAnalyst {
    fn summarize(&self, _: &SummaryRequest) -> Result<Summary, AnalystError> {
        Ok(Summary::default())
    }

    fn group(&self, _: &GroupRequest) -> Result<Grouping, AnalystError> {
        Ok(Grouping {
            groups: vec![
                group("Core", &["src/a.rs", "src/b.rs"]),
                group("Docs", &["README.md"]),
            ],
        })
    }

    fn diagram(&self, request: &DiagramRequest) -> Result<Diagram, AnalystError> {
        self.requests.lock().unwrap().push(request.clone());
        let mut failures = self.failures.lock().unwrap();
        if *failures > 0 {
            *failures -= 1;
            return Err(AnalystError::Failed("timed out".into()));
        }
        Ok(Diagram {
            mermaid: self.mermaid.clone(),
        })
    }
}

fn review_with(fx: &Fixture, analyst: &Arc<FakeAnalyst>) -> Harness {
    let mut session = Session::open(fx.path(), 160, 30).expect("session opens");
    session.set_analyst(analyst.clone());
    let mut h = Harness::from_session(session);
    h.session().wait_for_analyst();
    h
}

#[test]
fn diagram_group_draws_the_group_of_the_file_under_the_cursor() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::drawing(FLOW, 0);
    let mut h = review_with(&fx, &analyst);

    h.keys("[f:diagram group<CR>");
    h.session().wait_for_analyst();

    let screen = h.screen();
    assert!(
        screen.contains("Collect") && screen.contains("Render"),
        "{screen}"
    );
    assert_eq!(
        analyst.requests()[0].subject,
        DiagramSubject::Group {
            name: "Core".into(),
            files: vec!["src/a.rs".into(), "src/b.rs".into()],
        }
    );
}

#[test]
fn a_failed_group_request_is_not_cached_and_the_next_diagram_asks_again() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::drawing(FLOW, 1);
    let mut h = review_with(&fx, &analyst);

    h.keys("[f:diagram group<CR>");
    h.session().wait_for_analyst();
    assert!(h.screen().contains("timed out"), "{}", h.screen());

    h.keys("q:diagram group<CR>");
    h.session().wait_for_analyst();
    assert!(h.screen().contains("Collect"), "{}", h.screen());
    assert_eq!(analyst.requests().len(), 2);
}

#[test]
fn the_group_diagram_shows_the_source_on_a_render_error_and_copies_it() {
    let fx = feature_branch();
    let source = "graph LR\n    A[Collect] --> ???";
    let analyst = FakeAnalyst::drawing(source, 0);
    let mut h = review_with(&fx, &analyst);

    h.keys("[f:diagram group<CR>");
    h.session().wait_for_analyst();
    assert!(h.screen().contains("Collect] --> ???"), "{}", h.screen());

    h.keys("y");
    assert_eq!(h.copied(), vec![source.to_string()]);
}

#[test]
fn a_wide_group_diagram_scrolls_sideways_with_h_and_l() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::drawing(&wide(), 0);
    let mut h = review_with(&fx, &analyst);

    h.keys("[f:diagram group<CR>");
    h.session().wait_for_analyst();
    assert!(h.screen().contains("Node0"), "{}", h.screen());

    h.keys("LL");
    assert!(!h.screen().contains("Node0"), "{}", h.screen());
    h.keys("0");
    assert!(h.screen().contains("Node0"), "{}", h.screen());
}

/// Fourteen nodes side by side: wider than the overlay however it is laid out.
fn wide() -> String {
    let mut text = String::from("graph TD\n");
    for i in 0..14 {
        text.push_str(&format!("    Root --> N{i}[Node{i}]\n"));
    }
    text
}

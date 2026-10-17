//! Diagrams: the `:diagram` overlay and the diagram in the Summary, with a
//! fake standing in for the Analyst that answers a fixed Mermaid text.

mod support;

use std::sync::{Arc, Mutex};

use moonhare::analyst::{
    Analyst, AnalystError, Diagram, DiagramRequest, DiagramSubject, Summary, SummaryRequest,
};
use moonhare::session::Session;
use support::{feature_branch, Fixture, Harness};

const FLOW: &str = "graph LR\n    A[Collect] --> B[Render]";

struct FakeAnalyst {
    mermaid: String,
    summary_diagram: Option<String>,
    requests: Mutex<Vec<DiagramRequest>>,
}

impl FakeAnalyst {
    fn drawing(mermaid: &str) -> Arc<FakeAnalyst> {
        Arc::new(FakeAnalyst {
            mermaid: mermaid.into(),
            summary_diagram: None,
            requests: Mutex::new(Vec::new()),
        })
    }

    fn requests(&self) -> Vec<DiagramRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Analyst for FakeAnalyst {
    fn summarize(&self, _: &SummaryRequest) -> Result<Summary, AnalystError> {
        Ok(Summary {
            text: "Summary text".into(),
            diagram: self.summary_diagram.clone(),
        })
    }

    fn diagram(&self, request: &DiagramRequest) -> Result<Diagram, AnalystError> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(Diagram {
            mermaid: self.mermaid.clone(),
        })
    }
}

fn review_with(fx: &Fixture, analyst: &Arc<FakeAnalyst>) -> Harness {
    let mut session = Session::open(fx.path(), 100, 30).expect("session opens");
    session.set_analyst(analyst.clone());
    Harness::from_session(session)
}

#[test]
fn diagram_command_draws_the_diagram_of_the_file_under_the_cursor() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::drawing(FLOW);
    let mut h = review_with(&fx, &analyst);

    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();

    let screen = h.screen();
    assert!(
        screen.contains("Collect") && screen.contains("Render"),
        "{screen}"
    );
    assert!(!screen.contains("graph LR"), "{screen}");
    assert_eq!(
        analyst.requests()[0].subject,
        DiagramSubject::File {
            path: "README.md".into()
        }
    );
}

#[test]
fn diagram_here_draws_the_fragment_around_the_cursor_line() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::drawing(FLOW);
    let mut h = review_with(&fx, &analyst);

    h.keys("jj:diagram here<CR>");
    h.session().wait_for_analyst();

    assert!(h.screen().contains("Collect"), "{}", h.screen());
    assert_eq!(
        analyst.requests()[0].subject,
        DiagramSubject::Fragment {
            path: "README.md".into(),
            line: 3
        }
    );
}

fn with_summary_diagram(mermaid: Option<&str>) -> Arc<FakeAnalyst> {
    Arc::new(FakeAnalyst {
        mermaid: FLOW.into(),
        summary_diagram: mermaid.map(String::from),
        requests: Mutex::new(Vec::new()),
    })
}

#[test]
fn the_summary_shows_the_diagram_the_analyst_attached() {
    let fx = feature_branch();
    let analyst = with_summary_diagram(Some("graph LR\n    X[Parser] --> Y[Printer]"));
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();

    h.keys("<Space>s");

    let screen = h.screen();
    assert!(screen.contains("Summary text"), "{screen}");
    assert!(
        screen.contains("Parser") && screen.contains("Printer"),
        "{screen}"
    );
    assert!(!screen.contains("graph LR"), "{screen}");
}

#[test]
fn the_summary_has_no_diagram_when_the_analyst_attached_none() {
    let fx = feature_branch();
    let analyst = with_summary_diagram(None);
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();

    h.keys("<Space>s");

    let screen = h.screen();
    assert!(screen.contains("Summary text"), "{screen}");
    assert!(!screen.contains("Parser"), "{screen}");
    assert!(analyst.requests().is_empty());
}

#[test]
fn the_summary_diagram_scrolls_with_h_and_l_and_copies_with_y() {
    let fx = feature_branch();
    let analyst = with_summary_diagram(Some(WIDE));
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    h.keys("<Space>s");
    assert!(!h.screen().contains("Foxtrot"), "{}", h.screen());

    h.keys("llllllll");
    assert!(h.screen().contains("Foxtrot"), "{}", h.screen());

    h.keys("y");
    assert_eq!(h.session().take_clipboard().as_deref(), Some(WIDE));

    h.keys("q<Space>s");
    assert!(h.screen().contains("Alpha"), "{}", h.screen());
}

/// An Analyst whose first diagram fails (a timeout, no login) and later ones work.
struct Flaky(Mutex<usize>);

impl Analyst for Flaky {
    fn summarize(&self, _: &SummaryRequest) -> Result<Summary, AnalystError> {
        Ok(Summary::default())
    }

    fn diagram(&self, _: &DiagramRequest) -> Result<Diagram, AnalystError> {
        let mut calls = self.0.lock().unwrap();
        *calls += 1;
        if *calls == 1 {
            Err(AnalystError::Failed("claude timed out".into()))
        } else {
            Ok(Diagram {
                mermaid: FLOW.into(),
            })
        }
    }
}

#[test]
fn a_failed_request_is_not_cached_and_the_next_diagram_asks_again() {
    let fx = feature_branch();
    let mut session = Session::open(fx.path(), 100, 30).unwrap();
    session.set_analyst(Arc::new(Flaky(Mutex::new(0))));
    let mut h = Harness::from_session(session);

    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
    assert!(h.screen().contains("timed out"), "{}", h.screen());

    h.keys("q:diagram<CR>");
    h.session().wait_for_analyst();
    assert!(h.screen().contains("Collect"), "{}", h.screen());
}

#[test]
fn control_characters_in_mermaid_never_reach_the_screen() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::drawing("graph LR\n    A[\u{1b}[31mred\u{7}] --> ???");
    let mut h = review_with(&fx, &analyst);
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
    let screen = h.screen();
    assert!(screen.contains("red"), "{screen}");
    assert!(
        !screen.chars().any(|c| c.is_control() && c != '\n'),
        "{screen:?}"
    );
}

#[test]
fn a_source_too_long_to_draw_shows_an_error_and_the_source() {
    let fx = feature_branch();
    let long = format!("graph LR\n{}", "A --> B\n".repeat(10_000));
    let analyst = FakeAnalyst::drawing(&long);
    let mut h = review_with(&fx, &analyst);
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
    let screen = h.screen();
    assert!(screen.contains("too long"), "{screen}");
    assert!(screen.contains("A --> B"), "{screen}");
}

#[test]
fn a_source_too_large_to_copy_is_refused() {
    let fx = feature_branch();
    let long = format!("graph LR\n{}", "A --> B\n".repeat(20_000));
    let analyst = FakeAnalyst::drawing(&long);
    let mut h = review_with(&fx, &analyst);
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
    h.keys("y");
    assert_eq!(h.session().take_clipboard(), None);
    assert!(h.screen().contains("too large"), "{}", h.screen());
}

#[test]
fn a_tall_diagram_scrolls_with_j_and_k() {
    let fx = feature_branch();
    let chain: Vec<String> = (0..40).map(|i| format!("N{i}[Step{i}]")).collect();
    let analyst = FakeAnalyst::drawing(&format!("graph TD\n    {}", chain.join(" --> ")));
    let mut h = review_with(&fx, &analyst);
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
    assert!(h.screen().contains("Step0"), "{}", h.screen());
    assert!(!h.screen().contains("Step39"), "{}", h.screen());

    h.keys("G");
    assert!(h.screen().contains("Step39"), "{}", h.screen());
    h.keys("gg");
    assert!(h.screen().contains("Step0"), "{}", h.screen());
    h.keys("jjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjj");
    assert!(!h.screen().contains("Step0"), "{}", h.screen());
    h.keys("kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk");
    assert!(h.screen().contains("Step0"), "{}", h.screen());
}

const WIDE: &str = "graph LR\n    A[Alpha stage] --> B[Bravo stage] --> C[Charlie stage] --> D[Delta stage] --> E[Echo stage] --> F[Foxtrot stage]";

const BROKEN: &str = "graph LR\n    A[Open --> ???";

#[test]
fn a_diagram_that_cannot_be_drawn_shows_its_mermaid_source() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::drawing(BROKEN);
    let mut h = review_with(&fx, &analyst);
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();

    let screen = h.screen();
    assert!(screen.contains("Cannot draw"), "{screen}");
    assert!(screen.contains("A[Open --> ???"), "{screen}");
}

#[test]
fn y_copies_the_mermaid_source_whether_or_not_it_could_be_drawn() {
    for source in [FLOW, BROKEN] {
        let fx = feature_branch();
        let analyst = FakeAnalyst::drawing(source);
        let mut h = review_with(&fx, &analyst);
        h.keys(":diagram<CR>");
        h.session().wait_for_analyst();
        assert_eq!(h.session().take_clipboard(), None);

        h.keys("y");

        assert_eq!(h.session().take_clipboard().as_deref(), Some(source));
        assert_eq!(h.session().take_clipboard(), None);
        assert!(h.screen().contains("Copied"), "{}", h.screen());
    }
}

#[test]
fn a_wide_diagram_scrolls_sideways_with_h_and_l() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::drawing(WIDE);
    let mut h = review_with(&fx, &analyst);
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
    assert!(h.screen().contains("Alpha"), "{}", h.screen());
    assert!(!h.screen().contains("Foxtrot"), "{}", h.screen());

    h.keys("llllllll");
    let scrolled = h.screen();
    assert!(scrolled.contains("Foxtrot"), "{scrolled}");
    assert!(!scrolled.contains("Alpha"), "{scrolled}");

    h.keys("hhhhhhhhhhhh");
    assert!(h.screen().contains("Alpha"), "{}", h.screen());
}

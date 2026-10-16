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

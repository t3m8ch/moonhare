//! The Summary, an Explanation and the description of a Group are Markdown:
//! the overlays show it drawn, not as marks, and `y` copies the source.

mod support;

use std::sync::Arc;

use moonhare::analyst::{
    Analyst, AnalystError, ExplainRequest, Explanation, Group, GroupRequest, Grouping, Summary,
    SummaryRequest,
};
use moonhare::session::Session;
use ratatui::style::Color;
use support::{feature_branch, Harness};

/// An Analyst that answers every question with the same Markdown.
struct Writer(String);

impl Analyst for Writer {
    fn summarize(&self, _: &SummaryRequest) -> Result<Summary, AnalystError> {
        Ok(Summary {
            text: self.0.clone(),
            ..Summary::default()
        })
    }

    fn explain(&self, _: &ExplainRequest) -> Result<Explanation, AnalystError> {
        Ok(Explanation {
            text: self.0.clone(),
        })
    }

    fn group(&self, request: &GroupRequest) -> Result<Grouping, AnalystError> {
        Ok(Grouping {
            groups: vec![Group {
                name: "All".into(),
                description: self.0.clone(),
                mechanical: false,
                files: request.files.clone(),
            }],
        })
    }
}

fn session(markdown: &str, width: u16, height: u16) -> Harness {
    let fx = feature_branch();
    let mut session = Session::open(fx.path(), width, height).expect("session opens");
    session.set_analyst(Arc::new(Writer(markdown.to_string())));
    let mut h = Harness::from_session(session);
    h.session().wait_for_analyst();
    // The fixture must outlive the session's git calls.
    std::mem::forget(fx);
    h
}

const SUMMARY: &str = "## Риски\n\nЭто **важно**, см. `load()`.\n\n\
| Операция | Файл |\n|---|---|\n| `load` | api/load.ts |\n| save | api/save.ts |\n";

/// No Markdown mark is left on the screen and the table has a frame.
fn drawn(screen: &str) {
    assert!(screen.contains("Риски"), "{screen}");
    assert!(!screen.contains("## Риски"), "{screen}");
    assert!(!screen.contains("**"), "{screen}");
    assert!(!screen.contains('`'), "{screen}");
    assert!(!screen.contains("|---"), "{screen}");
    assert!(screen.contains("Это важно, см. load()."), "{screen}");
    assert!(screen.contains("┌──────────┬"), "{screen}");
    assert!(screen.contains("│ load     │ api/load.ts │"), "{screen}");
}

#[test]
fn the_summary_shows_markdown_drawn() {
    let mut h = session(SUMMARY, 100, 30);
    h.keys("<Space>s");
    let screen = h.screen();
    drawn(&screen);

    let frame = h.frame();
    let (x, y) = frame.find("Риски").unwrap();
    assert_ne!(
        frame.fg_at(x, y),
        frame.fg_at(x, y + 2),
        "a heading has its colour"
    );
    let (x, y) = frame.find("load()").unwrap();
    assert_ne!(frame.bg_at(x, y), Color::Reset, "code has a background");
}

#[test]
fn an_explanation_shows_markdown_drawn() {
    let mut h = session(SUMMARY, 100, 30);
    h.keys("<Space>x");
    h.session().wait_for_analyst();
    drawn(&h.screen());
}

#[test]
fn the_description_of_a_group_shows_markdown_drawn() {
    let mut h = session(SUMMARY, 160, 30);
    h.keys("<Tab><Tab>gg<Space>i");
    drawn(&h.screen());
}

#[test]
fn y_copies_the_markdown_source() {
    let mut h = session(SUMMARY, 100, 30);
    h.keys("<Space>sy");
    assert_eq!(h.copied(), vec![SUMMARY.to_string()]);
    h.keys("q<Space>x");
    h.session().wait_for_analyst();
    h.keys("y");
    assert_eq!(h.copied(), vec![SUMMARY.to_string()]);
    h.keys("q<Tab><Tab>gg<Space>iy");
    assert_eq!(h.copied(), vec![SUMMARY.to_string()]);
}

#[test]
fn the_mouse_copies_the_text_as_drawn() {
    let mut h = session(SUMMARY, 100, 30);
    h.keys("<Space>s");
    let (_, y) = h.frame().find("Риски").unwrap();
    // The text of a 100-column screen starts at column 4 of the overlay.
    h.drag((4, y), (80, y));
    assert_eq!(h.copied(), vec!["Риски".to_string()]);
}

#[test]
fn a_long_markdown_summary_scrolls() {
    let items: Vec<String> = (1..=80).map(|n| format!("- **item{n:03}**")).collect();
    let mut h = session(&items.join("\n"), 100, 30);
    h.keys("<Space>s");
    let screen = h.screen();
    assert!(screen.contains("• item001"), "{screen}");
    assert!(!screen.contains("item080"), "{screen}");
    assert!(screen.contains("/80 "), "{screen}");
    h.keys("j");
    assert!(!h.screen().contains("item001"), "{}", h.screen());
    h.keys("G");
    let screen = h.screen();
    assert!(screen.contains("• item080"), "{screen}");
    assert!(screen.contains(" 80/80 "), "{screen}");
}

#[test]
fn a_resize_lays_the_table_out_again() {
    let row = "alpha beta gamma delta";
    let table = format!("| A | B | C |\n|---|---|---|\n| {row} | {row} | {row} |\n");
    let mut h = session(&table, 100, 30);
    h.keys("<Space>s");
    let screen = h.screen();
    assert!(screen.contains(&format!("│ {row} │ {row} │")), "{screen}");

    h.resize(60, 30);
    let screen = h.screen();
    assert!(!screen.contains(row), "{screen}");
    assert!(screen.contains("│ alpha"), "{screen}");
    assert!(screen.contains("┌"), "{screen}");

    // Ten columns do not fit the narrow box even squeezed: cards.
    let heads: Vec<String> = (0..10).map(|c| format!("col{c}")).collect();
    let values: Vec<String> = (0..10).map(|c| format!("val{c}")).collect();
    let wide = format!(
        "| {} |\n|{}\n| {} |\n",
        heads.join(" | "),
        "---|".repeat(10),
        values.join(" | ")
    );
    let mut h = session(&wide, 100, 30);
    h.keys("<Space>s");
    assert!(h.screen().contains("│ col0 │ col1 │"), "{}", h.screen());
    h.resize(60, 30);
    let screen = h.screen();
    assert!(!screen.contains("┼"), "{screen}");
    assert!(screen.contains("col3: val3"), "{screen}");
}

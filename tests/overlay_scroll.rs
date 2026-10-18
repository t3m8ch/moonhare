//! Every overlay with text longer than the screen scrolls: `j/k`, arrows,
//! `Ctrl-d/u`, `PageDown/PageUp`, `gg/G`, `Home/End` and the wheel, with the
//! position shown on the bottom border.

mod support;

use std::sync::Arc;

use moonhare::analyst::{
    Analyst, AnalystError, Diagram, DiagramRequest, ExplainRequest, Explanation, Summary,
    SummaryRequest,
};
use moonhare::session::Session;
use support::{feature_branch, Harness};

/// `count` short lines `row001`, `row002`, ...
fn rows(count: usize) -> String {
    (1..=count)
        .map(|n| format!("row{n:03}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// An Analyst whose answers are 80 lines long.
struct Wordy;

impl Analyst for Wordy {
    fn summarize(&self, _: &SummaryRequest) -> Result<Summary, AnalystError> {
        Ok(Summary {
            text: rows(80),
            ..Summary::default()
        })
    }

    fn explain(&self, _: &ExplainRequest) -> Result<Explanation, AnalystError> {
        Ok(Explanation { text: rows(80) })
    }

    fn diagram(&self, _: &DiagramRequest) -> Result<Diagram, AnalystError> {
        let chain: Vec<String> = (0..40).map(|i| format!("N{i}[Step{i}]")).collect();
        Ok(Diagram {
            mermaid: format!("graph TD\n    {}", chain.join(" --> ")),
        })
    }
}

fn session(width: u16, height: u16) -> Harness {
    let fx = feature_branch();
    let mut session = Session::open(fx.path(), width, height).expect("session opens");
    session.set_analyst(Arc::new(Wordy));
    let mut h = Harness::from_session(session);
    h.session().wait_for_analyst();
    // The fixture must outlive the session's git calls.
    std::mem::forget(fx);
    h
}

/// The `shown/total` on the bottom border of an overlay, if there is one;
/// a diagram names its rows as `строки first–shown / total`.
fn position(screen: &str) -> Option<(usize, usize)> {
    let mut found = None;
    for row in screen.lines().filter(|r| r.contains('└')) {
        if let Some((_, rows)) = row.split_once("строки ") {
            let numbers: Vec<usize> = rows
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|n| n.parse().ok())
                .collect();
            if let [_, shown, total, ..] = numbers[..] {
                found = Some((shown, total));
            }
            continue;
        }
        let after = &row[row.rfind('└')?..];
        for word in after.split(['─', ' ', '┘']) {
            if let Some((a, b)) = word.split_once('/') {
                if let (Ok(a), Ok(b)) = (a.parse(), b.parse()) {
                    found = Some((a, b));
                }
            }
        }
    }
    found
}

/// Run every way of scrolling on the overlay that is open and shows `first`
/// on its first row and `last` on its last.
fn scrolls(h: &mut Harness, first: &str, last: &str) {
    let screen = h.screen();
    assert!(screen.contains(first), "{screen}");
    assert!(!screen.contains(last), "{screen}");
    let (shown, total) = position(&screen).unwrap_or_else(|| panic!("no position:\n{screen}"));
    assert!(shown < total, "{screen}");

    for (down, up) in [
        ("j", "k"),
        ("<Down>", "<Up>"),
        ("<C-d>", "<C-u>"),
        ("<PageDown>", "<PageUp>"),
    ] {
        h.keys(down);
        let screen = h.screen();
        let moved = position(&screen).unwrap().0 - shown;
        // A line, or a page or half of one.
        assert_eq!(
            moved == 1,
            matches!(down, "j" | "<Down>"),
            "{down}:\n{screen}"
        );
        assert!(moved >= 1, "{down}:\n{screen}");
        if moved > 1 {
            assert!(!screen.contains(first), "{down}:\n{screen}");
        }
        h.keys(up);
        let screen = h.screen();
        assert_eq!(position(&screen).unwrap().0, shown, "{up}:\n{screen}");
    }

    h.keys("G");
    let screen = h.screen();
    assert!(screen.contains(last), "G:\n{screen}");
    assert!(!screen.contains(first), "G:\n{screen}");
    assert_eq!(position(&screen), Some((total, total)), "{screen}");
    h.keys("gg");
    assert!(h.screen().contains(first), "gg:\n{}", h.screen());
    h.keys("<End>");
    assert!(h.screen().contains(last), "End:\n{}", h.screen());
    h.keys("<Home>");
    assert!(h.screen().contains(first), "Home:\n{}", h.screen());

    h.wheel_down(50, 10);
    let screen = h.screen();
    assert_eq!(position(&screen).unwrap().0, shown + 3, "wheel:\n{screen}");
    h.wheel_up(50, 10);
    assert_eq!(position(&h.screen()).unwrap().0, shown, "wheel");
    // A lone g is not a jump.
    h.keys("jjjgk");
    assert_eq!(
        position(&h.screen()).unwrap().0,
        shown + 2,
        "{}",
        h.screen()
    );
    h.keys("gg");
    assert!(h.screen().contains(first), "{}", h.screen());
}

#[test]
fn the_summary_scrolls() {
    let mut h = session(100, 30);
    h.keys("<Space>s");
    scrolls(&mut h, "row001", "row080");
    h.keys("<Esc>");
    assert!(!h.screen().contains("row001"), "{}", h.screen());
}

#[test]
fn the_explanation_scrolls() {
    let mut h = session(100, 30);
    h.keys("<Space>x");
    h.session().wait_for_analyst();
    scrolls(&mut h, "row001", "row080");
    h.keys("q");
    assert!(!h.screen().contains("row001"), "{}", h.screen());
}

#[test]
fn the_diagram_scrolls() {
    let mut h = session(100, 30);
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
    scrolls(&mut h, "Step0", "Step39");
}

#[test]
fn the_prompt_scrolls() {
    let mut h = session(100, 30);
    let text: String = (1..=60).map(|n| format!("row{n:03}<CR>")).collect();
    h.keys(&format!("Ci{text}<Esc><CR>"));
    h.keys("<Space>ps");
    let screen = h.screen();
    assert!(screen.contains("Prompt"), "{screen}");
    scrolls(&mut h, "You are the author", "row060");
}

#[test]
fn the_help_scrolls() {
    let mut h = session(100, 20);
    h.keys("?");
    let first = "hjkl везде";
    scrolls(&mut h, first, "Ctrl-w W");
    h.keys("?");
    assert!(!h.screen().contains(first), "{}", h.screen());
}

#[test]
fn a_long_comment_draft_follows_the_cursor_and_scrolls_in_normal_mode() {
    let mut h = session(100, 20);
    let text: String = (1..=40).map(|n| format!("row{n:03}<CR>")).collect();
    h.keys(&format!("Ci{text}end"));
    let screen = h.screen();
    assert!(screen.contains("end█"), "{screen}");
    assert!(!screen.contains("row001"), "{screen}");
    h.keys("<Esc>");
    h.keys("gg");
    assert!(h.screen().contains("row001"), "{}", h.screen());
    h.keys("G");
    assert!(h.screen().contains("Enter - save"), "{}", h.screen());
    h.wheel_up(50, 10);
    assert!(!h.screen().contains("Enter - save"), "{}", h.screen());
    // Typing again brings the cursor back into view.
    h.keys("ix");
    assert!(h.screen().contains("endx█"), "{}", h.screen());
}

#[test]
fn long_lines_wrap_to_the_window_and_scrolling_counts_wrapped_rows() {
    struct Prose;
    impl Analyst for Prose {
        fn summarize(&self, _: &SummaryRequest) -> Result<Summary, AnalystError> {
            let words: Vec<String> = (1..=300).map(|n| format!("w{n:03}")).collect();
            Ok(Summary {
                text: format!("{} {}", words.join(" "), "x".repeat(200)),
                ..Summary::default()
            })
        }
    }
    let fx = feature_branch();
    let mut session = Session::open(fx.path(), 60, 15).expect("session opens");
    session.set_analyst(Arc::new(Prose));
    let mut h = Harness::from_session(session);
    h.session().wait_for_analyst();
    h.keys("<Space>s");
    let screen = h.screen();
    assert!(screen.contains("w001"), "{screen}");
    assert!(!screen.contains("w300"), "{screen}");
    h.keys("G");
    let screen = h.screen();
    assert!(screen.contains("xxxxxxxx"), "{screen}");
    assert!(!screen.contains("w001"), "{screen}");
    // The position survives a resize; a taller window fits everything it can.
    h.resize(60, 40);
    let screen = h.screen();
    assert!(screen.contains("xxxxxxxx"), "{screen}");
    h.resize(60, 15);
    assert!(h.screen().contains("xxxxxxxx"), "{}", h.screen());
    h.keys("gg");
    assert!(h.screen().contains("w001"), "{}", h.screen());
}

#[test]
fn scrolling_works_in_a_tiny_terminal() {
    let mut h = session(60, 15);
    h.keys("<Space>s");
    h.keys("G");
    assert!(h.screen().contains("row080"), "{}", h.screen());
    h.keys("gg<C-d><C-d><PageDown><C-u>");
    h.resize(60, 20);
    h.keys("Gk");
    assert!(h.screen().contains("row079"), "{}", h.screen());
    h.keys("<Esc>");
    assert!(!h.screen().contains("row0"), "{}", h.screen());
}

#[test]
fn the_wheel_scrolls_then_a_drag_copies_what_is_shown() {
    let mut h = session(100, 30);
    h.keys("<Space>s");
    h.wheel_down(50, 10);
    let screen = h.screen();
    assert!(!screen.contains("row001"), "{screen}");
    let (x, y) = h.frame().find("row004").expect("row004");
    h.drag((x, y), (x + 5, y));
    assert_eq!(h.copied(), vec!["row004".to_string()]);
    // Esc still closes, with a selection made.
    h.keys("<Esc>");
    assert!(!h.screen().contains("row004"), "{}", h.screen());
}

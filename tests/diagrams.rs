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
fn the_summary_diagram_scrolls_with_h_and_l_and_y_copies_it_with_the_summary() {
    let fx = feature_branch();
    let tree = big_tree();
    let analyst = with_summary_diagram(Some(&tree));
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    h.keys("<Space>s");
    assert!(h.screen().contains("Column0Step0"), "{}", h.screen());
    assert!(!h.screen().contains("Column11Step0"), "{}", h.screen());

    h.keys("$");
    assert!(h.screen().contains("Column11Step0"), "{}", h.screen());
    assert!(!h.screen().contains("Column0Step0"), "{}", h.screen());
    h.keys("h");
    assert!(h.screen().contains("Column11Step0"), "{}", h.screen());

    // y copies the Markdown of the Summary, the diagram as a Mermaid block.
    h.keys("y");
    let copied = h.copied();
    assert_eq!(copied.len(), 1);
    assert!(copied[0].starts_with("Summary text"), "{copied:?}");
    assert!(
        copied[0].ends_with(&format!("```mermaid\n{}\n```\n", tree.trim_end())),
        "{copied:?}"
    );

    h.keys("q<Space>s");
    assert!(h.screen().contains("Column0Step0"), "{}", h.screen());
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
    assert!(h.copied().is_empty());
    assert!(h.screen().contains("Too large"), "{}", h.screen());
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
        assert!(h.copied().is_empty());

        h.keys("y");

        assert_eq!(h.copied(), vec![source.to_string()]);
        assert!(h.copied().is_empty());
        assert!(h.screen().contains("Copied"), "{}", h.screen());
    }
}

#[test]
fn a_wide_left_to_right_diagram_is_turned_to_fit_and_y_still_copies_it_as_written() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::drawing(WIDE);
    let mut session = Session::open(fx.path(), 100, 40).expect("session opens");
    session.set_analyst(analyst.clone());
    let mut h = Harness::from_session(session);
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
    let screen = h.screen();
    for name in ["Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot"] {
        assert!(screen.contains(name), "{name}:\n{screen}");
    }
    assert!(!screen.contains("колонки"), "{screen}");

    h.keys("y");
    assert_eq!(h.copied(), vec![WIDE.to_string()]);
}

#[test]
fn the_diagram_is_fitted_again_when_the_terminal_is_resized() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::drawing(
        "graph TD\n    A[the first step of a rather long process that goes on for a while] --> B[the second step]",
    );
    let mut h = review_with(&fx, &analyst);
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
    let whole = "the first step of a rather long process that goes on for a while";
    assert!(h.screen().contains(whole), "{}", h.screen());

    h.resize(64, 30);
    let narrow = h.screen();
    assert!(!narrow.contains(whole), "{narrow}");
    assert!(narrow.contains("while"), "{narrow}");
    assert!(!narrow.contains("колонки"), "{narrow}");

    for (w, ht) in [(20, 6), (1, 1), (0, 0), (100, 30)] {
        h.resize(w, ht);
        h.keys("ljL$G");
        let _ = h.screen();
    }
    assert!(h.screen().contains(whole), "{}", h.screen());
}

/// A tree wider and taller than any window: twelve columns of sixteen steps
/// whose names cannot be wrapped, under one root.
fn big_tree() -> String {
    let mut lines = vec!["flowchart TD".to_string()];
    for c in 0..12 {
        lines.push(format!(
            "    Root[TheRootOfEverything] --> C{c}S0[Column{c}Step0]"
        ));
        for s in 1..16 {
            lines.push(format!(
                "    C{c}S{}[Column{c}Step{}] --> C{c}S{s}[Column{c}Step{s}]",
                s - 1,
                s - 1
            ));
        }
    }
    lines.join("\n")
}

fn open_big(h: &mut Harness) {
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
}

fn big() -> (Fixture, Harness) {
    let fx = feature_branch();
    let analyst = FakeAnalyst::drawing(&big_tree());
    let mut h = review_with(&fx, &analyst);
    open_big(&mut h);
    (fx, h)
}

/// `(first, last, all)` of the columns or rows the border names.
fn position(screen: &str, what: &str) -> (usize, usize, usize) {
    let at = screen
        .find(&format!("{what} "))
        .unwrap_or_else(|| panic!("no {what} on the border:\n{screen}"));
    let rest = &screen[at + what.len() + 1..];
    let numbers: Vec<usize> = rest
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .take(3)
        .map(|s| s.parse().unwrap())
        .collect();
    (numbers[0], numbers[1], numbers[2])
}

/// The markers on the border of the overlay of a 100x30 terminal, in the
/// order left, right, top, bottom.
fn markers(screen: &str) -> String {
    let rows: Vec<Vec<char>> = screen.lines().map(|l| l.chars().collect()).collect();
    let at = |x: usize, y: usize| rows.get(y).and_then(|r| r.get(x)).copied();
    let mut found = String::new();
    if (3..27).any(|y| at(2, y) == Some('◀')) {
        found.push('◀');
    }
    if (3..27).any(|y| at(97, y) == Some('▶')) {
        found.push('▶');
    }
    if (3..97).any(|x| at(x, 2) == Some('▲')) {
        found.push('▲');
    }
    if (3..97).any(|x| at(x, 27) == Some('▼')) {
        found.push('▼');
    }
    found
}

#[test]
fn the_view_opens_on_the_top_of_the_diagram_with_its_top_block_in_sight() {
    let (_fx, mut h) = big();
    let screen = h.screen();
    // The root is centred over twelve columns, far right of the left edge:
    // the view is moved to it rather than opening on blank space.
    assert!(screen.contains("TheRootOfEverything"), "{screen}");
    assert_eq!(position(&screen, "строки").0, 1, "{screen}");

    // Reopened after moving, it starts over.
    h.keys("jjjq:diagram<CR>");
    assert_eq!(position(&h.screen(), "строки").0, 1);
}

#[test]
fn a_wide_and_tall_diagram_shows_markers_and_its_position_on_the_border() {
    let (_fx, mut h) = big();
    h.keys("0gg");
    let screen = h.screen();
    let (first, last, all) = position(&screen, "колонки");
    assert_eq!((first, last), (1, 92), "{screen}");
    assert!(all > 92, "{screen}");
    let (top, bottom, rows) = position(&screen, "строки");
    assert_eq!((top, bottom), (1, 24), "{screen}");
    assert!(rows > 24, "{screen}");
    assert_eq!(markers(&screen), "▶▼", "{screen}");

    h.keys("$G");
    let screen = h.screen();
    assert_eq!(position(&screen, "колонки").1, all, "{screen}");
    assert_eq!(position(&screen, "строки").1, rows, "{screen}");
    assert_eq!(markers(&screen), "◀▲", "{screen}");
}

#[test]
fn h_j_k_l_and_the_arrows_move_by_a_column_and_a_row() {
    let (_fx, mut h) = big();
    h.keys("0gg");
    h.keys("ll");
    assert_eq!(position(&h.screen(), "колонки").0, 3);
    h.keys("h");
    assert_eq!(position(&h.screen(), "колонки").0, 2);
    h.keys("jjj");
    assert_eq!(position(&h.screen(), "строки").0, 4);
    h.keys("k");
    assert_eq!(position(&h.screen(), "строки").0, 3);
    h.keys("<Down><Up><Up>");
    assert_eq!(position(&h.screen(), "строки").0, 2);
}

#[test]
fn big_steps_move_by_half_a_window_and_a_page() {
    let (_fx, mut h) = big();
    h.keys("0gg");
    h.keys("L");
    assert_eq!(position(&h.screen(), "колонки").0, 47);
    h.keys("H");
    assert_eq!(position(&h.screen(), "колонки").0, 1);
    h.keys("<C-d>");
    assert_eq!(position(&h.screen(), "строки").0, 13);
    h.keys("<C-u>");
    assert_eq!(position(&h.screen(), "строки").0, 1);
    h.keys("<C-f>");
    assert_eq!(position(&h.screen(), "строки").0, 24);
    h.keys("<C-b>");
    assert_eq!(position(&h.screen(), "строки").0, 1);
}

#[test]
fn the_wheel_moves_down_and_shift_with_the_wheel_moves_sideways() {
    let (_fx, mut h) = big();
    h.keys("0gg");
    h.wheel_down(50, 15);
    assert_eq!(position(&h.screen(), "строки").0, 4);
    h.wheel_up(50, 15);
    assert_eq!(position(&h.screen(), "строки").0, 1);
    h.shift_wheel_down(50, 15);
    let moved = position(&h.screen(), "колонки").0;
    assert!(moved > 1, "{}", h.screen());
    h.wheel_right(50, 15);
    assert!(position(&h.screen(), "колонки").0 > moved);
}

#[test]
fn a_diagram_that_fits_shows_no_markers_and_no_position() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::drawing(FLOW);
    let mut h = review_with(&fx, &analyst);
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
    let screen = h.screen();
    assert!(!screen.contains("колонки"), "{screen}");
    assert_eq!(markers(&screen), "", "{screen}");
}

#[test]
fn shift_with_the_wheel_moves_the_summary_diagram_sideways() {
    let fx = feature_branch();
    let analyst = with_summary_diagram(Some(&big_tree()));
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    h.keys("<Space>s");
    assert!(h.screen().contains("Column0Step0"), "{}", h.screen());

    for _ in 0..10 {
        h.shift_wheel_down(50, 15);
    }
    assert!(!h.screen().contains("Column0Step0"), "{}", h.screen());
    h.keys("0");
    assert!(h.screen().contains("Column0Step0"), "{}", h.screen());
}

/// The centre of the view along the columns, as a share of the picture.
fn centre(screen: &str) -> f64 {
    let (first, last, all) = position(screen, "колонки");
    (first + last) as f64 / 2.0 / all as f64
}

#[test]
fn minus_closes_the_layout_up_plus_spreads_it_out_and_equals_fits_it_again() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::drawing(FLOW);
    let mut h = review_with(&fx, &analyst);
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
    let row = |screen: &str| {
        screen
            .lines()
            .find(|l| l.contains("Collect"))
            .unwrap()
            .to_string()
    };
    let fitted = row(&h.screen());

    h.keys("-");
    let compact = row(&h.screen());
    assert!(
        compact.trim_end().len() < fitted.trim_end().len(),
        "{compact}\n{fitted}"
    );

    h.keys("=+");
    let roomy = row(&h.screen());
    assert!(
        roomy.trim_end().len() > fitted.trim_end().len(),
        "{roomy}\n{fitted}"
    );

    h.keys("=");
    assert_eq!(row(&h.screen()), fitted);
}

#[test]
fn a_change_of_density_keeps_the_centre_of_the_view() {
    let (_fx, mut h) = big();
    h.keys("0ggLLL");
    let before = centre(&h.screen());
    let all = position(&h.screen(), "колонки").2;

    h.keys("+");
    let roomy = h.screen();
    assert!(position(&roomy, "колонки").2 > all, "{roomy}");
    assert!((centre(&roomy) - before).abs() < 0.05, "{before} {roomy}");

    h.keys("=");
    assert!(
        (centre(&h.screen()) - before).abs() < 0.05,
        "{}",
        h.screen()
    );
}

/// Whether the screen shows Braille dots.
fn dotted(screen: &str) -> bool {
    screen
        .chars()
        .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
}

#[test]
fn the_overview_shows_the_whole_diagram_in_dots_with_the_frame_of_the_view() {
    let (_fx, mut h) = big();
    h.keys("0gg--");
    let screen = h.screen();
    assert!(dotted(&screen), "{screen}");
    assert!(!screen.contains("Column0Step0"), "{screen}");
    assert!(screen.contains("обзор"), "{screen}");
    assert!(!screen.contains("колонки"), "{screen}");

    // The frame of the view is lit, at the top left of the overview.
    let selection = moonhare::theme::DARK.selection_bg;
    let frame = h.frame();
    let lit: Vec<(u16, u16)> = (0..100)
        .flat_map(|x| (0..30).map(move |y| (x, y)))
        .filter(|&(x, y)| frame.bg_at(x, y) == selection)
        .collect();
    assert!(!lit.is_empty(), "{screen}");
    let left = lit.iter().map(|p| p.0).min().unwrap();
    assert!(left <= 5, "{lit:?}");

    // Moved to the right edge, the frame follows; Enter goes there.
    h.keys("$G");
    let frame = h.frame();
    let right = (0..100)
        .filter(|&x| (0..30).any(|y| frame.bg_at(x, y) == selection))
        .max()
        .unwrap();
    assert!(right > 50, "{}", h.screen());
    h.keys("<CR>");
    let screen = h.screen();
    assert!(!dotted(&screen), "{screen}");
    let (_, last, all) = position(&screen, "колонки");
    assert_eq!(last, all, "{screen}");
    let (_, bottom, rows) = position(&screen, "строки");
    assert_eq!(bottom, rows, "{screen}");
}

#[test]
fn equals_leaves_the_overview_for_the_fitted_layout() {
    let (_fx, mut h) = big();
    h.keys("--");
    assert!(dotted(&h.screen()));
    h.keys("-");
    assert!(dotted(&h.screen()));
    h.keys("=");
    let screen = h.screen();
    assert!(!dotted(&screen), "{screen}");
    assert!(screen.contains("Step"), "{screen}");
    h.keys("--+");
    assert!(!dotted(&h.screen()), "{}", h.screen());
}

#[test]
fn the_summary_diagram_changes_density_too() {
    let fx = feature_branch();
    let analyst = with_summary_diagram(Some(FLOW));
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    h.keys("<Space>s");
    let row = |screen: &str| {
        screen
            .lines()
            .find(|l| l.contains("Collect"))
            .unwrap()
            .trim_end()
            .len()
    };
    let fitted = row(&h.screen());
    h.keys("+");
    assert!(row(&h.screen()) > fitted, "{}", h.screen());
    h.keys("=-");
    assert!(row(&h.screen()) < fitted, "{}", h.screen());
    // No overview inline: it is drawn in the diagram's own window.
    h.keys("-");
    assert!(!dotted(&h.screen()), "{}", h.screen());
}

#[test]
fn f_shows_the_diagram_on_the_whole_terminal_and_f_or_esc_go_back() {
    let (_fx, mut h) = big();
    h.keys("0gg");
    assert!(h.screen().contains("┌ Diagram"), "{}", h.screen());

    h.keys("f");
    let screen = h.screen();
    // No border, no sidebar, no status line: the picture and a line under it.
    assert!(!screen.contains("┌ Diagram"), "{screen}");
    assert!(!screen.contains("Commits"), "{screen}");
    assert!(!screen.contains("NORMAL"), "{screen}");
    assert_eq!(position(&screen, "колонки").1, 100, "{screen}");
    assert_eq!(position(&screen, "строки").1, 29, "{screen}");
    assert!(screen.lines().last().unwrap().contains("f/Esc"), "{screen}");

    // Keys and the wheel move it as in the window.
    h.keys("j");
    assert_eq!(position(&h.screen(), "строки").0, 2);
    h.wheel_down(50, 15);
    assert_eq!(position(&h.screen(), "строки").0, 5);

    h.keys("f");
    assert!(h.screen().contains("┌ Diagram"), "{}", h.screen());
    h.keys("f<Esc>");
    assert!(h.screen().contains("┌ Diagram"), "{}", h.screen());
    h.keys("q");
    assert!(!h.screen().contains("┌ Diagram"), "{}", h.screen());
}

#[test]
fn full_screen_lays_the_diagram_out_for_the_whole_terminal() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::drawing(WIDE);
    let mut session = Session::open(fx.path(), 220, 40).expect("session opens");
    session.set_analyst(analyst.clone());
    let mut h = Harness::from_session(session);
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
    let row = |screen: &str, name: &str| screen.lines().position(|l| l.contains(name));
    let windowed = h.screen();
    assert_ne!(
        row(&windowed, "Alpha"),
        row(&windowed, "Foxtrot"),
        "{windowed}"
    );

    h.keys("f");
    let full = h.screen();
    assert_eq!(row(&full, "Alpha"), row(&full, "Foxtrot"), "{full}");
}

#[test]
fn f_in_the_summary_shows_its_diagram_on_the_whole_terminal() {
    let fx = feature_branch();
    let analyst = with_summary_diagram(Some(&big_tree()));
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    h.keys("<Space>s");
    assert!(h.screen().contains("Summary text"), "{}", h.screen());

    h.keys("f");
    let screen = h.screen();
    assert!(!screen.contains("Summary text"), "{screen}");
    assert!(screen.contains("TheRootOfEverything"), "{screen}");
    assert!(screen.contains("колонки"), "{screen}");
    h.keys("Lj");
    assert_eq!(position(&h.screen(), "строки").0, 2);

    h.keys("<Esc>");
    assert!(h.screen().contains("Summary text"), "{}", h.screen());
    h.keys("f");
    assert!(!h.screen().contains("Summary text"));
    h.keys("f");
    assert!(h.screen().contains("Summary text"), "{}", h.screen());
}

#[test]
fn f_in_a_summary_without_a_diagram_says_so() {
    let fx = feature_branch();
    let analyst = with_summary_diagram(None);
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    h.keys("<Space>sf");
    let screen = h.screen();
    assert!(screen.contains("Summary text"), "{screen}");
    assert!(screen.contains("no diagram"), "{screen}");
}

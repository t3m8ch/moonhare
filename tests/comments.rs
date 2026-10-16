//! Comments: three kinds, the in-app editor, states, hidden-Comments cue and
//! persistence between runs.

mod support;

use support::{feature_branch, Harness};

/// Index of the first screen row containing `needle`.
fn row_of(screen: &str, needle: &str) -> usize {
    screen
        .lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no row with {needle:?}:\n{screen}"))
}

#[test]
fn c_opens_an_editor_and_a_saved_line_comment_hangs_under_its_line() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    // README.md: "# demo", "", "more".
    h.keys("jjcifix this<Esc><CR>");
    let screen = h.screen();
    let code = row_of(&screen, "more");
    let comment = row_of(&screen, "fix this");
    assert!(
        comment > code && comment <= code + 3,
        "comment is not under its line:\n{screen}"
    );
    assert!(screen.contains("NORMAL"), "{screen}");
}

#[test]
fn a_second_esc_with_text_asks_before_losing_the_draft() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("jjcidraft text<Esc><Esc>");
    let screen = h.screen();
    assert!(screen.contains("Discard"), "{screen}");
    assert!(screen.contains("draft text"), "{screen}");

    // `n` goes back to the editor with the text intact.
    h.keys("n");
    assert_eq!(badge(&mut h), "COMMENT");
    assert!(h.screen().contains("draft text"));

    // `y` throws the draft away; nothing is saved.
    h.keys("<Esc>y");
    let screen = h.screen();
    assert!(!screen.contains("draft text"), "{screen}");
    assert_eq!(badge(&mut h), "NORMAL");
}

#[test]
fn esc_on_an_empty_draft_cancels_at_once() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("jjc<Esc>");
    assert_eq!(badge(&mut h), "NORMAL");
    h.keys("ci<Esc><Esc>");
    assert_eq!(badge(&mut h), "NORMAL");
}

#[test]
fn c_in_visual_comments_the_whole_selection_under_its_last_line() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("jvj");
    assert_eq!(badge(&mut h), "VISUAL");
    h.keys("cirange note<Esc><CR>");
    let screen = h.screen();
    assert_eq!(badge(&mut h), "NORMAL");
    assert!(screen.contains("new 2-3"), "{screen}");
    let code = row_of(&screen, "more");
    assert!(row_of(&screen, "range note") > code, "{screen}");
}

/// The mode badge at the start of the status line.
fn badge(h: &mut Harness) -> String {
    let screen = h.screen();
    let last = screen.lines().last().unwrap().to_string();
    last.split_whitespace().next().unwrap_or("").to_string()
}

#[test]
fn the_status_line_names_the_editor_mode() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("jjc");
    assert_eq!(badge(&mut h), "COMMENT");
    h.keys("i");
    assert_eq!(badge(&mut h), "INSERT");
    h.keys("<Esc>");
    assert_eq!(badge(&mut h), "COMMENT");
    h.keys("<CR>");
    assert_eq!(badge(&mut h), "NORMAL");
}

//! The "only unreviewed" filter (`Space u`): the queue of files still to read,
//! the delta since a file was marked, and how `m` / `M` behave inside it.

mod support;

use support::{feature_branch, Harness};

/// The sidebar lines (first 32 columns).
fn sidebar(h: &mut Harness) -> String {
    h.screen()
        .lines()
        .map(|l| l.chars().take(32).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_filter_hides_fully_reviewed_files_and_keeps_the_rest() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]fm");
    assert!(sidebar(&mut h).contains("a.rs"));

    h.keys(" u");
    let side = sidebar(&mut h);
    assert!(!side.contains("a.rs"), "{side}");
    assert!(
        side.contains("README.md") && side.contains("b.rs"),
        "{side}"
    );
    assert!(side.contains("Files (2)"), "{side}");
}

/// The Reviewer marked everything, then the Author agent changed `src/a.rs`.
fn marked_then_changed() -> support::Fixture {
    let fx = feature_branch();
    Harness::open(&fx, 160, 30).keys("m]fm]fm");
    fx.write("src/a.rs", "fn a() {\n    10\n    20\n}\n")
        .commit("agent extends a");
    fx
}

/// The diff pane only (right of the sidebar).
fn diff_pane(h: &mut Harness) -> String {
    h.screen()
        .lines()
        .map(|l| l.chars().skip(33).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_file_changed_after_the_mark_shows_only_the_delta_from_the_reviewed_version() {
    let fx = marked_then_changed();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(" u");
    let side = sidebar(&mut h);
    assert!(
        side.contains("Files (1)") && side.contains("a.rs"),
        "{side}"
    );
    assert!(!side.contains("README.md"), "{side}");

    let pane = diff_pane(&mut h);
    assert!(pane.contains("modified"), "{pane}");
    assert!(!pane.contains("added"), "{pane}");
    assert!(pane.contains("20"), "{pane}");
}

#[test]
fn switching_the_filter_off_brings_back_the_full_diff() {
    let fx = marked_then_changed();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(" u u");
    let pane = diff_pane(&mut h);
    assert!(pane.contains("added"), "{pane}");
    assert!(!pane.contains("modified"), "{pane}");
}

/// Path of the file under the sidebar cursor.
fn current(h: &mut Harness) -> &'static str {
    let side = sidebar(h);
    let row = side
        .lines()
        .find(|l| l.starts_with("│>"))
        .unwrap_or_else(|| panic!("no cursor row:\n{side}"));
    ["README.md", "src/a.rs", "src/b.rs"]
        .into_iter()
        .find(|p| row.contains(p.rsplit('/').next().unwrap()))
        .unwrap_or_else(|| panic!("unexpected cursor row {row}"))
}

#[test]
fn m_takes_the_file_out_of_the_queue_and_the_next_one_takes_its_place() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(" u]fm");
    let side = sidebar(&mut h);
    assert!(
        !side.contains("a.rs") && side.contains("Files (2)"),
        "{side}"
    );
    assert_eq!(current(&mut h), "src/b.rs");
    // The mark was saved: without the filter the file shows as reviewed.
    h.keys(" u");
    assert!(sidebar(&mut h).contains("✓"), "{}", sidebar(&mut h));
}

#[test]
fn capital_m_marks_and_moves_on_without_skipping_a_file() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(" uM");
    assert_eq!(current(&mut h), "src/a.rs");
    assert!(sidebar(&mut h).contains("Files (2)"));
    h.keys("M");
    assert_eq!(current(&mut h), "src/b.rs");
    assert!(sidebar(&mut h).contains("Files (1)"));
}

#[test]
fn an_empty_queue_says_so() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(" uMMM");
    let screen = h.screen();
    assert!(screen.contains("Files (0)"), "{screen}");
    assert!(
        diff_pane(&mut h).contains("Nothing left to review"),
        "{screen}"
    );
}

#[test]
fn switching_the_filter_off_brings_back_every_file_and_the_status_tells_the_state() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]fm");
    assert!(!h.screen().contains("only unreviewed"), "{}", h.screen());

    h.keys(" u");
    assert!(h.screen().contains("only unreviewed"), "{}", h.screen());

    h.keys(" u");
    let side = sidebar(&mut h);
    assert!(
        side.contains("a.rs") && side.contains("Files (3)"),
        "{side}"
    );
    assert!(!h.screen().contains("only unreviewed"), "{}", h.screen());
}

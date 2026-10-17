//! A file whose content is the same on both ends of the Scope (a pure rename,
//! a mode change, an edit that was undone) is not Reviewed until the Reviewer
//! marks it, and the mark can be taken back.

mod support;

use support::{Fixture, Harness};

/// Enough lines that git still sees a rename after one of them changes.
fn demo(title: &str) -> String {
    format!("# {title}\n{}", "line\n".repeat(10))
}

fn base() -> Fixture {
    let fx = Fixture::new();
    fx.write("README.md", &demo("demo")).commit("initial");
    fx.write("other.txt", "other\n").commit("other");
    fx.branch("feature");
    fx
}

/// `m` marks the file, `m` again takes the mark back; before the first `m`
/// nothing is Reviewed, and the unreviewed filter lists the file.
fn assert_needs_an_explicit_mark(fx: &Fixture) {
    let mut h = Harness::open(fx, 160, 30);
    let screen = h.screen();
    assert!(screen.contains("reviewed 0/1"), "{screen}");
    assert!(!screen.contains('✓'), "{screen}");

    h.keys(" u");
    assert!(h.screen().contains("Files (1)"), "{}", h.screen());
    h.keys(" u");

    h.keys("m");
    let screen = h.screen();
    assert!(screen.contains("reviewed 1/1"), "{screen}");
    assert!(screen.contains('✓'), "{screen}");

    h.keys("m");
    let screen = h.screen();
    assert!(screen.contains("reviewed 0/1"), "{screen}");
    assert!(!screen.contains('✓'), "{screen}");
}

const LATER: u64 = 5_000;

/// One file, `README.md`, edited by the first commit of `feature`.
fn edited() -> Fixture {
    let fx = base();
    fx.write("README.md", &demo("edited")).commit("edit");
    fx
}

fn chmod(fx: &Fixture) {
    fx.git(&["update-index", "--chmod=+x", "README.md"]);
    fx.git(&["commit", "-q", "-m", "chmod"]);
}

#[test]
fn a_mode_change_added_after_the_review_is_not_reviewed_by_the_earlier_mark() {
    let fx = edited();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("m");
    assert!(h.screen().contains("reviewed 1/1"), "{}", h.screen());

    chmod(&fx);
    h.wait(LATER);
    h.keys(":reload<CR>");

    let screen = h.screen();
    assert!(screen.contains("reviewed 0/1"), "{screen}");
    h.keys(" u");
    assert!(h.screen().contains("Files (1)"), "{}", h.screen());
}

#[test]
fn a_mode_change_slipped_into_the_middle_of_a_reviewed_chain_is_not_reviewed() {
    let fx = edited();
    fx.write("README.md", &demo("finished")).commit("finish");
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("m");
    assert!(h.screen().contains("reviewed 1/1"), "{}", h.screen());

    // The agent rewrites the history with a chmod between the two edits.
    fx.git(&["reset", "-q", "--hard", "main"]);
    fx.write("README.md", &demo("edited")).commit("edit");
    chmod(&fx);
    fx.write("README.md", &demo("finished")).commit("finish");
    h.wait(LATER);
    h.keys(":reload<CR>");

    assert!(h.screen().contains("reviewed 0/1"), "{}", h.screen());
}

#[test]
fn a_chain_with_a_mode_change_inside_is_reviewed_once_all_of_it_is_marked() {
    let fx = edited();
    chmod(&fx);
    fx.write("README.md", &demo("finished")).commit("finish");
    let mut h = Harness::open(&fx, 160, 30);
    assert!(h.screen().contains("reviewed 0/1"), "{}", h.screen());

    h.keys("m");

    assert!(h.screen().contains("reviewed 1/1"), "{}", h.screen());
}

#[test]
fn a_pure_rename_is_unreviewed_until_marked() {
    let fx = base();
    fx.git(&["mv", "README.md", "DOC.md"]);
    fx.commit("rename");
    assert_needs_an_explicit_mark(&fx);
}

#[test]
fn a_mode_change_is_unreviewed_until_marked() {
    let fx = base();
    fx.git(&["update-index", "--chmod=+x", "README.md"]);
    fx.git(&["commit", "-q", "-m", "chmod"]);
    assert_needs_an_explicit_mark(&fx);
}

#[test]
fn a_rename_whose_edit_was_undone_is_unreviewed_until_marked() {
    let fx = base();
    fx.git(&["mv", "README.md", "DOC.md"]);
    fx.write("DOC.md", &demo("changed"))
        .commit("rename and edit");
    fx.write("DOC.md", &demo("demo")).commit("undo the edit");
    assert_needs_an_explicit_mark(&fx);
}

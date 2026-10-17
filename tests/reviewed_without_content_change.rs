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

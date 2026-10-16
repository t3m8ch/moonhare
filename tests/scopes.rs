//! Scopes and the commit list: which commits' combined change is displayed.

mod support;

use support::{Fixture, Harness};

/// `main` holds `a_shared.txt` with `value = 0`; five commits on `feature`
/// each set it to `value = N` and add `cN.txt`.
fn five_commits() -> Fixture {
    let fx = Fixture::new();
    fx.write("a_shared.txt", "value = 0\n").commit("initial");
    fx.branch("feature");
    for (n, name) in [
        (1, "one"),
        (2, "two"),
        (3, "three"),
        (4, "four"),
        (5, "five"),
    ] {
        fx.write("a_shared.txt", &format!("value = {n}\n"))
            .write(&format!("c{n}.txt"), &format!("file {name}\n"))
            .commit(&format!("commit {name}"));
    }
    fx
}

fn row_with<'a>(screen: &'a str, needle: &str) -> Option<&'a str> {
    screen.lines().find(|l| l.contains(needle))
}

#[test]
fn commit_list_has_the_pinned_range_row_and_every_commit() {
    let fx = five_commits();
    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    assert!(screen.contains("▣ Весь Range"), "{screen}");
    assert!(screen.contains("Commits"), "{screen}");
    for name in ["one", "two", "three", "four", "five"] {
        assert!(screen.contains(&format!("commit {name}")), "{screen}");
    }
}

/// The `value = A ... value = B` row of the first file's diff.
fn value_row(h: &mut Harness) -> String {
    let screen = h.screen();
    let row = row_with(&screen, "value = ").unwrap_or_else(|| panic!("no value row:\n{screen}"));
    // Only the diff pane, not the sidebar to its left.
    row[row.find("value = ").unwrap()..].to_string()
}

#[test]
fn moving_the_cursor_changes_nothing_and_enter_applies_a_single_commit() {
    let fx = five_commits();
    let mut h = Harness::open(&fx, 160, 30);
    let whole = value_row(&mut h);
    assert!(
        whole.contains("value = 0") && whole.contains("value = 5"),
        "{whole}"
    );

    h.keys("<Tab>jjj");
    assert_eq!(value_row(&mut h), whole, "moving does not change the Scope");

    h.keys("<CR>");
    let row = value_row(&mut h);
    assert!(
        row.contains("value = 2") && row.contains("value = 3"),
        "{row}"
    );
    let screen = h.screen();
    assert!(screen.contains("c3.txt"), "{screen}");
    assert!(
        !screen.contains("c2.txt"),
        "only the commit's files:\n{screen}"
    );
}

#[test]
fn v_and_motion_select_a_contiguous_scope_of_commits() {
    let fx = five_commits();
    let mut h = Harness::open(&fx, 160, 30);

    h.keys("<Tab>jjjvjj<CR>");
    let row = value_row(&mut h);
    // The combined change of commits 3-5: from the state after commit 2.
    assert!(
        row.contains("value = 2") && row.contains("value = 5"),
        "{row}"
    );
    let screen = h.screen();
    for kept in ["c3.txt", "c4.txt", "c5.txt"] {
        assert!(screen.contains(kept), "{kept} listed:\n{screen}");
    }
    assert!(!screen.contains("c2.txt"), "{screen}");
}

#[test]
fn a_selection_never_includes_the_range_row_and_escape_cancels_it() {
    let fx = five_commits();
    let mut h = Harness::open(&fx, 160, 30);

    // `v` on the Range row does nothing: the Scope is the one commit reached.
    h.keys("<Tab>vjj<CR>");
    let row = value_row(&mut h);
    assert!(
        row.contains("value = 1") && row.contains("value = 2"),
        "{row}"
    );

    // Moving up in a selection stops at the first commit.
    // The cursor is on commit 2; select from commit 4 up past the top.
    h.keys("jjvkkkk<CR>");
    let row = value_row(&mut h);
    assert!(
        row.contains("value = 0") && row.contains("value = 4"),
        "{row}"
    );

    // Esc drops the selection: Enter then applies only the commit under the cursor.
    h.keys("jjjvj<Esc><CR>");
    let row = value_row(&mut h);
    assert!(
        row.contains("value = 4") && row.contains("value = 5"),
        "{row}"
    );
}

#[test]
fn scope_command_takes_numbers_or_all() {
    let fx = five_commits();
    let mut h = Harness::open(&fx, 160, 30);

    h.keys(":scope 2-4<CR>");
    let row = value_row(&mut h);
    assert!(
        row.contains("value = 1") && row.contains("value = 4"),
        "{row}"
    );

    h.keys(":scope all<CR>");
    let row = value_row(&mut h);
    assert!(
        row.contains("value = 0") && row.contains("value = 5"),
        "{row}"
    );

    h.keys(":scope 4<CR>");
    let row = value_row(&mut h);
    assert!(
        row.contains("value = 3") && row.contains("value = 4"),
        "{row}"
    );

    // Bad input reports and keeps the Scope.
    h.keys(":scope 4-9<CR>");
    assert!(h.screen().contains("No such commits"), "{}", h.screen());
    assert_eq!(value_row(&mut h), row);
}

#[test]
fn a_and_the_range_row_return_to_the_whole_range() {
    let fx = five_commits();
    let mut h = Harness::open(&fx, 160, 30);
    let whole = value_row(&mut h);

    h.keys(":scope 2<CR>");
    assert_ne!(value_row(&mut h), whole);
    h.keys("a");
    assert_eq!(value_row(&mut h), whole, "a returns to the whole Range");

    h.keys(":scope 2<CR><Tab>");
    assert_ne!(value_row(&mut h), whole);
    h.keys("kk<CR>");
    assert_eq!(value_row(&mut h), whole, "Enter on the Range row");
}

#[test]
fn the_commit_list_marks_the_current_scope_and_the_cursor() {
    let fx = five_commits();
    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    let marked = |screen: &str, needle: &str| row_with(screen, needle).unwrap().contains('●');
    assert!(
        marked(&screen, "Весь Range"),
        "whole Range is the Scope:\n{screen}"
    );
    assert!(!marked(&screen, "commit two"), "{screen}");

    h.keys(":scope 2-3<CR>");
    let screen = h.screen();
    assert!(!marked(&screen, "Весь Range"), "{screen}");
    assert!(!marked(&screen, "commit one"), "{screen}");
    assert!(
        marked(&screen, "commit two") && marked(&screen, "commit three"),
        "{screen}"
    );
    assert!(!marked(&screen, "commit four"), "{screen}");
    assert!(
        screen.contains("scope:2-3"),
        "status line names the Scope:\n{screen}"
    );

    // The cursor is shown only while the commit list has focus.
    assert!(
        !row_with(&screen, "commit three").unwrap().contains('>'),
        "{screen}"
    );
    h.keys("<Tab>j");
    let screen = h.screen();
    assert!(
        row_with(&screen, "commit three").unwrap().contains('>'),
        "{screen}"
    );
    assert!(
        screen.contains("Commits 3/5"),
        "position in the title:\n{screen}"
    );
}

#[test]
fn space_c_opens_a_picker_that_selects_a_scope_like_the_list_and_closes() {
    let fx = five_commits();
    let mut h = Harness::open(&fx, 160, 30);

    h.keys("<Space>c");
    let screen = h.screen();
    assert!(screen.contains("Pick a Scope"), "picker is open:\n{screen}");

    // Moving does not apply; Enter applies a selection and closes.
    h.keys("jjvj");
    assert!(value_row(&mut h).contains("value = 5"), "not applied yet");
    h.keys("<CR>");
    let screen = h.screen();
    assert!(!screen.contains("Pick a Scope"), "picker closed:\n{screen}");
    let row = value_row(&mut h);
    assert!(
        row.contains("value = 1") && row.contains("value = 3"),
        "{row}"
    );

    // Esc closes without applying; a inside the picker picks the whole Range.
    h.keys("<Space>cjj<Esc>");
    assert!(!h.screen().contains("Pick a Scope"));
    assert!(value_row(&mut h).contains("value = 3"), "Scope unchanged");
    h.keys("<Space>ca");
    assert!(!h.screen().contains("Pick a Scope"));
    assert!(value_row(&mut h).contains("value = 5"), "whole Range");
}

#[test]
fn clicking_a_commit_moves_the_cursor_without_applying() {
    let fx = five_commits();
    let mut h = Harness::open(&fx, 160, 30);
    let whole = value_row(&mut h);
    let (x, y) = h.frame().find("commit four").expect("row");
    h.click(x, y);
    let screen = h.screen();
    assert!(
        row_with(&screen, "commit four").unwrap().contains('>'),
        "{screen}"
    );
    assert_eq!(value_row(&mut h), whole);
    h.keys("<CR>");
    assert!(value_row(&mut h).contains("value = 4"));
}

#[test]
fn a_long_commit_list_scrolls_under_the_pinned_range_row() {
    let fx = Fixture::new();
    fx.write("f.txt", "0\n").commit("initial");
    fx.branch("feature");
    for n in 1..=30 {
        fx.write("f.txt", &format!("{n}\n"))
            .commit(&format!("step {n:02}"));
    }
    let mut h = Harness::open(&fx, 160, 30);

    h.keys("<Tab>jjjjjjjjjjjjjjjjjjjj");
    let screen = h.screen();
    assert!(screen.contains("Весь Range"), "pinned row stays:\n{screen}");
    assert!(screen.contains("step 20"), "cursor row visible:\n{screen}");
    assert!(
        !screen.contains("step 01"),
        "scrolled past the top:\n{screen}"
    );
    assert!(screen.contains("Commits 20/30"), "{screen}");
}

#[test]
fn bracket_c_steps_through_single_commits_at_once() {
    let fx = five_commits();
    let mut h = Harness::open(&fx, 160, 30);

    h.keys("]c");
    let row = value_row(&mut h);
    assert!(
        row.contains("value = 0") && row.contains("value = 1"),
        "{row}"
    );
    h.keys("]c]c");
    let row = value_row(&mut h);
    assert!(
        row.contains("value = 2") && row.contains("value = 3"),
        "{row}"
    );
    h.keys("[c");
    let row = value_row(&mut h);
    assert!(
        row.contains("value = 1") && row.contains("value = 2"),
        "{row}"
    );

    // The ends hold.
    h.keys("]c]c]c]c]c]c");
    let row = value_row(&mut h);
    assert!(
        row.contains("value = 4") && row.contains("value = 5"),
        "{row}"
    );
    h.keys(":scope 1<CR>[c");
    let row = value_row(&mut h);
    assert!(
        row.contains("value = 0") && row.contains("value = 1"),
        "{row}"
    );

    // From the whole Range, [c lands on the last commit.
    h.keys("a[c");
    let row = value_row(&mut h);
    assert!(
        row.contains("value = 4") && row.contains("value = 5"),
        "{row}"
    );
}

#[test]
fn a_scope_spanning_a_rename_and_a_later_edit_shows_one_renamed_file() {
    let body = "fn alpha() {\n    1\n}\n\nfn beta() {\n    2\n}\n\nfn gamma() {\n    3\n}\n";
    let fx = Fixture::new();
    fx.write("old_name.rs", body).commit("initial");
    fx.branch("feature");
    fx.write("other.txt", "other\n").commit("unrelated");
    fx.git(&["mv", "old_name.rs", "new_name.rs"]);
    fx.commit("rename");
    fx.write("new_name.rs", &body.replace("    2\n", "    22\n"))
        .commit("edit");
    let mut h = Harness::open(&fx, 160, 30);

    h.keys(":scope 2-3<CR>");
    let screen = h.screen();
    assert!(screen.contains("Files (1)"), "{screen}");
    let header = row_with(&screen, "renamed").unwrap_or_else(|| panic!("renamed:\n{screen}"));
    assert!(
        header.contains("old_name.rs") && header.contains("new_name.rs"),
        "{screen}"
    );
    assert!(screen.contains("22"), "the edit is in the Scope:\n{screen}");

    // The edit alone is a plain modification of the new path.
    h.keys(":scope 3<CR>");
    let screen = h.screen();
    assert!(!screen.contains("renamed"), "{screen}");
    assert!(screen.contains("new_name.rs"), "{screen}");
}

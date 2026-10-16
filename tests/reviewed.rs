//! Reviewed marks: marking files in a Scope, partial state, progress,
//! navigation over unreviewed files and persistence inside `.git`.

mod support;

use support::{feature_branch, Harness};

/// The sidebar line (first 32 columns) of a file.
fn file_row(h: &mut Harness, path: &str) -> String {
    let screen = h.screen();
    screen
        .lines()
        .map(|l| l.chars().take(32).collect::<String>())
        .find(|l| l.contains(path))
        .unwrap_or_else(|| panic!("no row for {path}:\n{screen}"))
}

/// Put the cursor on the file with this index in the list.
fn select(h: &mut Harness, index: usize) {
    h.keys("[f[f[f");
    for _ in 0..index {
        h.keys("]f");
    }
}

#[test]
fn m_marks_the_selected_file_and_updates_progress() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    assert!(!file_row(&mut h, "README.md").contains('✓'));
    assert!(h.screen().contains("reviewed 0/3"), "{}", h.screen());
    h.keys("m");
    assert!(file_row(&mut h, "README.md").contains('✓'));
    assert!(!file_row(&mut h, "src/a.rs").contains('✓'));
    assert!(h.screen().contains("reviewed 1/3"), "{}", h.screen());
}

#[test]
fn a_mark_in_one_commit_makes_the_file_partial_until_every_part_is_marked() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    // src/a.rs is changed by commit 1 and commit 3.
    h.keys(":scope 1<CR>");
    select(&mut h, 0);
    h.keys("m");
    assert!(file_row(&mut h, "src/a.rs").contains('✓'));

    h.keys(":scope all<CR>");
    let row = file_row(&mut h, "src/a.rs");
    assert!(row.contains('◐') && !row.contains('✓'), "{row}");
    assert!(h.screen().contains("reviewed 0/3"), "{}", h.screen());

    h.keys(":scope 3<CR>");
    assert!(!file_row(&mut h, "src/a.rs").contains('✓'));
    h.keys("m:scope all<CR>");
    assert!(file_row(&mut h, "src/a.rs").contains('✓'));
}

#[test]
fn a_mark_in_the_whole_range_covers_every_commit_that_touched_the_file() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]fm");
    assert!(file_row(&mut h, "src/a.rs").contains('✓'));
    for scope in ["1", "3", "1-3"] {
        h.keys(&format!(":scope {scope}<CR>"));
        assert!(file_row(&mut h, "src/a.rs").contains('✓'), "scope {scope}");
    }
    h.keys(":scope 2<CR>");
    assert!(!file_row(&mut h, "src/b.rs").contains('✓'));
}

#[test]
fn progress_counters_follow_the_scope() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]fm");
    let screen = h.screen();
    assert!(screen.contains("reviewed 1/3 [██░░░░░░]"), "{screen}");
    assert!(screen.contains("whole Range 1/3"), "{screen}");

    h.keys(":scope 2<CR>");
    let screen = h.screen();
    assert!(screen.contains("reviewed 0/2"), "{screen}");
    assert!(screen.contains("whole Range 1/3"), "{screen}");

    h.keys(":scope 3<CR>");
    let screen = h.screen();
    assert!(screen.contains("reviewed 1/1 [████████]"), "{screen}");
}

#[test]
fn m_again_takes_the_mark_back() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("m");
    assert!(file_row(&mut h, "README.md").contains('✓'));
    h.keys("m");
    assert!(!file_row(&mut h, "README.md").contains('✓'));
}

//! Reviewed marks: marking files in a Scope, partial state, progress,
//! navigation over unreviewed files and persistence inside `.git`.

mod support;

use support::{feature_branch, Harness};

/// The sidebar line (first 32 columns) of a file: the tree shows its name
/// under its directory.
fn file_row(h: &mut Harness, path: &str) -> String {
    let screen = h.screen();
    let name = path.rsplit('/').next().unwrap();
    screen
        .lines()
        .map(|l| l.chars().take(32).collect::<String>())
        .find(|l| l.contains(name))
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

/// Path of the file under the sidebar cursor.
fn current(h: &mut Harness) -> &'static str {
    let screen = h.screen();
    let row = screen
        .lines()
        .map(|l| l.chars().take(32).collect::<String>())
        .find(|l| l.starts_with("│>"))
        .unwrap_or_else(|| panic!("no cursor row:\n{screen}"));
    ["README.md", "src/a.rs", "src/b.rs"]
        .into_iter()
        .find(|p| row.contains(p.rsplit('/').next().unwrap()))
        .unwrap_or_else(|| panic!("unexpected cursor row {row}"))
}

#[test]
fn capital_m_marks_and_moves_to_the_next_unreviewed_file() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("M");
    assert!(file_row(&mut h, "README.md").contains('✓'));
    assert_eq!(current(&mut h), "src/a.rs");
    h.keys("]f]fM");
    // Nothing unreviewed is left after b.rs: the cursor stays and says so.
    assert_eq!(current(&mut h), "src/b.rs");
    assert!(file_row(&mut h, "src/b.rs").contains('✓'));
    assert!(h.screen().contains("No more unreviewed"), "{}", h.screen());
}

#[test]
fn u_jumps_over_reviewed_files_and_f_always_reaches_them() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]fm]f");
    // README is unreviewed, a.rs reviewed, b.rs unreviewed; cursor on b.rs.
    assert_eq!(current(&mut h), "src/b.rs");
    h.keys("[u");
    assert_eq!(current(&mut h), "README.md");
    h.keys("]u");
    assert_eq!(current(&mut h), "src/b.rs");
    h.keys("[f");
    assert_eq!(current(&mut h), "src/a.rs");
    h.keys("[f]f");
    assert_eq!(current(&mut h), "src/a.rs");
}

#[test]
fn marks_survive_a_restart_and_follow_the_branch_name() {
    let fx = feature_branch();
    Harness::open(&fx, 160, 30).keys("]fm]fm");

    let mut h = Harness::open(&fx, 160, 30);
    assert!(file_row(&mut h, "src/a.rs").contains('✓'));
    assert!(file_row(&mut h, "src/b.rs").contains('✓'));
    assert!(!file_row(&mut h, "README.md").contains('✓'));
    assert!(h.screen().contains("reviewed 2/3"), "{}", h.screen());

    // The same Review is reached from another checkout by branch name.
    fx.checkout("main");
    let target = moonhare::git::Target::Branch("feature".into());
    let mut h = Harness::open_target(&fx, &target, 160, 30);
    assert!(file_row(&mut h, "src/a.rs").contains('✓'));
}

#[test]
fn an_explicit_range_is_its_own_review_and_the_working_copy_stays_clean() {
    let fx = feature_branch();
    Harness::open(&fx, 160, 30).keys("m");
    let range = moonhare::git::Target::from_arg(Some("main..feature"));
    let mut h = Harness::open_target(&fx, &range, 160, 30);
    assert!(!file_row(&mut h, "README.md").contains('✓'));
    h.keys("m");
    drop(h);
    // Neither the branch nor the working copy is touched by marking.
    assert_eq!(fx.git(&["status", "--porcelain"]), "");
    assert!(fx
        .git(&["for-each-ref", "refs/heads"])
        .contains("refs/heads/feature"));
}

#[test]
fn marks_survive_rewritten_history_when_the_content_is_the_same() {
    let fx = feature_branch();
    Harness::open(&fx, 160, 30).keys("]fm");
    // Amending the message rewrites the SHA of the last commit.
    fx.git(&["commit", "-q", "--amend", "-m", "change a (reworded)"]);
    let mut h = Harness::open(&fx, 160, 30);
    assert!(file_row(&mut h, "src/a.rs").contains('✓'));

    // Changing the file again invalidates only that file's mark.
    fx.write("src/a.rs", "fn a() {\n    11\n}\n")
        .commit("tweak a");
    let mut h = Harness::open(&fx, 160, 30);
    assert!(file_row(&mut h, "src/a.rs").contains('◐'));
}

#[test]
fn reviewed_versions_survive_git_gc() {
    let fx = feature_branch();
    let newest = fx
        .git(&["rev-parse", "feature:src/a.rs"])
        .trim()
        .to_string();
    let first = fx
        .git(&["rev-parse", "feature~2:src/a.rs"])
        .trim()
        .to_string();
    Harness::open(&fx, 160, 30).keys("]fm");

    // Drop every other way to reach the commits, then collect garbage.
    fx.checkout("main");
    fx.git(&["branch", "-D", "feature"]);
    fx.git(&["reflog", "expire", "--expire=now", "--all"]);
    fx.git(&["gc", "-q", "--prune=now"]);

    for blob in [newest, first] {
        fx.git(&["cat-file", "-e", &blob]);
    }
}

#[test]
fn reset_clears_the_review_and_its_pins() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("m]fm");
    assert!(h.screen().contains("reviewed 2/3"), "{}", h.screen());
    assert!(!fx.git(&["for-each-ref", "refs/moonhare"]).is_empty());

    h.keys(":reset<CR>");
    assert!(h.screen().contains("reviewed 0/3"), "{}", h.screen());
    assert!(!file_row(&mut h, "README.md").contains('✓'));
    assert_eq!(fx.git(&["for-each-ref", "refs/moonhare"]), "");

    let mut h = Harness::open(&fx, 160, 30);
    assert!(h.screen().contains("reviewed 0/3"), "{}", h.screen());
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

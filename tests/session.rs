mod support;

use support::{feature_branch, Fixture, Harness};

#[test]
fn branch_of_several_commits_lists_every_changed_file_and_shows_the_first_diff() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);

    let screen = h.screen();
    for file in ["README.md", "src/a.rs", "src/b.rs"] {
        assert!(screen.contains(file), "sidebar lists {file}:\n{screen}");
    }
    // The diff of the first file (README.md) spans the whole range.
    assert!(screen.contains("more"), "README diff visible:\n{screen}");
}

#[test]
fn diff_spans_the_whole_range_not_individual_commits() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);

    h.keys("]f");
    let screen = h.screen();
    // src/a.rs was added in one commit and changed in another; only its final
    // content is shown, as one added file.
    assert!(screen.contains("10"), "final version visible:\n{screen}");
    assert!(
        !screen.lines().any(|l| l.trim_end().ends_with(" 1")),
        "intermediate version hidden:\n{screen}"
    );

    h.keys("]f");
    assert!(h.screen().contains("fn b()"), "third file shown");
}

#[test]
fn files_switch_with_bracket_f_and_by_clicking_the_list() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    assert!(h.screen().contains("file 1/3"));

    h.keys("]f");
    assert!(h.screen().contains("file 2/3"));
    h.keys("]f]f]f");
    assert!(h.screen().contains("file 3/3"), "stops at the last file");
    h.keys("[f");
    assert!(h.screen().contains("file 2/3"));

    // Click the row the file list shows for src/b.rs, wherever the panes sit.
    let (x, y) = h
        .frame()
        .find("src/b.rs")
        .expect("src/b.rs is listed in the sidebar");
    h.click(x, y);
    assert!(h.screen().contains("file 3/3"), "click selects src/b.rs");
}

#[test]
fn created_deleted_and_binary_files_are_shown() {
    let fx = Fixture::new();
    fx.write("gone.txt", "bye bye\n")
        .write("blob.bin", "")
        .commit("initial");
    fx.branch("feature");
    fx.remove("gone.txt")
        .write_bytes("blob.bin", &[0, 1, 2, 3])
        .write("fresh.txt", "hello fresh\n")
        .commit("mixed");

    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    for f in ["gone.txt", "blob.bin", "fresh.txt"] {
        assert!(screen.contains(f), "{f} listed:\n{screen}");
    }
    // Sorted: blob.bin, fresh.txt, gone.txt.
    assert!(screen.contains("Binary file not shown"), "{screen}");
    h.keys("]f");
    assert!(h.screen().contains("hello fresh"));
    h.keys("]f");
    assert!(h.screen().contains("bye bye"));
}

#[test]
fn branch_without_changes_says_so() {
    let fx = Fixture::new();
    fx.write("README.md", "x\n").commit("initial");
    fx.branch("feature");
    let mut h = Harness::open(&fx, 160, 30);
    assert!(h.screen().contains("No changes"));
}

#[test]
fn opening_outside_a_repository_fails_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    assert!(moonhare::session::Session::open(dir.path(), 160, 30).is_err());
}

#[test]
fn opening_and_using_the_session_leaves_the_repository_untouched() {
    let fx = feature_branch();
    // A dirty working copy: an edit to a tracked file and an untracked file.
    fx.write("src/a.rs", "fn a() { dirty }\n")
        .write("scratch.txt", "untracked\n");
    let before = fx.state();

    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]f]fGggjk<C-d><C-u>s[f:q<CR>")
        .click(5, 2)
        .wheel_down(100, 5);
    h.resize(100, 30).resize(40, 10);
    let _ = h.screen();
    drop(h);

    assert_eq!(fx.state(), before);
}

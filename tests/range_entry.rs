mod support;

use moonhare::git::Target;
use support::{feature_branch, Fixture, Harness};

/// `main` moved on (`on_main.txt`) after `feature` was cut from it.
fn main_moved_on_after_branching() -> Fixture {
    let fx = Fixture::new();
    fx.write("README.md", "# demo\n").commit("initial");
    fx.branch("feature");
    fx.write("mine.txt", "mine\n").commit("feature work");
    fx.checkout("main");
    fx.write("on_main.txt", "main only\n")
        .commit("main moves on");
    fx
}

#[test]
fn base_of_a_branch_cut_from_main_is_the_fork_point_even_after_main_moved_on() {
    let fx = main_moved_on_after_branching();
    let mut h = Harness::open_target(&fx, &Target::Branch("feature".into()), 160, 30);

    let screen = h.screen();
    assert!(screen.contains("mine.txt"), "{screen}");
    assert!(!screen.contains("on_main.txt"), "{screen}");
}

#[test]
fn a_merge_commit_in_the_range_is_warned_about_and_a_plain_range_is_not() {
    let fx = main_moved_on_after_branching();
    fx.checkout("feature");
    fx.git(&["merge", "-q", "--no-ff", "-m", "merge main", "main"]);
    fx.write("later.txt", "later\n").commit("feature continues");
    let mut h = Harness::open_target(&fx, &Target::Branch("feature".into()), 160, 30);
    let screen = h.screen();
    assert!(screen.contains("merge commit"), "{screen}");
    assert!(screen.contains("first parent"), "{screen}");
    // The warning survives keystrokes that clear transient messages.
    h.keys("]f[f");
    assert!(h.screen().contains("merge commit"));

    let plain = main_moved_on_after_branching();
    let mut h = Harness::open_target(&plain, &Target::Branch("feature".into()), 160, 30);
    assert!(!h.screen().contains("merge commit"));
}

#[test]
fn opening_any_target_leaves_branches_refs_and_working_copy_untouched() {
    let fx = main_moved_on_after_branching();
    fx.checkout("feature");
    fx.git(&["merge", "-q", "--no-ff", "-m", "merge main", "main"]);
    fx.checkout("main");
    fx.write("on_main.txt", "dirty\n")
        .write("scratch.txt", "untracked\n");
    let before = fx.state();

    for target in [
        Target::Branch("feature".into()),
        Target::Range {
            base: "main~1".into(),
            head: "feature".into(),
        },
    ] {
        let mut h = Harness::open_target(&fx, &target, 160, 30);
        h.keys("]f[fGgg<C-d><C-u>s:q<CR>").click(5, 2);
        h.resize(100, 30);
        let _ = h.screen();
    }
    let _ = Harness::try_open_target(&fx, &Target::Branch("nope".into()), 160, 30);

    assert_eq!(fx.state(), before);
}

#[test]
fn command_line_arguments_name_a_branch_or_an_explicit_range() {
    assert_eq!(Target::from_arg(None), Target::Current);
    assert_eq!(
        Target::from_arg(Some("feature")),
        Target::Branch("feature".into())
    );
    assert_eq!(
        Target::from_arg(Some("main..feature")),
        Target::Range {
            base: "main".into(),
            head: "feature".into()
        }
    );
}

fn open_error(fx: &Fixture, target: &Target) -> String {
    match Harness::try_open_target(fx, target, 160, 30) {
        Ok(_) => panic!("expected {target:?} to fail"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn an_unknown_branch_or_revision_is_a_clear_error() {
    let fx = feature_branch();
    let e = open_error(&fx, &Target::Branch("nope".into()));
    assert!(e.contains("no such branch") && e.contains("nope"), "{e}");

    let e = open_error(
        &fx,
        &Target::Range {
            base: "nope".into(),
            head: "feature".into(),
        },
    );
    assert!(e.contains("unknown revision") && e.contains("nope"), "{e}");
}

#[test]
fn an_empty_range_is_a_clear_error() {
    let fx = feature_branch();
    // The default branch itself has nothing beyond its own merge-base.
    let e = open_error(&fx, &Target::Branch("main".into()));
    assert!(e.contains("empty range"), "{e}");

    let e = open_error(
        &fx,
        &Target::Range {
            base: "feature".into(),
            head: "feature".into(),
        },
    );
    assert!(e.contains("empty range"), "{e}");
}

#[test]
fn a_directory_that_is_not_a_repository_is_a_clear_error() {
    let dir = tempfile::tempdir().unwrap();
    let e = moonhare::session::Session::open_target(
        dir.path(),
        &Target::Branch("feature".into()),
        160,
        30,
    )
    .err()
    .expect("fails")
    .to_string();
    assert!(e.contains("not inside a git repository"), "{e}");
}

#[test]
fn base_of_a_branch_that_merged_main_in_excludes_what_came_from_main() {
    let fx = main_moved_on_after_branching();
    fx.checkout("feature");
    fx.git(&["merge", "-q", "--no-ff", "-m", "merge main", "main"]);
    fx.write("later.txt", "later\n").commit("feature continues");
    let mut h = Harness::open_target(&fx, &Target::Branch("feature".into()), 160, 30);

    let screen = h.screen();
    assert!(screen.contains("later.txt"), "{screen}");
    assert!(screen.contains("mine.txt"), "{screen}");
    assert!(!screen.contains("on_main.txt"), "{screen}");
}

#[test]
fn a_branch_that_is_not_checked_out_opens_by_name() {
    let fx = feature_branch();
    fx.checkout("main");
    let mut h = Harness::open_target(&fx, &Target::Branch("feature".into()), 160, 30);

    let screen = h.screen();
    // The tree lists the files by name under their directory.
    for file in ["README.md", "src", "a.rs", "b.rs"] {
        assert!(screen.contains(file), "sidebar lists {file}:\n{screen}");
    }
    assert!(screen.contains("main..feature"), "{screen}");
}

#[test]
fn an_explicit_range_shows_exactly_the_commits_between_base_and_head() {
    let fx = feature_branch();
    let target = Target::Range {
        base: "feature~1".into(),
        head: "feature".into(),
    };
    let mut h = Harness::open_target(&fx, &target, 160, 30);

    let screen = h.screen();
    assert!(screen.contains("src/a.rs"), "{screen}");
    assert!(
        !screen.contains("src/b.rs"),
        "b.rs is outside the range:\n{screen}"
    );
    assert!(!screen.contains("README.md"), "{screen}");
    assert!(screen.contains("feature~1..feature"), "{screen}");
}

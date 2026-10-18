//! A pull request is optional: without a remote, without GitHub and without
//! `gh` moonhare works as well, and says nothing about it unless asked.

mod support;

use std::sync::Arc;

use moonhare::git::Target;
use moonhare::github::GitHub;
use moonhare::session::Session;
use support::fake_gh::FakeGh;
use support::{feature_branch, Fixture, Harness};

const PR_JSON: &str = r#"{"number":42,"title":"Add retry","body":"Retries","baseRefName":"main"}"#;

/// The Review of the current branch of `fx`, once the Forge behind `program`
/// has answered.
fn review(fx: &Fixture, program: impl Into<std::path::PathBuf>) -> Harness {
    let forge = Arc::new(GitHub::with_program(program).in_dir(fx.path()));
    let session = Session::open_target_with_forge(fx.path(), &Target::Current, forge, 120, 30)
        .expect("session opens");
    let mut h = Harness::from_session(session);
    h.session().wait_for_forge();
    h
}

fn on_github() -> Fixture {
    let fx = feature_branch();
    fx.git(&["remote", "add", "origin", "git@github.com:owner/repo.git"]);
    fx
}

/// The status line: the row above the key hints.
fn status_line(h: &mut Harness) -> String {
    let screen = h.screen();
    let lines: Vec<&str> = screen.lines().collect();
    lines[lines.len() - 2].to_string()
}

#[test]
fn without_a_forge_to_ask_the_status_line_stays_quiet() {
    let cases: Vec<(&str, Fixture, FakeGh)> = vec![
        ("no remote", feature_branch(), FakeGh::replying(PR_JSON)),
        (
            "gh: no git remotes found",
            on_github(),
            FakeGh::with("", "no git remotes found\n", 1),
        ),
        (
            "gh: no known GitHub host",
            on_github(),
            FakeGh::with(
                "",
                "none of the git remotes configured for this repository point to a known GitHub host. \
                 To tell gh about a new GitHub host, please use `gh auth login`\n",
                1,
            ),
        ),
        (
            "gh: not logged in",
            on_github(),
            FakeGh::with(
                "",
                "You are not logged into any GitHub hosts. To log in, run: gh auth login\n",
                1,
            ),
        ),
        (
            "no pull request for the branch",
            on_github(),
            FakeGh::with("", "no pull requests found for branch \"feature\"\n", 1),
        ),
    ];
    for (case, fx, gh) in cases {
        let mut h = review(&fx, gh.program());
        let screen = h.screen();
        assert!(!screen.contains('⚠'), "{case}:\n{screen}");
        assert!(h.session().pull_request().is_none(), "{case}");
    }
}

#[test]
fn a_missing_gh_stays_quiet_too() {
    let fx = on_github();
    let mut h = review(&fx, "/nonexistent/dir/gh");
    let screen = h.screen();
    assert!(!screen.contains('⚠'), "{screen}");
}

#[test]
fn without_a_github_remote_gh_is_never_started() {
    let gh = FakeGh::replying(PR_JSON);
    let fx = feature_branch();
    fx.git(&[
        "remote",
        "add",
        "origin",
        "https://gitlab.com/owner/repo.git",
    ]);

    let mut h = review(&fx, gh.program());

    assert_eq!(gh.calls(), Vec::<String>::new());
    assert!(!h.screen().contains('⚠'));
}

#[test]
fn the_reason_there_is_no_pull_request_is_shown_on_request() {
    let gh = FakeGh::replying(PR_JSON);
    let fx = feature_branch();
    let mut h = review(&fx, gh.program());

    h.keys(":pr<CR>");

    let status = status_line(&mut h);
    assert!(status.contains("PR: —"), "{status}");
    assert!(status.contains("no git remote"), "{status}");
}

#[test]
fn the_pull_request_is_shown_on_request() {
    let gh = FakeGh::replying(PR_JSON);
    let fx = on_github();
    let mut h = review(&fx, gh.program());

    h.keys(":pr<CR>");

    let status = status_line(&mut h);
    assert!(status.contains("PR #42"), "{status}");
    assert!(status.contains("Add retry"), "{status}");
}

#[test]
fn a_network_error_with_a_github_remote_is_a_warning() {
    let gh = FakeGh::with(
        "",
        "error connecting to api.github.com\ncheck your internet connection or https://githubstatus.com\n",
        1,
    );
    let fx = on_github();
    let mut h = review(&fx, gh.program());

    let screen = h.screen();
    assert!(screen.contains("⚠ cannot reach GitHub"), "{screen}");
}

#[test]
fn an_empty_range_explains_itself_in_the_diff_and_the_file_panel() {
    let fx = feature_branch();
    fx.checkout("main");
    let mut h = Harness::open_target(&fx, &Target::Current, 120, 30);

    let frame = h.frame();
    let screen = frame.text();
    let (diff_x, _) = frame
        .find("moonhare will show new commits when they appear")
        .unwrap_or_else(|| panic!("no hint in the diff:\n{screen}"));
    let (_, files_y) = frame.find("Files (0)").expect("file panel");
    let (panel_x, panel_y) = frame
        .find("No changes yet")
        .unwrap_or_else(|| panic!("no hint in the file panel:\n{screen}"));
    assert!(screen.contains("No changes against main"), "{screen}");
    assert!(panel_x < diff_x && panel_y > files_y, "{screen}");
}

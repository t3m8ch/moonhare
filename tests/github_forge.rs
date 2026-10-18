//! Contract of the GitHub adapter, checked against a fake `gh` executable.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use moonhare::forge::{Forge, ForgeError, PullRequest};
use moonhare::github::GitHub;
use support::fake_gh::FakeGh;
use support::Fixture;

/// A repository whose `origin` is on GitHub.
fn on_github() -> Fixture {
    with_remote("https://github.com/owner/repo.git")
}

fn with_remote(url: &str) -> Fixture {
    let fx = Fixture::new();
    fx.git(&["remote", "add", "origin", url]);
    fx
}

/// The adapter for the repository at `repo`, run through `program`.
fn github(program: impl AsRef<Path>, repo: &Fixture) -> GitHub {
    GitHub::with_program(program.as_ref()).in_dir(repo.path())
}

const PR_JSON: &str =
    r#"{"number":42,"title":"Add retry","body":"Retries\nthe call","baseRefName":"main"}"#;

#[test]
fn finds_the_pull_request_of_a_branch_with_title_description_and_base() {
    let gh = FakeGh::replying(PR_JSON);
    let repo = on_github();
    let forge = github(gh.program(), &repo);

    let pr = forge.pull_request_for_branch("feature").unwrap();

    assert_eq!(
        pr,
        Some(PullRequest {
            number: 42,
            title: "Add retry".into(),
            body: "Retries\nthe call".into(),
            base_branch: "main".into(),
        })
    );
    assert_eq!(
        gh.calls(),
        vec!["pr view --json number,title,body,baseRefName -- feature"]
    );
}

#[test]
fn a_branch_name_with_a_leading_dash_is_not_taken_for_a_flag() {
    let gh = FakeGh::replying(PR_JSON);

    let repo = on_github();
    github(gh.program(), &repo)
        .pull_request_for_branch("--web")
        .unwrap();

    assert_eq!(
        gh.calls(),
        vec!["pr view --json number,title,body,baseRefName -- --web"]
    );
}

#[test]
fn a_branch_without_a_pull_request_is_not_an_error() {
    let gh = FakeGh::with("", "no pull requests found for branch \"feature\"\n", 1);
    let repo = on_github();
    let forge = github(gh.program(), &repo);

    assert_eq!(forge.pull_request_for_branch("feature"), Ok(None));
}
#[test]
fn a_missing_gh_is_reported_as_unavailable_with_a_clear_message() {
    let repo = on_github();
    let forge = github("/nonexistent/dir/gh", &repo);

    let err = forge.pull_request_for_branch("feature").unwrap_err();

    assert!(matches!(err, ForgeError::Unavailable(_)), "{err:?}");
    assert!(err.to_string().contains("gh"), "{err}");
    assert!(err.to_string().contains("not installed"), "{err}");
}
#[test]
fn an_unreachable_network_is_reported_as_such() {
    let gh = FakeGh::with(
        "",
        "error connecting to api.github.com\ncheck your internet connection or https://githubstatus.com\n",
        1,
    );
    let repo = on_github();
    let forge = github(gh.program(), &repo);

    let err = forge.pull_request_for_branch("feature").unwrap_err();

    assert!(matches!(err, ForgeError::Network(_)), "{err:?}");
    assert!(err.to_string().contains("network"), "{err}");
}
#[test]
fn looks_up_a_pull_request_by_number_and_reports_an_unknown_one() {
    let gh = FakeGh::replying(PR_JSON);
    let repo = on_github();
    let pr = github(gh.program(), &repo).pull_request(42).unwrap();
    assert_eq!(pr.title, "Add retry");
    assert_eq!(
        gh.calls(),
        vec!["pr view --json number,title,body,baseRefName -- 42"]
    );

    let gh = FakeGh::with(
        "",
        "GraphQL: Could not resolve to a PullRequest with the number of 999.\n",
        1,
    );
    let err = github(gh.program(), &repo).pull_request(999).unwrap_err();
    assert!(matches!(err, ForgeError::NotFound(_)), "{err:?}");
    assert!(err.to_string().contains("999"), "{err}");
}

#[test]
fn a_gh_that_hangs_is_killed_and_reported_as_a_network_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gh");
    let pid_file = dir.path().join("pid");
    fs::write(
        &path,
        format!(
            "#!/bin/sh\n[ \"$1\" = --probe ] && exit 0\necho $$ > '{}'\nexec sleep 100\n",
            pid_file.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    while std::process::Command::new(&path)
        .arg("--probe")
        .status()
        .is_err()
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let repo = on_github();
    let forge = github(&path, &repo).with_timeout(std::time::Duration::from_millis(500));
    let started = std::time::Instant::now();

    let err = forge.pull_request_for_branch("feature").unwrap_err();

    assert!(matches!(err, ForgeError::Network(_)), "{err:?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    let pid = fs::read_to_string(&pid_file).unwrap().trim().to_string();
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid])
        .output()
        .unwrap()
        .status
        .success();
    assert!(!alive, "gh (pid {pid}) is still running");
}

#[test]
fn gh_is_not_run_in_a_repository_without_remotes() {
    let gh = FakeGh::replying(PR_JSON);
    let repo = Fixture::new();

    let err = github(gh.program(), &repo)
        .pull_request_for_branch("feature")
        .unwrap_err();

    assert!(matches!(err, ForgeError::Unavailable(_)), "{err:?}");
    assert!(err.to_string().contains("no git remote"), "{err}");
    assert_eq!(gh.calls(), Vec::<String>::new());
}

#[test]
fn gh_is_not_run_when_no_remote_is_on_github() {
    for url in [
        "https://gitlab.com/owner/repo.git",
        "git@gitlab.com:owner/repo.git",
        "ssh://git@git.example.org:2222/owner/repo.git",
        "/srv/git/repo.git",
        "../repo",
        "file:///srv/git/repo.git",
    ] {
        let gh = FakeGh::replying(PR_JSON);
        let repo = with_remote(url);

        let err = github(gh.program(), &repo)
            .pull_request_for_branch("feature")
            .unwrap_err();

        assert!(matches!(err, ForgeError::Unavailable(_)), "{url}: {err:?}");
        assert!(err.to_string().contains("GitHub"), "{url}: {err}");
        assert_eq!(gh.calls(), Vec::<String>::new(), "{url}");
    }
}

#[test]
fn gh_is_run_for_every_spelling_of_a_github_remote() {
    for url in [
        "https://github.com/owner/repo.git",
        "https://user@github.com/owner/repo",
        "git@github.com:owner/repo.git",
        "ssh://git@github.com/owner/repo.git",
        "ssh://git@ssh.github.com:443/owner/repo.git",
    ] {
        let gh = FakeGh::replying(PR_JSON);
        let repo = with_remote(url);
        // A second remote elsewhere does not hide the GitHub one.
        repo.git(&["remote", "add", "mirror", "https://gitlab.com/o/r.git"]);

        let pr = github(gh.program(), &repo)
            .pull_request_for_branch("feature")
            .unwrap();

        assert_eq!(pr.map(|p| p.number), Some(42), "{url}");
        assert_eq!(gh.calls().len(), 1, "{url}");
    }
}

#[test]
fn gh_without_a_forge_to_ask_is_unavailable_not_failed() {
    for stderr in [
        "no git remotes found\n",
        "none of the git remotes configured for this repository point to a known GitHub host. \
         To tell gh about a new GitHub host, please use `gh auth login`\n",
        "You are not logged into any GitHub hosts. To log in, run: gh auth login\n",
        "To get started with GitHub CLI, please run:  gh auth login\n\
         Alternatively, populate the GH_TOKEN environment variable with a GitHub API authentication token.\n",
    ] {
        let gh = FakeGh::with("", stderr, 1);
        let repo = on_github();

        let err = github(gh.program(), &repo)
            .pull_request_for_branch("feature")
            .unwrap_err();

        assert!(matches!(err, ForgeError::Unavailable(_)), "{stderr}: {err:?}");
    }
}

#[test]
fn an_expired_login_is_a_failure_not_a_missing_forge() {
    let gh = FakeGh::with(
        "",
        "HTTP 401: Bad credentials (https://api.github.com/graphql)\nTry authenticating with:  gh auth login\n",
        1,
    );
    let repo = on_github();

    let err = github(gh.program(), &repo)
        .pull_request_for_branch("feature")
        .unwrap_err();

    assert!(matches!(err, ForgeError::Failed(_)), "{err:?}");
}

#[test]
fn a_pull_request_number_without_github_is_a_clear_error() {
    let gh = FakeGh::replying(PR_JSON);
    let repo = Fixture::new();

    let err = github(gh.program(), &repo).pull_request(5).unwrap_err();

    assert!(matches!(err, ForgeError::Unavailable(_)), "{err:?}");
    let text = err.to_string();
    assert!(text.contains("#5"), "{text}");
    assert!(text.contains("GitHub"), "{text}");
    assert_eq!(gh.calls(), Vec::<String>::new());
}

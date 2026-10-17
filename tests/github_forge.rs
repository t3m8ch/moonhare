//! Contract of the GitHub adapter, checked against a fake `gh` executable.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use moonhare::forge::{Forge, ForgeError, PullRequest};
use moonhare::github::GitHub;

/// A stand-in `gh`: logs its arguments, prints canned stdout and stderr and
/// exits with a canned status.
struct FakeGh {
    dir: tempfile::TempDir,
}

impl FakeGh {
    fn replying(stdout: &str) -> Self {
        Self::with(stdout, "", 0)
    }

    fn with(stdout: &str, stderr: &str, status: i32) -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("stdout"), stdout).unwrap();
        fs::write(dir.path().join("stderr"), stderr).unwrap();
        let script = format!(
            "#!/bin/sh\n[ \"$1\" = --probe ] && exit 0\nd='{}'\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> \"$d/args\"; done\n\
             printf '<<end>>\\n' >> \"$d/args\"\ncat \"$d/stdout\"\ncat \"$d/stderr\" >&2\nexit {status}\n",
            dir.path().display()
        );
        let path = dir.path().join("gh");
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        // Another test thread may fork while the script is still open for
        // writing, which makes exec fail with ETXTBSY. Run it once to be sure
        // it is settled.
        let probe = || std::process::Command::new(&path).arg("--probe").status();
        while probe().is_err() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        FakeGh { dir }
    }

    fn program(&self) -> PathBuf {
        self.dir.path().join("gh")
    }

    /// One entry per invocation, arguments joined by spaces.
    fn calls(&self) -> Vec<String> {
        let log = fs::read_to_string(self.dir.path().join("args")).unwrap_or_default();
        log.split("<<end>>\n")
            .filter(|c| !c.is_empty())
            .map(|c| c.lines().collect::<Vec<_>>().join(" "))
            .collect()
    }
}

const PR_JSON: &str =
    r#"{"number":42,"title":"Add retry","body":"Retries\nthe call","baseRefName":"main"}"#;

#[test]
fn finds_the_pull_request_of_a_branch_with_title_description_and_base() {
    let gh = FakeGh::replying(PR_JSON);
    let forge = GitHub::with_program(gh.program());

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

    GitHub::with_program(gh.program())
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
    let forge = GitHub::with_program(gh.program());

    assert_eq!(forge.pull_request_for_branch("feature"), Ok(None));
}
#[test]
fn a_missing_gh_is_reported_as_unavailable_with_a_clear_message() {
    let forge = GitHub::with_program("/nonexistent/dir/gh");

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
    let forge = GitHub::with_program(gh.program());

    let err = forge.pull_request_for_branch("feature").unwrap_err();

    assert!(matches!(err, ForgeError::Network(_)), "{err:?}");
    assert!(err.to_string().contains("network"), "{err}");
}
#[test]
fn looks_up_a_pull_request_by_number_and_reports_an_unknown_one() {
    let gh = FakeGh::replying(PR_JSON);
    let pr = GitHub::with_program(gh.program()).pull_request(42).unwrap();
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
    let err = GitHub::with_program(gh.program())
        .pull_request(999)
        .unwrap_err();
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
    let forge = GitHub::with_program(&path).with_timeout(std::time::Duration::from_millis(500));
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

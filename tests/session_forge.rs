//! The review session and the Forge, with a fake standing in for the Forge.

mod support;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use moonhare::forge::{Forge, ForgeError, PullRequest};
use moonhare::git::Target;
use moonhare::session::Session;
use support::{Fixture, Harness};

/// A Forge that answers from memory. Fetching a head points moonhare's ref at
/// a commit the test chose, the way a real fetch would.
struct FakeForge {
    pull_request: Result<Option<PullRequest>, ForgeError>,
    head: String,
}

impl FakeForge {
    fn with_pull_request(pr: PullRequest, head: &str) -> Self {
        FakeForge {
            pull_request: Ok(Some(pr)),
            head: head.to_string(),
        }
    }
}

fn pr(number: u64, title: &str, body: &str) -> PullRequest {
    PullRequest {
        number,
        title: title.into(),
        body: body.into(),
        base_branch: "main".into(),
    }
}

impl Forge for FakeForge {
    fn pull_request_for_branch(&self, _branch: &str) -> Result<Option<PullRequest>, ForgeError> {
        self.pull_request.clone()
    }

    fn pull_request(&self, _number: u64) -> Result<PullRequest, ForgeError> {
        self.pull_request
            .clone()
            .and_then(|p| p.ok_or_else(|| ForgeError::NotFound("no such pull request".into())))
    }

    fn fetch_pull_request_head(&self, repo: &Path, number: u64) -> Result<String, ForgeError> {
        let reference = format!("refs/moonhare/pr/{number}");
        let status = Command::new("git")
            .args(["update-ref", &reference, &self.head])
            .current_dir(repo)
            .status()
            .unwrap();
        assert!(status.success());
        Ok(reference)
    }
}

/// `main` is checked out and has no `a.rs`; the branch `pr-source` adds it.
fn clone_with_a_branch_not_checked_out() -> (Fixture, String) {
    let fx = Fixture::new();
    fx.write("README.md", "# demo\n").commit("initial");
    fx.branch("pr-source");
    fx.write("a.rs", "fn a() {}\n").commit("add a");
    let head = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.checkout("main");
    (fx, head)
}

#[test]
fn opening_a_pull_request_by_number_shows_it_as_a_range_without_changing_the_clone() {
    let (fx, head) = clone_with_a_branch_not_checked_out();
    let forge = FakeForge::with_pull_request(pr(7, "Add a", "Adds a."), &head);
    let branches = fx.git(&["branch", "--list"]);

    let session = Session::open_pull_request(fx.path(), &forge, 7, 160, 30).unwrap();
    let screen = Harness::from_session(session).screen();

    assert!(screen.contains("a.rs"), "{screen}");
    assert_eq!(fx.git(&["branch", "--list"]), branches);
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/main");
    assert_eq!(fx.git(&["status", "--porcelain"]), "");
}

#[test]
fn the_pull_request_of_a_branch_is_available_to_other_modules() {
    let (fx, head) = clone_with_a_branch_not_checked_out();
    let forge = FakeForge::with_pull_request(pr(7, "Add a", "Adds a."), &head);

    let session = Session::open_target_with_forge(
        fx.path(),
        &Target::Branch("pr-source".into()),
        Arc::new(forge),
        160,
        30,
    )
    .unwrap();

    let mut session = session;
    session.wait_for_forge();
    let found = session.pull_request().expect("pull request attached");
    assert_eq!(found.title, "Add a");
    assert_eq!(found.body, "Adds a.");
}
#[test]
fn an_unavailable_forge_does_not_stop_the_review_and_the_reason_is_shown_on_request() {
    let (fx, _) = clone_with_a_branch_not_checked_out();
    let forge = FakeForge {
        pull_request: Err(ForgeError::Unavailable("gh is not installed".into())),
        head: String::new(),
    };

    let mut session = Session::open_target_with_forge(
        fx.path(),
        &Target::Branch("pr-source".into()),
        Arc::new(forge),
        160,
        30,
    )
    .unwrap();
    session.wait_for_forge();
    assert!(session.pull_request().is_none());
    let mut h = Harness::from_session(session);
    let screen = h.screen();

    assert!(screen.contains("a.rs"), "{screen}");
    assert!(!screen.contains("gh is not installed"), "{screen}");
    let screen = h.keys(":pr<CR>").screen();
    assert!(screen.contains("PR: — gh is not installed"), "{screen}");
}

#[test]
fn a_branch_without_a_pull_request_opens_quietly() {
    let (fx, _) = clone_with_a_branch_not_checked_out();
    let forge = FakeForge {
        pull_request: Ok(None),
        head: String::new(),
    };

    let mut session = Session::open_target_with_forge(
        fx.path(),
        &Target::Branch("pr-source".into()),
        Arc::new(forge),
        160,
        30,
    )
    .unwrap();

    session.wait_for_forge();
    assert!(session.pull_request().is_none());
    assert!(!Harness::from_session(session).screen().contains('⚠'));
}

#[test]
fn opening_an_unknown_pull_request_fails_with_the_forges_message() {
    let (fx, head) = clone_with_a_branch_not_checked_out();
    let forge = FakeForge {
        pull_request: Ok(None),
        head,
    };

    let err = Session::open_pull_request(fx.path(), &forge, 99, 160, 30)
        .err()
        .expect("fails");

    assert!(err.to_string().contains("no such pull request"), "{err}");
}

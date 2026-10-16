//! Fetching the head of a pull request into moonhare's own ref.

mod support;

use moonhare::forge::{Forge, ForgeError};
use moonhare::github::GitHub;
use support::Fixture;

/// `server` plays GitHub: it advertises pull request 7 as `refs/pull/7/head`.
/// `clone` is the reviewer's clone with `server` as `origin`.
fn server_and_clone() -> (Fixture, Fixture, String) {
    let server = Fixture::new();
    server.write("README.md", "# demo\n").commit("initial");
    server.branch("pr-source");
    server.write("a.rs", "fn a() {}\n").commit("add a");
    let pr_head = server.git(&["rev-parse", "HEAD"]).trim().to_string();
    server.git(&["update-ref", "refs/pull/7/head", &pr_head]);
    server.checkout("main");

    let clone = Fixture::new();
    clone.git(&["remote", "add", "origin", server.path().to_str().unwrap()]);
    (server, clone, pr_head)
}

#[test]
fn fetching_a_pull_request_head_makes_a_private_ref_and_touches_nothing_else() {
    let (_server, clone, pr_head) = server_and_clone();
    clone.write("README.md", "# demo\n").commit("local work");
    let before = (
        clone.git(&["branch", "--list"]),
        clone.git(&["symbolic-ref", "HEAD"]),
        clone.git(&["status", "--porcelain"]),
    );

    let reference = GitHub::new()
        .fetch_pull_request_head(clone.path(), 7)
        .unwrap();

    assert_eq!(clone.git(&["rev-parse", &reference]).trim(), pr_head);
    assert!(reference.starts_with("refs/moonhare/"), "{reference}");
    let after = (
        clone.git(&["branch", "--list"]),
        clone.git(&["symbolic-ref", "HEAD"]),
        clone.git(&["status", "--porcelain"]),
    );
    assert_eq!(before, after);
}

#[test]
fn fetching_an_unknown_pull_request_is_reported_as_not_found() {
    let (_server, clone, _) = server_and_clone();

    let err = GitHub::new()
        .fetch_pull_request_head(clone.path(), 99)
        .unwrap_err();

    assert!(matches!(err, ForgeError::NotFound(_)), "{err:?}");
    assert!(err.to_string().contains("#99"), "{err}");
}

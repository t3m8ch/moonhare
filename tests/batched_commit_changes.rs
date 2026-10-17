//! The changes of every commit come from one git process, and are the same
//! as asking git about each commit alone.

mod support;

use moonhare::git::Repo;
use moonhare::review::{file_changes, file_changes_of_pairs};
use support::Fixture;

#[test]
fn batched_changes_equal_per_commit_changes_even_for_empty_commits_and_renames() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\ntwo\nthree\nfour\n").commit("base");
    fx.git(&["checkout", "-q", "-b", "feature"]);
    fx.write("b.txt", "new\n").commit("add b");
    fx.git(&["commit", "-q", "--allow-empty", "-m", "nothing"]);
    fx.git(&["mv", "a.txt", "c.txt"]);
    fx.git(&["commit", "-q", "-m", "rename"]);
    fx.remove("b.txt").commit("remove b");

    let repo = Repo::open(fx.path()).unwrap();
    let shas: Vec<String> = fx
        .git(&["rev-list", "--reverse", "main..feature"])
        .lines()
        .map(str::to_string)
        .collect();
    let base = fx.git(&["rev-parse", "main"]).trim().to_string();
    let mut pairs = Vec::new();
    let mut parent = base;
    for sha in &shas {
        pairs.push((parent.clone(), sha.clone()));
        parent = sha.clone();
    }
    let pairs: Vec<(&str, &str)> = pairs.iter().map(|(b, h)| (&b[..], &h[..])).collect();

    let batched = file_changes_of_pairs(&repo, &pairs).unwrap();
    let alone: Vec<_> = pairs
        .iter()
        .map(|(b, h)| file_changes(&repo, b, h).unwrap())
        .collect();
    assert_eq!(batched.len(), 4);
    assert_eq!(batched, alone);
    assert!(batched[1].is_empty());
    assert_eq!(batched[2][0].old_path.as_deref(), Some("a.txt"));
}

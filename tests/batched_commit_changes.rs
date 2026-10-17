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

/// Writes a linear history of `n` commits, each changing five files, in one
/// `git fast-import` run. Returns the shas, oldest first.
fn long_history(fx: &Fixture, n: usize) -> Vec<String> {
    let mut script = String::new();
    for i in 1..=n {
        let message = format!("commit {i}");
        script.push_str(&format!(
            "commit refs/heads/long\nmark :{i}\ncommitter T <t@e.com> {} +0000\ndata {}\n{message}\n",
            1_700_000_000 + i,
            message.len()
        ));
        if i > 1 {
            script.push_str(&format!("from :{}\n", i - 1));
        }
        for f in 0..5 {
            let body = format!("file {f} at {i}\n");
            script.push_str(&format!(
                "M 100644 inline dir/with/a/fairly/long/path/file{f}.txt\ndata {}\n{body}",
                body.len()
            ));
        }
        script.push('\n');
    }
    let mut child = std::process::Command::new("git")
        .args(["fast-import", "--quiet"])
        .current_dir(fx.path())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    fx.git(&["rev-list", "--reverse", "long"])
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn a_thousand_commits_do_not_deadlock_the_pipes_to_git() {
    let fx = Fixture::new();
    let shas = long_history(&fx, 1000);
    let path = fx.path().to_path_buf();
    let (done, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let repo = Repo::open(&path).unwrap();
        let mut pairs = Vec::new();
        for window in shas.windows(2) {
            pairs.push((window[0].as_str(), window[1].as_str()));
        }
        let changes = file_changes_of_pairs(&repo, &pairs).unwrap();
        let _ = done.send(changes);
    });
    let changes = result
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("git answered");
    assert_eq!(changes.len(), 999);
    assert!(changes.iter().all(|c| c.len() == 5));
}

#[test]
fn pairs_sharing_a_head_are_each_answered() {
    let fx = Fixture::new();
    fx.write("a.txt", "1\n").commit("one");
    fx.write("a.txt", "2\n").commit("two");
    let repo = Repo::open(fx.path()).unwrap();
    let head = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    let first = fx.git(&["rev-parse", "HEAD~1"]).trim().to_string();
    let pairs = [
        (first.as_str(), head.as_str()),
        (first.as_str(), head.as_str()),
    ];
    let changes = file_changes_of_pairs(&repo, &pairs).unwrap();
    assert_eq!(changes[0].len(), 1);
    assert_eq!(changes[0], changes[1]);
}

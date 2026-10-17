//! Ranges of dozens and hundreds of commits: the position in the title, the
//! pinned header, search, half-page steps and folded Rounds.

mod support;

use std::io::Write;
use std::process::{Command, Stdio};

use support::{Fixture, Harness};

/// `main` holds one commit; `feature` gets `count` more, each adding its own
/// file and named `step N`. Built with one `git fast-import`, so a Range of a
/// few hundred commits costs milliseconds.
fn long_branch(count: usize) -> Fixture {
    let fx = Fixture::new();
    fx.write("README.md", "# demo\n").commit("initial");
    fx.branch("feature");
    add_commits(&fx, 1, count, |n| format!("step {n}"));
    fx
}

/// Append commits `from..=to` on the checked-out `feature`, subjects by `subject`.
fn add_commits(fx: &Fixture, from: usize, to: usize, subject: impl Fn(usize) -> String) {
    let mut stream = String::new();
    for n in from..=to {
        let msg = subject(n);
        let body = format!("fn f{n}() {{}}\n");
        stream += &format!(
            "commit refs/heads/feature\ncommitter T <t@example.com> {} +0000\ndata {}\n{msg}\n",
            1_700_000_000 + n,
            msg.len() + 1
        );
        // Only the first commit names its parent; the rest follow the branch tip.
        if n == from {
            stream += "from refs/heads/feature^0\n";
        }
        stream += &format!(
            "M 100644 inline f{n}.rs\ndata {}\n{body}\n",
            body.len()
        );
    }
    let mut child = Command::new("git")
        .args(["fast-import", "--quiet", "--force"])
        .current_dir(fx.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stream.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    fx.git(&["reset", "-q", "--hard", "feature"]);
}

/// The commit list title's position text, such as `Commits 50/100`.
fn title(screen: &str) -> String {
    let row = screen
        .lines()
        .find(|l| l.contains("Commits "))
        .unwrap_or_else(|| panic!("no commit list title:\n{screen}"));
    let at = row.find("Commits ").unwrap();
    row[at..]
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn ctrl_d_and_ctrl_u_step_the_commit_list_by_its_height() {
    let fx = long_branch(100);
    let mut h = Harness::open(&fx, 160, 60);
    h.keys("<Tab>");
    assert_eq!(title(&h.screen()), "Commits 100");

    h.keys("<C-d>");
    let first = title(&h.screen());
    let step: usize = first["Commits ".len()..first.find('/').unwrap()]
        .parse()
        .unwrap();
    assert!(step >= 5, "a list-sized step, got {first}");
    h.keys("<C-d>");
    assert_eq!(title(&h.screen()), format!("Commits {}/100", step * 2));

    h.keys("<C-u><C-u>");
    assert_eq!(title(&h.screen()), "Commits 100");

    for _ in 0..40 {
        h.keys("<C-d>");
    }
    assert_eq!(title(&h.screen()), "Commits 100/100");
}

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

/// Copy a Prompt in a session of its own, so the next session finds Rounds.
fn copy_one_prompt(fx: &Fixture) {
    let mut h = Harness::open(fx, 160, 30);
    h.keys("Cifresh one<Esc><CR>Y");
    assert_eq!(h.session().take_terminal_output().len(), 1);
}

/// Sixty commits, a Prompt, sixty more: Round 1 and Round 2.
fn two_rounds() -> Fixture {
    let fx = long_branch(60);
    copy_one_prompt(&fx);
    add_commits(&fx, 61, 120, |n| format!("step {n}"));
    fx
}

fn row_index(screen: &str, needle: &str) -> Option<usize> {
    screen.lines().position(|l| l.contains(needle))
}

#[test]
fn scrolling_keeps_the_range_row_and_the_current_round_header_on_top() {
    let fx = two_rounds();
    let mut h = Harness::open(&fx, 160, 60);
    h.keys("<Tab>");

    // Not scrolled: the Range row is followed by Round 1's own header, once.
    let screen = h.screen();
    let range = row_index(&screen, "▣ Весь Range").expect("range row");
    assert!(
        screen.lines().nth(range + 1).unwrap().contains("Round 1"),
        "{screen}"
    );
    assert_eq!(screen.matches("Round 1").count(), 1, "{screen}");

    // Scrolled deep into Round 2, its header is pinned under the Range row.
    h.keys(":scope 120<CR>");
    let screen = h.screen();
    let range = row_index(&screen, "▣ Весь Range").expect("range row");
    assert!(
        screen
            .lines()
            .nth(range + 1)
            .unwrap()
            .contains("Round 2, after Prompt #1"),
        "{screen}"
    );
    assert!(!screen.contains("Round 1"), "{screen}");
    assert!(screen.contains("step 120"), "{screen}");
}

#[test]
fn clicks_under_the_pinned_header_hit_the_right_rows_and_the_header_jumps_to_its_round() {
    let fx = two_rounds();
    let mut h = Harness::open(&fx, 160, 60);
    h.keys("<Tab>:scope 120<CR>");

    let screen = h.screen();
    let row = row_index(&screen, "step 118").expect("commit row") as u16;
    h.click(8, row);
    assert_eq!(title(&h.screen()), "Commits 118/120");

    // The pinned header sits right under the Range row: a click on it goes to
    // the first commit of Round 2 and brings the Round's own header back.
    let range = row_index(&h.screen(), "▣ Весь Range").unwrap() as u16;
    h.click(8, range + 1);
    let screen = h.screen();
    assert_eq!(title(&screen), "Commits 61/120");
    assert_eq!(screen.matches("Round 2").count(), 1, "{screen}");
    assert!(screen.contains("step 61"), "{screen}");
}

#[test]
fn slash_finds_a_commit_by_its_number() {
    let fx = long_branch(100);
    let mut h = Harness::open(&fx, 160, 60);
    h.keys("<Tab>/73<CR>");
    assert_eq!(title(&h.screen()), "Commits 73/100");
    assert!(h.screen().contains("step 73"), "{}", h.screen());
    // Searching moves the cursor only: the Scope stays the whole Range.
    let screen = h.screen();
    let range = screen.lines().find(|l| l.contains("▣ Весь Range")).unwrap();
    assert!(range.contains('●'), "{screen}");
}

#[test]
fn slash_finds_a_commit_by_its_sha() {
    let fx = long_branch(100);
    let sha = fx.git(&["rev-parse", "feature~45"]);
    let mut h = Harness::open(&fx, 160, 60);
    h.keys(&format!("<Tab>/{}<CR>", &sha[..9]));
    // feature~45 is the 55th of the 100 commits.
    assert_eq!(title(&h.screen()), "Commits 55/100");
}

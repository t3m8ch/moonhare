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

#[test]
fn slash_finds_text_and_n_with_capital_n_walk_the_matches_around_the_list() {
    let fx = Fixture::new();
    fx.write("README.md", "# demo\n").commit("initial");
    fx.branch("feature");
    add_commits(&fx, 1, 100, |n| match n {
        37 | 80 => format!("Frobnicate the widget {n}"),
        _ => format!("step {n}"),
    });
    let mut h = Harness::open(&fx, 160, 60);
    h.keys("<Tab>/frobnicate<CR>");
    assert_eq!(title(&h.screen()), "Commits 37/100");
    h.keys("n");
    assert_eq!(title(&h.screen()), "Commits 80/100");
    h.keys("n");
    assert_eq!(title(&h.screen()), "Commits 37/100", "wraps to the first");
    h.keys("N");
    assert_eq!(title(&h.screen()), "Commits 80/100", "wraps to the last");
    h.keys("N");
    assert_eq!(title(&h.screen()), "Commits 37/100");

    // An empty pattern repeats the last one; a miss says so and stays put.
    h.keys("/<CR>");
    assert_eq!(title(&h.screen()), "Commits 80/100");
    h.keys("/zzz<CR>");
    assert_eq!(title(&h.screen()), "Commits 80/100");
    assert!(h.screen().contains("No match: zzz"), "{}", h.screen());
    // Esc leaves the search line without moving.
    h.keys("/step<Esc>");
    assert_eq!(title(&h.screen()), "Commits 80/100");
}

#[test]
fn n_and_slash_in_the_diff_do_not_touch_the_commit_list() {
    let fx = long_branch(100);
    let mut h = Harness::open(&fx, 160, 60);
    h.keys("/step<CR>n");
    assert_eq!(title(&h.screen()), "Commits 100");
}

#[test]
fn za_folds_a_round_into_one_row_and_the_scope_stays_as_it_was() {
    let fx = two_rounds();
    let mut h = Harness::open(&fx, 160, 60);
    h.keys("<Tab>:scope 5<CR>");
    assert!(h.screen().contains("step 12"), "{}", h.screen());

    h.keys("zc");
    let screen = h.screen();
    assert!(screen.contains("Round 1, 60 commits"), "{screen}");
    assert!(!screen.contains("step 12"), "{screen}");
    assert!(screen.contains("Round 2, after Prompt #1"), "{screen}");
    assert!(screen.contains("step 61"), "{screen}");
    assert!(screen.contains("scope:5"), "{screen}");
    // The folded row carries the Scope mark of the commit hidden in it.
    let folded = screen.lines().find(|l| l.contains("Round 1, 60")).unwrap();
    assert!(folded.contains('●'), "{screen}");

    h.keys("zo");
    let screen = h.screen();
    assert!(screen.contains("step 12"), "{screen}");
    assert!(!screen.contains("Round 1, 60 commits"), "{screen}");

    // `za` toggles.
    h.keys("za");
    assert!(h.screen().contains("Round 1, 60 commits"), "{}", h.screen());
    h.keys("za");
    assert!(h.screen().contains("step 12"), "{}", h.screen());
}

#[test]
fn motions_treat_a_folded_round_as_one_stop_and_enter_still_applies_a_commit() {
    let fx = two_rounds();
    let mut h = Harness::open(&fx, 160, 60);
    h.keys("<Tab>:scope 7<CR>zc");
    assert_eq!(title(&h.screen()), "Commits 1/120", "cursor on the folded row");

    h.keys("j");
    assert_eq!(title(&h.screen()), "Commits 61/120", "j skips the hidden commits");
    h.keys("k");
    assert_eq!(title(&h.screen()), "Commits 1/120");
    h.keys("k");
    assert_eq!(title(&h.screen()), "Commits 120", "k reaches the Range row");

    // A selection across the folded Round covers its hidden commits too.
    h.keys("jvj<CR>");
    assert!(h.screen().contains("scope:1-61"), "{}", h.screen());
}

#[test]
fn clicking_a_folded_round_opens_it() {
    let fx = two_rounds();
    let mut h = Harness::open(&fx, 160, 60);
    h.keys("<Tab>:scope 7<CR>zc");
    let row = row_index(&h.screen(), "Round 1, 60 commits").unwrap() as u16;
    h.click(8, row);
    let screen = h.screen();
    assert!(screen.contains("step 12"), "{screen}");
    assert!(!screen.contains("Round 1, 60 commits"), "{screen}");
}

#[test]
fn jumping_to_a_commit_inside_a_folded_round_opens_the_round() {
    let fx = two_rounds();
    let mut h = Harness::open(&fx, 160, 60);

    // `:scope N`
    h.keys("<Tab>:scope 3<CR>zc:scope 4<CR>");
    assert_eq!(title(&h.screen()), "Commits 4/120");
    assert!(h.screen().contains("step 4"), "{}", h.screen());

    // `/` search
    h.keys("zc/step 9<CR>");
    assert_eq!(title(&h.screen()), "Commits 9/120");
    assert!(h.screen().contains("step 9"), "{}", h.screen());

    // `]c` from the whole Range lands on the first commit
    h.keys("zca]c");
    assert_eq!(title(&h.screen()), "Commits 1/120");
    assert!(h.screen().contains("step 1 "), "{}", h.screen());
}

#[test]
fn hundreds_of_commits_survive_every_key_click_and_size_without_panics_or_lag() {
    let fx = long_branch(150);
    copy_one_prompt(&fx);
    add_commits(&fx, 151, 300, |n| format!("step {n}"));
    let started = std::time::Instant::now();
    for (w, hgt) in [(160, 60), (80, 16), (160, 12)] {
        let mut h = Harness::open(&fx, w, hgt);
        h.keys("<Tab>");
        for keys in [
            "<C-d><C-d><C-u>zc<C-d>za<C-u><C-u><C-u>jjjj",
            "/step 2<CR>nnnNNN/<CR>zo",
            ":scope 299-300<CR><C-d><C-d>zcN",
            "<Space>c<C-d><C-d><C-u>/step<CR>n<Esc>",
            "G]c]c[c[c",
        ] {
            h.keys(keys);
            h.screen();
            for row in 0..hgt {
                h.click(3, row);
                h.screen();
            }
        }
        h.resize(w, hgt / 2 + 3);
        h.screen();
    }
    assert!(
        started.elapsed() < std::time::Duration::from_secs(60),
        "frames for 300 commits are slow: {:?}",
        started.elapsed()
    );
}

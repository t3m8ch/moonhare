//! New commits and rewritten history: the tool notices them without
//! rebuilding the screen, applies them on `:reload` / `R`, and keeps the
//! Reviewed marks whose file content did not change.

mod support;

use support::{feature_branch, Harness};

/// Long enough for the tool to look at the branch again.
const LATER: u64 = 5_000;

/// The screen without its status line, which may legitimately change.
fn without_status(h: &mut Harness) -> String {
    h.screen()
        .lines()
        .filter(|l| !l.contains("NORMAL"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_new_commit_is_announced_in_the_status_and_the_screen_stays_as_it_was() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]f");
    let before = without_status(&mut h);
    assert!(!h.screen().contains("new commit"), "{}", h.screen());

    fx.write("src/c.rs", "fn c() {}\n").commit("add c");
    h.wait(LATER);

    assert!(h.screen().contains("+1 new commit"), "{}", h.screen());
    assert_eq!(without_status(&mut h), before);
}

/// The sidebar (first 32 columns).
fn sidebar(h: &mut Harness) -> String {
    h.screen()
        .lines()
        .map(|l| l.chars().take(32).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn reload_shows_the_new_commits_and_their_files() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    fx.write("src/c.rs", "fn c() {}\n").commit("add c");
    h.wait(LATER);
    assert!(!sidebar(&mut h).contains("add c"));

    h.keys(":reload<CR>");

    let side = sidebar(&mut h);
    assert!(side.contains("add c"), "{side}");
    assert!(
        side.contains("src/c.rs") && side.contains("Files (4)"),
        "{side}"
    );
    assert!(side.contains("Commits 4"), "{side}");
    assert!(!h.screen().contains("new commit"), "{}", h.screen());
}

#[test]
fn capital_r_reloads_too_and_keeps_the_selected_file() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]f]f");
    fx.write("src/c.rs", "fn c() {}\n").commit("add c");
    h.wait(LATER);

    h.keys("R");

    let side = sidebar(&mut h);
    assert!(side.contains("Files (4)") && side.contains("add c"), "{side}");
    let cursor = side.lines().find(|l| l.starts_with("│>")).unwrap();
    assert!(cursor.contains("src/b.rs"), "{side}");
}

#[test]
fn reload_without_news_says_so() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(":reload<CR>");
    assert!(h.screen().contains("Nothing to reload"), "{}", h.screen());
}

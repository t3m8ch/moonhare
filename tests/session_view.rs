//! Layout, highlighting, scrolling and command behaviour of the session.

mod support;

use support::{feature_branch, Fixture, Harness};

/// `main` has one file; the branch changes one line of it.
fn one_line_changed() -> Fixture {
    let fx = Fixture::new();
    fx.write(
        "lib.rs",
        "fn keep() {}\n\nfn value() -> i32 {\n    old_value\n}\n",
    )
    .commit("initial");
    fx.branch("feature");
    fx.write(
        "lib.rs",
        "fn keep() {}\n\nfn value() -> i32 {\n    new_value\n}\n",
    )
    .commit("change");
    fx
}

fn row_with<'a>(screen: &'a str, needle: &str) -> Option<&'a str> {
    screen.lines().find(|l| l.contains(needle))
}

fn is_side_by_side(screen: &str) -> bool {
    row_with(screen, "old_value").is_some_and(|r| r.contains("new_value"))
}

#[test]
fn wide_terminal_shows_side_by_side_and_narrow_shows_inline() {
    let fx = one_line_changed();

    let mut wide = Harness::open(&fx, 160, 30);
    let screen = wide.screen();
    assert!(
        is_side_by_side(&screen),
        "old and new share a row:\n{screen}"
    );

    let mut narrow = Harness::open(&fx, 100, 30);
    let screen = narrow.screen();
    let old = row_with(&screen, "old_value").expect("old line");
    assert!(!old.contains("new_value"), "inline stacks them:\n{screen}");
    assert!(old.contains('-'), "removed line is marked:\n{screen}");
    let new = row_with(&screen, "new_value").expect("new line");
    assert!(new.contains('+'), "added line is marked:\n{screen}");
}

#[test]
fn s_switches_layout_and_the_manual_choice_beats_the_width_heuristic() {
    let fx = one_line_changed();
    let mut h = Harness::open(&fx, 160, 30);
    assert!(is_side_by_side(&h.screen()));

    h.keys("s");
    assert!(!is_side_by_side(&h.screen()), "s switched to inline");

    // Still inline at a width where auto would pick side-by-side.
    h.resize(200, 30);
    assert!(
        !is_side_by_side(&h.screen()),
        "manual choice survives resize"
    );
    h.resize(100, 30);
    h.keys("s");
    assert!(
        is_side_by_side(&h.screen()),
        "s switched back, even when narrow"
    );
    h.resize(160, 30).resize(100, 30);
    assert!(
        is_side_by_side(&h.screen()),
        "and that choice sticks as well"
    );
}

#[test]
fn highlighting_covers_the_whole_file_not_only_changed_lines() {
    let fx = one_line_changed();
    let mut h = Harness::open(&fx, 160, 30);
    let frame = h.frame();

    // `fn` on the first line is unchanged, yet coloured as a keyword, unlike
    // the identifier after it.
    let (kx, ky) = frame.find("fn keep").expect("first line shown");
    let (ix, iy) = (kx + 3, ky);
    assert_ne!(frame.fg_at(kx, ky), frame.fg_at(ix, iy));
}

fn long_file() -> Fixture {
    let fx = Fixture::new();
    fx.write("README.md", "x\n").commit("initial");
    fx.branch("feature");
    let body: String = (1..=100).map(|i| format!("line number {i:03}\n")).collect();
    fx.write("long.txt", &body).commit("add long file");
    fx
}

#[test]
fn vim_motions_scroll_the_diff() {
    let fx = long_file();
    let mut h = Harness::open(&fx, 160, 20);
    let visible = |h: &mut Harness, n: u32| h.screen().contains(&format!("line number {n:03}"));
    assert!(visible(&mut h, 1));
    assert!(!visible(&mut h, 60));

    h.keys("G");
    assert!(visible(&mut h, 100), "G shows the end");
    assert!(!visible(&mut h, 1));

    h.keys("gg");
    assert!(visible(&mut h, 1), "gg returns to the top");

    h.keys("j");
    assert!(!visible(&mut h, 1), "j scrolls one line down");
    assert!(visible(&mut h, 2));
    h.keys("k");
    assert!(visible(&mut h, 1), "k scrolls back");

    h.keys("<C-d>");
    assert!(!visible(&mut h, 1), "Ctrl-d scrolls half a page");
    assert!(visible(&mut h, 10), "half a page: line 10 is now first");
    assert!(
        visible(&mut h, 18),
        "so the end of the old page is still in view"
    );
    h.keys("<C-u>");
    assert!(visible(&mut h, 1), "Ctrl-u scrolls back");
}

#[test]
fn j_stops_at_the_end_of_the_file() {
    let fx = long_file();
    let mut h = Harness::open(&fx, 160, 20);
    h.keys("G");
    let at_end = h.screen();
    h.keys("jjjj");
    assert_eq!(h.screen(), at_end);
}

#[test]
fn mouse_wheel_scrolls_the_diff() {
    let fx = long_file();
    let mut h = Harness::open(&fx, 160, 20);
    h.wheel_down(100, 5);
    let screen = h.screen();
    assert!(!screen.contains("line number 001"), "{screen}");
    assert!(screen.contains("line number 004"), "{screen}");
}

#[test]
fn colon_q_enter_quits() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(":q");
    assert!(!h.quit_requested(), "not before Enter");
    assert!(h.screen().contains(":q"), "the command line is visible");
    h.keys("<CR>");
    assert!(h.quit_requested());
}

#[test]
fn escape_cancels_a_command() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(":q<Esc><CR>");
    assert!(!h.quit_requested());
}

#[test]
fn unknown_command_reports_instead_of_quitting() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(":nonsense<CR>");
    assert!(!h.quit_requested());
    assert!(h.screen().contains("Not an editor command: nonsense"));
}

#[test]
fn tiny_terminal_asks_for_more_room() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.resize(40, 10);
    assert!(h.screen().contains("Terminal too small"));
    h.resize(160, 30);
    assert!(h.screen().contains("src/a.rs"));
}

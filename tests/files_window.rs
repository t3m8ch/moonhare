//! The file list as a window of its own: it takes part in the `Tab` cycle.
mod support;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use support::*;

fn file_status(h: &mut Harness) -> String {
    h.screen()
        .lines()
        .find(|l| l.contains("file "))
        .unwrap_or("")
        .to_string()
}

/// Colour of the top-left corner of the box whose title starts with `title`.
fn border(h: &mut Harness, title: &str) -> Color {
    let frame = h.frame();
    let (x, y) = frame
        .find(title)
        .unwrap_or_else(|| panic!("no {title}:\n{}", frame.text()));
    frame.fg_at(x - 1, y)
}

#[test]
fn tab_cycles_commits_files_diff_and_shift_tab_goes_back() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 30);
    let dim = border(&mut h, "Files");
    assert_eq!(border(&mut h, "Commits"), dim, "diff has the focus");
    h.keys("<Tab>");
    assert_ne!(border(&mut h, "Commits"), dim, "commits focused");
    assert_eq!(border(&mut h, "Files"), dim);
    h.keys("<Tab>");
    assert_eq!(border(&mut h, "Commits"), dim);
    assert_ne!(border(&mut h, "Files"), dim, "files focused");
    h.keys("<Tab>");
    assert_eq!(border(&mut h, "Files"), dim, "back in the diff");
    h.keys("<S-Tab>");
    assert_ne!(border(&mut h, "Files"), dim, "files again");
    h.keys("<S-Tab>");
    assert_ne!(border(&mut h, "Commits"), dim);
    h.keys("<S-Tab>");
    assert_eq!(border(&mut h, "Commits"), dim);
}

#[test]
fn j_k_and_enter_pick_a_file_in_the_files_window() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 30);
    h.keys("<Tab><Tab>");
    assert!(file_status(&mut h).contains("file 1/3"));
    // README.md, then the directory src: the open file stays.
    h.keys("j");
    assert!(file_status(&mut h).contains("file 1/3"), "{}", h.screen());
    h.keys("j");
    assert!(file_status(&mut h).contains("file 2/3"), "{}", h.screen());
    h.keys("jj");
    assert!(file_status(&mut h).contains("file 3/3"), "clamped");
    h.keys("k");
    assert!(file_status(&mut h).contains("file 2/3"));
    // Enter hands the focus to the diff: j now moves the diff cursor.
    h.keys("<CR>j");
    assert!(file_status(&mut h).contains("file 2/3"));
}

#[test]
fn the_hints_and_help_describe_the_files_window() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 60);
    h.keys("<Tab><Tab>");
    let screen = h.screen();
    let hints = screen.lines().last().unwrap().to_string();
    assert!(
        hints.contains("j/k: файлы") && hints.contains("Enter: открыть"),
        "{hints}"
    );
    h.keys("?");
    let screen = h.screen();
    assert!(
        screen.contains("S-Tab") && screen.contains("]g"),
        "{screen}"
    );
}

fn at_file(h: &mut Harness, n: usize) -> bool {
    file_status(h).contains(&format!("file {n}/3"))
}

#[test]
fn arrows_move_the_file_selection_like_jk() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 30);
    h.keys("<Tab><Tab>");
    // Over the directory src to src/a.rs.
    h.session().handle(key(KeyCode::Down));
    h.session().handle(key(KeyCode::Down));
    assert!(at_file(&mut h, 2), "{}", h.screen());
    h.session().handle(key(KeyCode::Down));
    assert!(at_file(&mut h, 3));
    h.session().handle(key(KeyCode::Up));
    assert!(at_file(&mut h, 2));
}

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn gg_big_g_and_ctrl_d_u_jump_in_the_files_window() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 30);
    h.keys("<C-w>j");
    assert!(at_file(&mut h, 1), "Ctrl-w j from the diff stays");
    h.keys("<C-w>h<C-w>j");
    h.keys("G");
    assert!(at_file(&mut h, 3), "{}", h.screen());
    h.keys("gg");
    assert!(at_file(&mut h, 1));
    h.keys("2G");
    assert!(at_file(&mut h, 2));
    h.keys("<C-d>");
    assert!(at_file(&mut h, 3));
    h.keys("<C-u>");
    assert!(at_file(&mut h, 1));
}

#[test]
fn groups_are_walked_with_jk_and_enter_opens_the_file() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 30);
    h.keys("<C-w>h<C-w>j");
    // README.md, the directory src, src/a.rs, src/b.rs.
    h.keys("jjj<CR>");
    assert!(at_file(&mut h, 3));
    // The focus is in the diff now: Tab goes to the commits.
    h.keys("<Tab>");
    assert_ne!(border(&mut h, "Commits"), border(&mut h, "Files"));
}

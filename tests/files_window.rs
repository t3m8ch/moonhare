//! The file list as a window of its own: it takes part in the `Tab` cycle.
mod support;
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

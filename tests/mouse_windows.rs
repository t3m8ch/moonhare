//! The mouse in the windows around the diff: the commit and file lists, the
//! pickers and the overlays take clicks and the wheel.

mod support;

use support::{feature_branch, Harness};

/// The screen row whose text contains `needle`.
fn row_of(h: &mut Harness, needle: &str) -> (u16, u16) {
    h.frame()
        .find(needle)
        .unwrap_or_else(|| panic!("no {needle:?} on\n{}", h.screen()))
}

/// Like [`row_of`], but the last place on the screen: what an overlay draws
/// over the windows, not the windows themselves.
fn last_row_of(h: &mut Harness, needle: &str) -> (u16, u16) {
    let screen = h.screen();
    let lines: Vec<_> = screen.lines().collect();
    let (row, line) = lines
        .iter()
        .enumerate()
        .rfind(|(_, l)| l.contains(needle))
        .unwrap_or_else(|| panic!("no {needle:?} on\n{screen}"));
    let col = line[..line.rfind(needle).unwrap()].chars().count();
    (col as u16, row as u16)
}

#[test]
fn the_wheel_over_the_commit_list_moves_its_cursor() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    let (x, y) = row_of(&mut h, "add a");
    h.wheel_down(x, y);
    h.wheel_down(x, y);
    let screen = h.screen();
    let line = screen.lines().find(|l| l.contains("add b")).unwrap();
    assert!(line.starts_with("│>"), "{screen}");
    h.wheel_up(x, y);
    let screen = h.screen();
    let line = screen.lines().find(|l| l.contains("add a")).unwrap();
    assert!(line.starts_with("│>"), "{screen}");
}

#[test]
fn the_wheel_over_the_file_list_moves_between_files() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    let (x, y) = row_of(&mut h, "src/a.rs");
    h.wheel_down(x, y);
    assert!(h.screen().contains("file 2/3"), "{}", h.screen());
    h.wheel_down(x, y);
    h.wheel_down(x, y);
    assert!(h.screen().contains("file 3/3"), "{}", h.screen());
    h.wheel_up(x, y);
    assert!(h.screen().contains("file 2/3"), "{}", h.screen());
}

#[test]
fn the_commit_picker_takes_clicks_and_the_wheel() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("<Space>c");
    let (x, y) = last_row_of(&mut h, "add b");
    h.click(x, y);
    h.keys("<CR>");
    assert!(h.screen().contains("scope:2"), "{}", h.screen());

    // The cursor starts on the applied commit 2: the wheel moves it.
    h.keys("<Space>c");
    let (x, y) = last_row_of(&mut h, "change a");
    h.wheel_up(x, y);
    h.keys("<CR>");
    assert!(h.screen().contains("scope:1"), "{}", h.screen());
    h.keys("<Space>c");
    h.wheel_down(x, y);
    h.wheel_down(x, y);
    h.keys("<CR>");
    assert!(h.screen().contains("scope:3"), "{}", h.screen());
}

//! The width of the sidebar: `Ctrl-w <` / `>` / `=`, `:sidebar N` and
//! dragging the border with the mouse.

mod support;

use support::{feature_branch, Harness};

/// The sidebar's width in the frame: where its top-right corner is.
fn width(h: &mut Harness) -> u16 {
    let screen = h.screen();
    let row = screen.lines().next().unwrap();
    row.chars().position(|c| c == '┐').expect("a sidebar") as u16 + 1
}

#[test]
fn ctrl_w_angle_brackets_resize_the_sidebar_and_equals_resets_it() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    let default = width(&mut h);
    h.keys("<C-w>>");
    let wider = width(&mut h);
    assert!(wider > default, "{wider} > {default}");
    h.keys("<C-w>>");
    assert!(width(&mut h) > wider);
    h.keys("<C-w><lt><C-w><lt><C-w><lt>");
    assert!(width(&mut h) < default);
    h.keys("<C-w>=");
    assert_eq!(width(&mut h), default);
}

#[test]
fn the_width_stays_between_the_minimum_and_the_maximum() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    for _ in 0..40 {
        h.keys("<C-w><lt>");
    }
    let min = width(&mut h);
    assert!(min >= 16, "{min}");
    h.keys("<C-w><lt>");
    assert_eq!(width(&mut h), min);
    for _ in 0..100 {
        h.keys("<C-w>>");
    }
    let max = width(&mut h);
    assert!(max <= 160 - 40, "the diff keeps room: {max}");
    h.keys("<C-w>>");
    assert_eq!(width(&mut h), max);
}

#[test]
fn the_sidebar_command_sets_the_width_and_resets_it() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    let default = width(&mut h);
    h.keys(":sidebar 50<CR>");
    assert_eq!(width(&mut h), 50);
    h.keys(":sidebar 1000<CR>");
    assert_eq!(width(&mut h), 160 - 40);
    h.keys(":sidebar 2<CR>");
    assert!(width(&mut h) >= 16);
    h.keys(":sidebar 50<CR>:sidebar<CR>");
    assert_eq!(width(&mut h), default);
    h.keys(":sidebar wide<CR>");
    assert!(h.screen().contains("Usage: :sidebar"), "{}", h.screen());
}

#[test]
fn the_which_key_help_and_hint_line_name_the_keys() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("<C-w>").wait(350);
    let screen = h.screen();
    assert!(screen.contains("ширин"), "{screen}");
    assert!(screen.contains("сайдбар шире"), "{screen}");
    h.keys("<Esc>?").keys(&"j".repeat(80));
    assert!(h.screen().contains("Ctrl-w ="), "{}", h.screen());
}

#[test]
fn dragging_the_border_changes_the_width_after_the_cursor() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    let w = width(&mut h);
    h.drag((w - 1, 5), (49, 5));
    assert_eq!(width(&mut h), 50);
    h.drag((49, 7), (24, 3));
    assert_eq!(width(&mut h), 25);
    h.drag((24, 7), (2, 7));
    assert!(width(&mut h) >= 16);
}

#[test]
fn a_drag_of_the_border_selects_and_copies_nothing() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    let w = width(&mut h);
    h.drag((w - 1, 5), (100, 8));
    assert!(h.copied().is_empty());
    assert_eq!(width(&mut h), 101);
}

#[test]
fn a_drag_in_the_diff_does_not_resize() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    let default = width(&mut h);
    h.drag((default + 6, 4), (default + 30, 4));
    assert_eq!(width(&mut h), default);
}

#[test]
fn a_manual_width_survives_a_resize_and_narrow_screens_still_hide_the_sidebar() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(":sidebar 70<CR>");
    h.resize(120, 30);
    assert_eq!(width(&mut h), 70);
    // Too narrow for the chosen width: the diff keeps its room.
    h.resize(100, 30);
    assert_eq!(width(&mut h), 60);
    // Below the automatic threshold the sidebar hides as before.
    h.resize(90, 30);
    assert!(!h.screen().contains("Files ("), "{}", h.screen());
    h.resize(160, 30);
    assert_eq!(width(&mut h), 70);
}

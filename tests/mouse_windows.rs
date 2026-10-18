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

#[test]
fn the_file_picker_takes_clicks_and_the_wheel() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("<Space>f");
    let (x, y) = last_row_of(&mut h, "b.rs");
    h.click(x, y);
    assert!(h.screen().contains("file 3/3"), "{}", h.screen());
    assert!(!h.screen().contains("Pick a file"), "{}", h.screen());

    h.keys("<Space>f");
    let (x, y) = last_row_of(&mut h, "a.rs");
    h.wheel_up(x, y);
    h.keys("<CR>");
    assert!(h.screen().contains("file 2/3"), "{}", h.screen());
    // From a.rs up onto the directory src, then down past a.rs to b.rs.
    h.keys("<Space>f");
    h.wheel_up(x, y);
    h.wheel_down(x, y);
    h.wheel_down(x, y);
    h.keys("<CR>");
    assert!(h.screen().contains("file 3/3"), "{}", h.screen());
}

#[test]
fn dragging_over_an_overlay_copies_its_text() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("?");
    let (x, y) = row_of(&mut h, "hjkl везде");
    h.drag((x, y), (x + 80, y + 1));
    assert_eq!(
        h.copied(),
        vec!["hjkl везде обычные движения и окон не переключают;\nокна переключаются через Ctrl-w и Tab.".to_string()]
    );
    // A double click picks a word there too.
    h.wait(1000);
    h.click(x + 1, y).click(x + 1, y);
    assert_eq!(h.copied(), vec!["hjkl".to_string()]);
}

#[test]
fn the_selection_in_an_overlay_goes_with_the_overlay() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("?");
    let (x, y) = row_of(&mut h, "hjkl везде");
    let plain = h.frame().bg_at(x + 1, y);
    h.drag((x, y), (x + 20, y));
    assert_ne!(h.frame().bg_at(x + 1, y), plain);
    h.keys("<Esc>?");
    assert_eq!(h.frame().bg_at(x + 1, y), plain);
}

#[test]
fn the_wheel_scrolls_the_help() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("?");
    assert!(h.screen().contains("hjkl везде"), "{}", h.screen());
    h.wheel_down(50, 10);
    h.wheel_down(50, 10);
    assert!(!h.screen().contains("hjkl везде"), "{}", h.screen());
    h.wheel_up(50, 10);
    h.wheel_up(50, 10);
    assert!(h.screen().contains("hjkl везде"), "{}", h.screen());
}

#[test]
fn in_an_overlay_a_one_character_word_is_copied_and_a_shaky_click_is_not() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("?");
    let (x, y) = row_of(&mut h, "через Ctrl-w");
    let dash = x + "через Ctrl".chars().count() as u16;
    h.drag((dash, y), (dash, y));
    assert!(h.copied().is_empty());
    h.wait(1000);
    h.click(dash, y).click(dash, y);
    assert_eq!(h.copied(), vec!["-".to_string()]);
}

#[test]
fn the_mouse_never_panics_on_a_terminal_too_small_to_draw() {
    let fx = feature_branch();
    for (w, h_) in [(4, 10), (3, 3), (1, 1), (4, 40)] {
        let mut h = Harness::open(&fx, w, h_);
        h.keys("<Space>f");
        h.click(1, 1)
            .wheel_down(1, 1)
            .wheel_up(0, 0)
            .drag((0, 0), (3, 2));
        h.keys("<Esc><Space>c");
        h.click(1, 1).wheel_down(1, 1).drag((0, 0), (3, 2));
        h.keys("<Esc>?");
        h.click(1, 1).wheel_down(1, 1).drag((0, 0), (3, 2));
    }
}

#[test]
fn wide_characters_in_an_overlay_count_two_cells() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("jjci日本語 ok<Esc><CR>");
    h.keys("<Space>ps");
    let (x, y) = row_of(&mut h, "日");
    h.click(x + 3, y).click(x + 3, y);
    assert_eq!(h.copied(), vec!["日本語".to_string()]);
    h.click(x + 3, y);
    assert_eq!(h.copied(), vec!["日本語 ok".to_string()]);
}

#[test]
fn the_wheel_scrolls_the_file_list_without_changing_the_file() {
    let fx = support::Fixture::new();
    fx.write("README.md", "# demo\n").commit("initial");
    fx.branch("feature");
    for n in 1..=30 {
        fx.write(&format!("f{n:02}.txt"), "x\n");
    }
    fx.commit("many files");
    let mut h = Harness::open(&fx, 160, 24);
    assert!(!h.screen().contains("f30.txt"), "{}", h.screen());
    let (x, y) = row_of(&mut h, "f02.txt");
    for _ in 0..40 {
        h.wheel_down(x, y);
    }
    let screen = h.screen();
    assert!(screen.contains("f30.txt"), "{screen}");
    assert!(screen.contains("file 1/30"), "{screen}");
    // A click lands on the row it is on.
    let (x, y) = row_of(&mut h, "f30.txt");
    h.click(x, y);
    assert!(h.screen().contains("file 30/30"), "{}", h.screen());
    for _ in 0..40 {
        h.wheel_up(x, y);
    }
    assert!(h.screen().contains("f01.txt"), "{}", h.screen());
    assert!(h.screen().contains("file 30/30"), "{}", h.screen());
    // Moving the selection brings the list back to it.
    h.keys("[f");
    assert!(h.screen().contains("file 29/30"), "{}", h.screen());
    assert!(h.screen().contains("f29.txt"), "{}", h.screen());
}

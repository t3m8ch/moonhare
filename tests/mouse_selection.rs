//! The mouse in the diff: clicks, selecting code with a drag and copying it
//! through OSC 52 when the button is released.

mod support;

use support::{Fixture, Harness};

/// One file whose second line is replaced and a line is added after it.
fn changed_file() -> Fixture {
    let fx = Fixture::new();
    fx.write(
        "src/lib.rs",
        "fn alpha() {\n    old_call(1);\n    keep_me();\n}\n",
    )
    .commit("initial");
    fx.branch("feature");
    fx.write(
        "src/lib.rs",
        "fn alpha() {\n    new_call(2);\n    extra();\n    keep_me();\n}\n",
    )
    .commit("change");
    fx
}

#[test]
fn a_click_in_the_diff_puts_the_cursor_on_the_line() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 100, 24);
    let (x, y) = h.frame().find("keep_me").unwrap();
    h.click(x, y);
    h.keys("c");
    assert!(h.screen().contains("new 4"), "{}", h.screen());
}

#[test]
fn dragging_over_unambiguous_code_copies_it_on_release_without_the_gutter() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 100, 24);
    let (ex, ey) = h.frame().find("extra").unwrap();
    let (kx, ky) = h.frame().find("keep_me").unwrap();
    h.drag((ex, ey), (kx + 9, ky));
    assert_eq!(h.copied(), vec!["extra();\n    keep_me();".to_string()]);
}

#[test]
fn a_mix_of_removed_and_added_lines_is_copied_as_a_diff_with_prefixes() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 100, 24);
    let (ox, oy) = h.frame().find("old_call").unwrap();
    let (ex, ey) = h.frame().find("extra").unwrap();
    h.drag((ox, oy), (ex + 7, ey));
    assert_eq!(
        h.copied(),
        vec!["-old_call(1);\n+    new_call(2);\n+    extra();".to_string()]
    );
}

#[test]
fn side_by_side_selection_stays_in_the_half_where_it_started() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 160, 24);
    let (ox, oy) = h.frame().find("old_call").unwrap();
    let (_, ky) = h.frame().find("keep_me").unwrap();
    // The drag ends in the right half, which must stay out of the copy.
    h.drag((ox, oy), (150, ky));
    assert_eq!(h.copied(), vec!["old_call(1);\n    keep_me();".to_string()]);
}

#[test]
fn a_double_click_selects_the_word_and_a_triple_click_the_line() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 100, 24);
    let (x, y) = h.frame().find("new_call").unwrap();
    h.click(x + 2, y).click(x + 2, y);
    assert_eq!(h.copied(), vec!["new_call".to_string()]);
    h.click(x + 2, y);
    assert_eq!(h.copied(), vec!["    new_call(2);".to_string()]);
}

#[test]
fn clicks_far_apart_in_time_are_not_a_double_click() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 100, 24);
    let (x, y) = h.frame().find("new_call").unwrap();
    h.click(x + 2, y).wait(1000).click(x + 2, y);
    assert_eq!(h.copied(), Vec::<String>::new());
}

#[test]
fn the_selection_is_highlighted_until_escape_or_a_click_elsewhere() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 100, 24);
    let (ex, ey) = h.frame().find("extra").unwrap();
    let (kx, ky) = h.frame().find("keep_me").unwrap();
    let plain = h.frame().bg_at(ex + 2, ey);
    h.drag((ex, ey), (kx + 9, ky));
    let marked = h.frame().bg_at(ex + 2, ey);
    assert_ne!(marked, plain);
    // The gutter is not part of the selection.
    assert_eq!(h.frame().bg_at(ex - 2, ey), h.frame().bg_at(ex - 2, ey - 1));

    h.keys("<Esc>");
    assert_eq!(h.frame().bg_at(ex + 2, ey), plain);

    h.drag((ex, ey), (kx + 9, ky));
    assert_ne!(h.frame().bg_at(ex + 2, ey), plain);
    h.click(ex, ey - 1);
    assert_eq!(h.frame().bg_at(ex + 2, ey), plain);
}

#[test]
fn y_in_visual_copies_the_selected_lines_as_code() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 100, 24);
    let (x, y) = h.frame().find("extra").unwrap();
    h.click(x, y);
    h.keys("vjy");
    assert_eq!(h.copied(), vec!["    extra();\n    keep_me();".to_string()]);
    // The selection is over.
    assert!(!h.screen().contains("VISUAL"), "{}", h.screen());
}

/// A Comment on the `extra` line, written and saved.
fn with_comment_on_extra(h: &mut Harness) -> (u16, u16) {
    let (x, y) = h.frame().find("extra").unwrap();
    h.click(x, y);
    h.keys("cicheck this<Esc><CR>");
    // Later clicks on the same cell must not chain into a double click.
    h.wait(1000);
    h.frame().find("extra").unwrap()
}

#[test]
fn space_y_in_visual_copies_the_code_with_its_comments_and_y_without() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 100, 24);
    let (x, y) = with_comment_on_extra(&mut h);
    h.click(x, y);
    h.keys("vj<Space>y");
    assert_eq!(
        h.copied(),
        vec!["    extra();\n> check this\n    keep_me();".to_string()]
    );
    h.keys("kvjy");
    assert_eq!(h.copied(), vec!["    extra();\n    keep_me();".to_string()]);
}

#[test]
fn space_y_after_a_mouse_selection_copies_the_code_with_its_comments() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 100, 24);
    let (x, y) = with_comment_on_extra(&mut h);
    let (_, ky) = h.frame().find("keep_me").unwrap();
    h.drag((x, y), (x + 3, ky));
    assert_eq!(h.copied(), vec!["extra();\n    keep".to_string()]);
    h.keys("<Space>y");
    assert_eq!(
        h.copied(),
        vec!["extra();\n> check this\n    keep".to_string()]
    );
}

#[test]
fn copying_code_leaves_comment_states_and_rounds_alone() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 100, 24);
    let (x, y) = with_comment_on_extra(&mut h);
    let before = h.screen();
    h.click(x, y);
    h.keys("vj<Space>y");
    h.keys("kvjy");
    h.wait(1000);
    let (_, ky) = h.frame().find("keep_me").unwrap();
    h.drag((x, y), (x + 3, ky));
    h.keys("<Space>y");
    assert_eq!(h.copied().len(), 4);
    h.keys("<Esc>");
    let after = h.screen();
    assert!(after.contains("Comment · open"), "{after}");
    assert!(after.contains("round:1"), "{after}");
    // A copied Prompt would have closed the Round and sent the Comment.
    assert!(!after.contains("Comment · sent"), "{after}");
    assert!(before.contains("Comment · open"), "{before}");
}

#[test]
fn y_in_visual_side_by_side_copies_a_changed_row_as_a_diff() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 160, 24);
    let (x, y) = h.frame().find("old_call").unwrap();
    h.click(x, y);
    h.keys("vjy");
    assert_eq!(
        h.copied(),
        vec!["-    old_call(1);\n+    new_call(2);\n+    extra();".to_string()]
    );
}

#[test]
fn set_nomouse_gives_the_mouse_back_to_the_terminal() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 100, 24);
    assert!(h.session().mouse_enabled());
    h.keys(":set nomouse<CR>");
    assert!(!h.session().mouse_enabled());
    let (x, y) = h.frame().find("extra").unwrap();
    let (kx, ky) = h.frame().find("keep_me").unwrap();
    h.drag((x, y), (kx + 9, ky));
    h.click(x, y);
    h.keys("c");
    // The click did not move the cursor and nothing was copied.
    assert!(h.screen().contains("new 1"), "{}", h.screen());
    assert!(h.copied().is_empty());

    h.keys("<Esc><Esc>:set mouse<CR>");
    assert!(h.session().mouse_enabled());
    h.drag((x, y), (kx + 9, ky));
    assert_eq!(h.copied().len(), 1);
}

#[test]
fn copied_code_cannot_smuggle_escape_sequences_or_direction_marks() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n").commit("initial");
    fx.branch("feature");
    fx.write(
        "a.txt",
        "one\nevil \u{1b}]52;c;AAAA\u{7} \u{202e}rtl\u{200f}\n",
    )
    .commit("change");
    let mut h = Harness::open(&fx, 100, 24);
    let (x, y) = h.frame().find("evil").unwrap();
    h.click(x, y).wait(1000);
    h.keys("vy");
    let copied = h.copied();
    assert_eq!(copied.len(), 1);
    assert!(!copied[0].contains('\u{1b}'), "{:?}", copied[0]);
    assert!(!copied[0].contains('\u{7}'), "{:?}", copied[0]);
    assert!(!copied[0].contains('\u{202e}'), "{:?}", copied[0]);
    assert!(!copied[0].contains('\u{200f}'), "{:?}", copied[0]);
    assert!(copied[0].starts_with("evil "), "{:?}", copied[0]);
}

#[test]
fn a_click_in_another_window_drops_the_selection() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 100, 24);
    let (ex, ey) = h.frame().find("extra").unwrap();
    let (kx, ky) = h.frame().find("keep_me").unwrap();
    let plain = h.frame().bg_at(ex + 2, ey);
    h.drag((ex, ey), (kx + 9, ky));
    assert_ne!(h.frame().bg_at(ex + 2, ey), plain);
    let (fx_, fy) = h.frame().find("src/lib.rs").unwrap();
    h.click(fx_, fy);
    assert_eq!(h.frame().bg_at(ex + 2, ey), plain);
}

#[test]
fn visual_after_a_mouse_selection_copies_the_visual_lines() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 100, 24);
    let (ex, ey) = h.frame().find("extra").unwrap();
    let (kx, ky) = h.frame().find("keep_me").unwrap();
    h.drag((ex, ey), (kx + 9, ky));
    h.copied();
    h.keys("vy");
    // The cursor stands on `keep_me`, where the mouse left it.
    assert_eq!(h.copied(), vec!["    keep_me();".to_string()]);
}

/// Two changed files; the first, `src/a.rs`, is shown.
fn two_files() -> Fixture {
    let fx = Fixture::new();
    fx.write("src/a.rs", "fn a() {\n    1\n}\n")
        .write("src/b.rs", "fn b() {\n    2\n}\n")
        .commit("initial");
    fx.branch("feature");
    fx.write("src/a.rs", "fn a() {\n    10\n}\n")
        .write("src/b.rs", "fn b() {\n    20\n}\n")
        .commit("change");
    fx
}

/// Select code in the first file, do `action`, and check that `y` has
/// nothing left to copy.
fn selection_is_dropped_by(action: impl FnOnce(&mut Harness, &Fixture)) {
    let fx = two_files();
    let mut h = Harness::open(&fx, 100, 24);
    let (x, y) = h.frame().find("fn a").unwrap();
    h.drag((x, y), (x + 3, y));
    assert_eq!(h.copied().len(), 1);
    h.wait(1000);
    action(&mut h, &fx);
    h.keys("y");
    assert!(h.copied().is_empty(), "{}", h.screen());
}

#[test]
fn the_selection_is_dropped_when_the_wheel_changes_the_file() {
    selection_is_dropped_by(|h, _| {
        let (x, y) = h.frame().find("src/b.rs").unwrap();
        h.wheel_down(x, y);
    });
}

#[test]
fn the_selection_is_dropped_when_the_file_changes_by_key() {
    selection_is_dropped_by(|h, _| {
        h.keys("]f");
    });
}

#[test]
fn the_selection_is_dropped_when_the_file_changes_by_click() {
    selection_is_dropped_by(|h, _| {
        let (x, y) = h.frame().find("src/b.rs").unwrap();
        h.click(x, y);
    });
}

#[test]
fn the_selection_is_dropped_by_a_reload() {
    selection_is_dropped_by(|h, fx| {
        fx.write("src/a.rs", "fn a() {\n    11\n}\n").commit("more");
        h.keys(":reload<CR>");
    });
}

#[test]
fn the_selection_is_dropped_when_the_layout_changes() {
    selection_is_dropped_by(|h, _| {
        h.keys("s");
    });
}

#[test]
fn the_selection_is_dropped_by_a_resize() {
    selection_is_dropped_by(|h, _| {
        h.resize(120, 24);
    });
}

#[test]
fn the_selection_is_dropped_when_the_scope_changes() {
    selection_is_dropped_by(|h, _| {
        h.keys("]c");
    });
}

/// Nothing was copied and the Reviewer is told so.
fn assert_nothing_to_copy(h: &mut Harness) {
    assert!(h.copied().is_empty());
    let screen = h.screen();
    assert!(screen.contains("Nothing to copy"), "{screen}");
    assert!(!screen.contains("Copied"), "{screen}");
}

#[test]
fn a_drag_over_a_fold_copies_nothing() {
    let fx = Fixture::new();
    let body: String = (1..=30).map(|n| format!("line {n}\n")).collect();
    fx.write("a.txt", &format!("top\n{body}bottom\n"))
        .commit("initial");
    fx.branch("feature");
    fx.write("a.txt", &format!("TOP\n{body}BOTTOM\n"))
        .commit("change");
    let mut h = Harness::open(&fx, 100, 24);
    let (x, y) = h.frame().find("unchanged lines").unwrap();
    h.drag((x, y), (x + 5, y));
    assert_nothing_to_copy(&mut h);
}

#[test]
fn a_drag_over_the_empty_half_of_side_by_side_copies_nothing() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 160, 24);
    let (_, y) = h.frame().find("extra").unwrap();
    // `extra` is an added line: its left half is empty.
    h.drag((40, y), (50, y));
    assert_nothing_to_copy(&mut h);
}

#[test]
fn a_drag_that_stays_in_one_cell_copies_nothing_but_a_one_character_word_still_does() {
    let fx = changed_file();
    let mut h = Harness::open(&fx, 100, 24);
    let (x, y) = h.frame().find("extra").unwrap();
    h.drag((x + 1, y), (x + 1, y));
    assert!(h.copied().is_empty());
    assert!(!h.screen().contains("Copied"), "{}", h.screen());

    let (bx, by) = h.frame().find("alpha() {").unwrap();
    h.wait(1000);
    h.click(bx + 8, by).click(bx + 8, by);
    assert_eq!(h.copied(), vec!["{".to_string()]);
}

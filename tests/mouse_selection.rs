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
// END

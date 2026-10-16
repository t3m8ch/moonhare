//! Cursor, folds, hunks and scrolling inside the diff pane.

mod support;

use ratatui::style::Color;
use support::{Fixture, Harness};

/// `main` has a tiny file; the branch adds a 100-line file.
fn long_file() -> Fixture {
    let fx = Fixture::new();
    fx.write("README.md", "x\n").commit("initial");
    fx.branch("feature");
    let body: String = (1..=100).map(|i| format!("line number {i:03}\n")).collect();
    fx.write("long.txt", &body).commit("add long file");
    fx
}

/// Text of the row whose line number is highlighted as the cursor.
fn cursor_row(h: &mut Harness) -> String {
    let frame = h.frame();
    let text = frame.text();
    for (y, row) in text.lines().enumerate() {
        // The diff pane starts at column 32; the gutter is a few columns wide.
        if (32..40).any(|x| frame.fg_at(x, y as u16) == Color::Indexed(75)) {
            return row.to_string();
        }
    }
    panic!("no cursor row on screen:\n{text}");
}

#[test]
fn j_and_k_move_a_cursor_and_the_view_follows_only_at_the_edge() {
    let fx = long_file();
    let mut h = Harness::open(&fx, 160, 21);
    assert!(cursor_row(&mut h).contains("line number 001"));

    h.keys("j");
    assert!(cursor_row(&mut h).contains("line number 002"));
    assert!(h.screen().contains("line number 001"), "view did not move");

    h.keys("30j");
    assert!(cursor_row(&mut h).contains("line number 032"));
    let screen = h.screen();
    assert!(
        !screen.contains("line number 001"),
        "view followed:\n{screen}"
    );

    h.keys("5k");
    assert!(cursor_row(&mut h).contains("line number 027"));
}

#[test]
fn a_lone_zero_is_not_swallowed_by_counts_and_ten_j_moves_ten() {
    let fx = long_file();
    let mut h = Harness::open(&fx, 160, 21);
    h.keys("10j");
    assert!(cursor_row(&mut h).contains("line number 011"));
}

#[test]
fn ctrl_d_and_ctrl_u_move_the_cursor_and_the_view_by_half_a_page() {
    let fx = long_file();
    let mut h = Harness::open(&fx, 160, 21);
    // The pane shows 18 lines, so half a page is 9.
    h.keys("<C-d>");
    assert!(cursor_row(&mut h).contains("line number 010"));
    assert!(!h.screen().contains("line number 001"));
    h.keys("<C-d>");
    assert!(cursor_row(&mut h).contains("line number 019"));
    h.keys("<C-u>");
    assert!(cursor_row(&mut h).contains("line number 010"));
    assert!(h.screen().contains("line number 010"));
}

#[test]
fn ctrl_f_and_ctrl_b_move_a_page_with_two_lines_of_overlap() {
    let fx = long_file();
    let mut h = Harness::open(&fx, 160, 21);
    h.keys("<C-f>");
    let screen = h.screen();
    assert!(!screen.contains("line number 016"), "{screen}");
    assert!(screen.contains("line number 017"), "two lines of overlap");
    assert!(cursor_row(&mut h).contains("line number 017"));
    h.keys("<C-b>");
    assert!(h.screen().contains("line number 001"));
    assert!(cursor_row(&mut h).contains("line number 018"));
}

#[test]
fn ctrl_e_and_ctrl_y_scroll_the_view_while_the_cursor_stays_until_it_leaves() {
    let fx = long_file();
    let mut h = Harness::open(&fx, 160, 21);
    h.keys("5j3<C-e>");
    let screen = h.screen();
    assert!(!screen.contains("line number 003"), "{screen}");
    assert!(screen.contains("line number 004"));
    assert!(
        cursor_row(&mut h).contains("line number 006"),
        "cursor did not move"
    );

    h.keys("10<C-e>");
    assert!(
        cursor_row(&mut h).contains("line number 014"),
        "pushed to the top of the view"
    );

    h.keys("30j30<C-y>");
    assert!(h.screen().contains("line number 001"));
    assert!(
        cursor_row(&mut h).contains("line number 018"),
        "pushed to the bottom of the view"
    );
}

#[test]
fn gg_and_capital_g_and_counted_g_jump_to_lines() {
    let fx = long_file();
    let mut h = Harness::open(&fx, 160, 21);
    h.keys("G");
    assert!(cursor_row(&mut h).contains("line number 100"));
    h.keys("12G");
    assert!(cursor_row(&mut h).contains("line number 012"));
    h.keys("50gg");
    assert!(cursor_row(&mut h).contains("line number 050"));
    h.keys("gg");
    assert!(cursor_row(&mut h).contains("line number 001"));
    h.keys("500G");
    assert!(cursor_row(&mut h).contains("line number 100"), "clamped");
}

/// 80 numbered lines; lines 30 and 70 differ.
fn two_hunks() -> Fixture {
    let fx = Fixture::new();
    let text = |changed: &[usize]| -> String {
        (1..=80)
            .map(|i| {
                if changed.contains(&i) {
                    format!("row {i:02} CHANGED\n")
                } else {
                    format!("row {i:02}\n")
                }
            })
            .collect()
    };
    fx.write("rows.txt", &text(&[])).commit("initial");
    fx.branch("feature");
    fx.write("rows.txt", &text(&[30, 70])).commit("change");
    fx
}

#[test]
fn unchanged_stretches_fold_with_a_counter_in_both_layouts() {
    let fx = two_hunks();
    for (width, layout) in [(160, "side-by-side"), (100, "inline")] {
        let mut h = Harness::open(&fx, width, 40);
        let screen = h.screen();
        // Rows 1-29 are above the first change (3 stay as context): 26 hidden.
        assert!(
            screen.contains("⋯ 26 unchanged lines"),
            "{layout}:\n{screen}"
        );
        // Rows 31-69 sit between the hunks (3 context each side): 39 - 6 = 33.
        assert!(
            screen.contains("⋯ 33 unchanged lines"),
            "{layout}:\n{screen}"
        );
        assert!(screen.contains("row 27"), "{layout}: context kept");
        assert!(!screen.contains("row 26"), "{layout}: folded away");
        assert!(screen.contains("row 30 CHANGED"));
    }
}

#[test]
fn short_unchanged_stretches_are_not_folded() {
    let fx = Fixture::new();
    let text = |a: &str, b: &str| format!("one\ntwo\nthree\nfour\n{a}\nfive\nsix\nseven\n{b}\n");
    fx.write("f.txt", &text("A", "B")).commit("initial");
    fx.branch("feature");
    fx.write("f.txt", &text("a", "b")).commit("change");
    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    assert!(!screen.contains("unchanged lines"), "{screen}");
    assert!(screen.contains("one") && screen.contains("seven"));
}

#[test]
fn hunk_jumps_move_between_hunks_and_hint_at_the_file_boundaries() {
    let fx = two_hunks();
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("]h");
    assert!(cursor_row(&mut h).contains("row 30 CHANGED"));
    h.keys("]h");
    assert!(cursor_row(&mut h).contains("row 70 CHANGED"));
    h.keys("]h");
    assert!(h.screen().contains("No more hunks"));
    assert!(cursor_row(&mut h).contains("row 70 CHANGED"), "stays put");

    h.keys("[h");
    assert!(cursor_row(&mut h).contains("row 30 CHANGED"));
    h.keys("[h");
    assert!(h.screen().contains("No previous hunk"));
    assert!(cursor_row(&mut h).contains("row 30 CHANGED"), "stays put");
}

#[test]
fn hunk_jumps_work_in_inline_and_with_a_count() {
    let fx = two_hunks();
    let mut h = Harness::open(&fx, 100, 40);
    // A hunk starts with its removed line in inline.
    h.keys("2]h");
    assert!(cursor_row(&mut h).contains("row 70"));
    h.keys("2[h");
    assert!(cursor_row(&mut h).contains("row 30"));
}

/// A new file with one very long line made of numbered words.
fn long_line() -> Fixture {
    let fx = Fixture::new();
    fx.write("README.md", "x\n").commit("initial");
    fx.branch("feature");
    let line: String = (0..60).map(|i| format!("w{i:02}_")).collect();
    fx.write("wide.txt", &format!("{line}\n"))
        .commit("add wide");
    fx
}

#[test]
fn h_and_l_scroll_long_lines_sideways_and_a_lone_zero_resets() {
    let fx = long_line();
    for width in [160u16, 100] {
        let mut h = Harness::open(&fx, width, 20);
        assert!(h.screen().contains("w00_"), "{width}: starts at the left");

        h.keys("l");
        let screen = h.screen();
        assert!(
            !screen.contains("w01_"),
            "{width}: scrolled right:\n{screen}"
        );
        assert!(screen.contains("w02_"));

        h.keys("h");
        assert!(h.screen().contains("w00_"), "{width}: h scrolls back");

        h.keys("llll0");
        assert!(h.screen().contains("w00_"), "{width}: 0 resets");

        h.keys("1000l");
        let screen = h.screen();
        assert!(screen.contains("w59_"), "{width}: reaches the end");
        assert!(!screen.contains("w00_"));
    }
}

#[test]
fn horizontal_offset_survives_a_layout_switch() {
    let fx = long_line();
    let mut h = Harness::open(&fx, 160, 21);
    h.keys("s"); // inline
    h.keys("l");
    assert!(!h.screen().contains("w01_"));
    h.keys("s"); // back to side-by-side
    assert!(!h.screen().contains("w01_"));
}

/// Time budget for one keypress plus redraw, in a debug build.
const KEY_BUDGET: std::time::Duration = std::time::Duration::from_millis(250);

#[test]
fn a_700_line_file_and_a_3000_line_range_scroll_without_noticeable_delay() {
    let fx = Fixture::new();
    fx.write("README.md", "x\n").commit("initial");
    fx.branch("feature");
    let rust = |n: usize| -> String {
        (0..n)
            .map(|i| format!("fn f{i}(x: i32) -> i32 {{\n    x + {i}\n}}\n"))
            .collect()
    };
    // 700 lines of Rust in one file, and 3000 more spread over 30 files.
    fx.write("big.rs", &rust(233));
    for f in 0..30 {
        fx.write(&format!("src/m{f:02}.rs"), &rust(33));
    }
    fx.commit("add lots of code");

    let started = std::time::Instant::now();
    let mut h = Harness::open(&fx, 160, 50);
    let open_time = started.elapsed();
    assert!(open_time < KEY_BUDGET * 4, "opening took {open_time:?}");

    let mut slowest = std::time::Duration::ZERO;
    let mut press = |h: &mut Harness, keys: &str| {
        let t = std::time::Instant::now();
        h.keys(keys);
        h.screen();
        slowest = slowest.max(t.elapsed());
    };
    for keys in [
        "G", "gg", "<C-d>", "<C-d>", "<C-f>", "<C-b>", "<C-e>", "<C-y>", "500j", "s", "G", "gg",
        "]h", "]f", "G", "]f", "s",
    ] {
        press(&mut h, keys);
    }
    for _ in 0..29 {
        press(&mut h, "]f");
    }
    assert!(slowest < KEY_BUDGET, "slowest keypress took {slowest:?}");
}

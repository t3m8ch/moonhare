//! Revealing the unchanged lines a fold hides, and folding them back.

mod support;

use ratatui::style::Color;
use support::{Fixture, Harness};

/// `rows.rs` of `total` numbered `let` lines; the branch changes the lines
/// numbered in `changed` (1-based).
fn rows(total: usize, changed: &[usize]) -> Fixture {
    let fx = Fixture::new();
    let text = |changed: &[usize]| -> String {
        (1..=total)
            .map(|i| {
                if changed.contains(&i) {
                    format!("let v{i:03} = {i} + 1000;\n")
                } else {
                    format!("let v{i:03} = {i};\n")
                }
            })
            .collect()
    };
    fx.write("rows.rs", &text(&[])).commit("initial");
    fx.branch("feature");
    fx.write("rows.rs", &text(changed)).commit("change");
    fx
}

/// 80 lines, 30 and 70 changed: 26 lines folded above the first change and
/// 33 between the two.
fn small_folds() -> Fixture {
    rows(80, &[30, 70])
}

/// Text of the row whose line number is highlighted as the cursor.
fn cursor_row(h: &mut Harness) -> String {
    let frame = h.frame();
    let text = frame.text();
    for (y, row) in text.lines().enumerate() {
        if (32..40).any(|x| frame.fg_at(x, y as u16) == Color::Indexed(75)) {
            return row.to_string();
        }
    }
    panic!("no cursor row on screen:\n{text}");
}

/// The line number shown before the code of line `n` (the right-hand one in
/// side-by-side), if the line is on screen.
fn number_of(screen: &str, n: usize) -> Option<usize> {
    let code = format!("let v{n:03} = {n};");
    let row = screen.lines().find(|l| l.contains(&code))?;
    let gutter = &row[..row.rfind(&code)?];
    gutter
        .split([' ', '│'])
        .rfind(|t| !t.is_empty())?
        .parse()
        .ok()
}

#[test]
fn enter_on_a_small_fold_reveals_it_whole_with_numbers_and_highlighting() {
    let fx = small_folds();
    for (width, layout) in [(160, "side-by-side"), (100, "inline")] {
        let mut h = Harness::open(&fx, width, 50);
        assert!(h.screen().contains("⋯ 26 unchanged lines"), "{layout}");
        // The cursor starts on the fold at the top of the file.
        h.keys("<CR>");
        let screen = h.screen();
        assert!(!screen.contains("26 unchanged"), "{layout}:\n{screen}");
        assert!(
            screen.contains("⋯ 33 unchanged lines"),
            "{layout}: the other stays"
        );
        for n in [1, 13, 26] {
            assert_eq!(number_of(&screen, n), Some(n), "{layout}:\n{screen}");
        }
        // A revealed line is highlighted like the context kept around it.
        let frame = h.frame();
        let (x1, y1) = frame.find("let v001").unwrap();
        let (x27, y27) = frame.find("let v027").unwrap();
        assert_eq!(frame.fg_at(x1, y1), frame.fg_at(x27, y27), "{layout}");
        assert_ne!(
            frame.fg_at(x1 + 4, y1),
            frame.fg_at(x1, y1),
            "{layout}: highlighted"
        );
        assert!(cursor_row(&mut h).contains("let v001"), "{layout}");
    }
}

/// 120 lines, 10 and 100 changed: lines 14-96 make one fold of 83.
fn big_fold() -> Fixture {
    rows(120, &[10, 100])
}

fn shown(screen: &str, n: usize) -> bool {
    screen.contains(&format!("let v{n:03} = {n};"))
}

#[test]
fn zo_reveals_a_big_fold_a_chunk_at_a_time_from_the_side_the_cursor_came() {
    let fx = big_fold();
    let mut h = Harness::open(&fx, 160, 60);
    // Down from the first hunk: the chunk under it opens.
    h.keys("8j");
    assert!(cursor_row(&mut h).contains("⋯ 83 unchanged lines"));
    h.keys("zo");
    let screen = h.screen();
    assert!(screen.contains("⋯ 63 unchanged lines"), "{screen}");
    assert!(shown(&screen, 14) && shown(&screen, 33), "{screen}");
    assert!(!shown(&screen, 34), "{screen}");
    assert!(
        cursor_row(&mut h).contains("⋯ 63 unchanged lines"),
        "stays on the fold"
    );
    // Enter does the same.
    h.keys("<CR>");
    assert!(h.screen().contains("⋯ 43 unchanged lines"));

    // Up from the second hunk: the chunk over it opens.
    let mut h = Harness::open(&fx, 160, 60);
    h.keys("]h]h4k");
    assert!(cursor_row(&mut h).contains("⋯ 83 unchanged lines"));
    h.keys("zo");
    let screen = h.screen();
    assert!(screen.contains("⋯ 63 unchanged lines"), "{screen}");
    assert!(shown(&screen, 77) && shown(&screen, 96), "{screen}");
    assert!(!shown(&screen, 76), "{screen}");
    assert!(cursor_row(&mut h).contains("⋯ 63 unchanged lines"));
}

#[test]
fn capital_zo_reveals_a_big_fold_whole() {
    let fx = big_fold();
    let mut h = Harness::open(&fx, 160, 60);
    h.keys("8jzOgg");
    let screen = h.screen();
    assert!(!screen.contains("83 unchanged"), "{screen}");
    // The 6 lines over the first change and 17 under the last stay folded.
    assert!(screen.contains("⋯ 6 unchanged lines"), "{screen}");
    h.keys("G");
    assert!(h.screen().contains("⋯ 17 unchanged lines"));
    for n in [14, 50, 96] {
        h.keys(&format!("{n}G"));
        assert!(cursor_row(&mut h).contains(&format!("let v{n:03}")), "{n}");
    }
}

#[test]
fn zc_and_za_fold_revealed_lines_back() {
    let fx = small_folds();
    for (width, layout) in [(160, "side-by-side"), (100, "inline")] {
        let mut h = Harness::open(&fx, width, 50);
        h.keys("<CR>5j");
        assert!(cursor_row(&mut h).contains("let v006"), "{layout}");
        h.keys("zc");
        let screen = h.screen();
        assert!(
            screen.contains("⋯ 26 unchanged lines"),
            "{layout}:\n{screen}"
        );
        assert!(!shown(&screen, 6), "{layout}");
        assert!(cursor_row(&mut h).contains("⋯ 26 unchanged"), "{layout}");

        h.keys("za");
        assert!(!h.screen().contains("26 unchanged"), "{layout}: za opens");
        h.keys("10j");
        h.keys("za");
        assert!(
            h.screen().contains("⋯ 26 unchanged lines"),
            "{layout}: za folds"
        );

        // Nothing revealed around a changed line.
        h.keys("]h");
        h.keys("zc");
        assert!(h.screen().contains("No revealed lines here"), "{layout}");
    }
}

#[test]
fn capital_zr_reveals_every_fold_of_the_file_and_capital_zm_folds_them_back() {
    let fx = small_folds();
    for (width, layout) in [(160, "side-by-side"), (100, "inline")] {
        let mut h = Harness::open(&fx, width, 50);
        h.keys("]h");
        h.keys("zR");
        let screen = h.screen();
        assert!(!screen.contains("unchanged lines"), "{layout}:\n{screen}");
        assert!(cursor_row(&mut h).contains("let v030"), "{layout}");
        h.keys("gg");
        assert!(h.screen().contains("let v001 = 1;"), "{layout}");
        h.keys("50G");
        assert!(cursor_row(&mut h).contains("let v050"), "{layout}");
        h.keys("zM");
        let screen = h.screen();
        assert!(
            screen.contains("⋯ 26 unchanged lines"),
            "{layout}:\n{screen}"
        );
        assert!(
            screen.contains("⋯ 33 unchanged lines"),
            "{layout}:\n{screen}"
        );
        assert!(
            cursor_row(&mut h).contains("⋯ 33 unchanged"),
            "{layout}: on the fold of 50"
        );
    }
}

#[test]
fn a_click_on_a_fold_reveals_it() {
    let fx = small_folds();
    for (width, layout) in [(160, "side-by-side"), (100, "inline")] {
        let mut h = Harness::open(&fx, width, 50);
        let (x, y) = h.frame().find("⋯ 33 unchanged lines").unwrap();
        h.click(x, y);
        let screen = h.screen();
        assert!(!screen.contains("33 unchanged"), "{layout}:\n{screen}");
        assert!(shown(&screen, 50), "{layout}");
        assert!(screen.contains("⋯ 26 unchanged lines"), "{layout}");
        assert!(h.copied().is_empty(), "{layout}: a click copies nothing");
    }
}

/// `rows.rs` as in [`small_folds`], and a second changed file after it.
fn two_files() -> Fixture {
    let fx = small_folds();
    fx.write("zz.txt", "new\n").commit("add zz");
    fx
}

#[test]
fn revealed_lines_survive_other_files_a_resize_and_a_layout_switch() {
    let fx = two_files();
    let mut h = Harness::open(&fx, 160, 50);
    h.keys("<CR>]f");
    assert!(h.screen().contains("zz.txt"));
    h.keys("[f");
    let screen = h.screen();
    assert!(!screen.contains("26 unchanged"), "{screen}");
    assert!(screen.contains("⋯ 33 unchanged lines"));

    // Narrow enough for inline, then back.
    h.resize(100, 40);
    let screen = h.screen();
    assert!(screen.contains("layout:inline"), "{screen}");
    assert!(
        !screen.contains("26 unchanged") && shown(&screen, 5),
        "{screen}"
    );
    h.keys("s");
    assert!(!h.screen().contains("26 unchanged"));
    h.resize(160, 50);
    assert!(!h.screen().contains("26 unchanged"));
}

#[test]
fn another_scope_starts_folded_and_coming_back_finds_the_lines_revealed() {
    let fx = two_files();
    let mut h = Harness::open(&fx, 160, 50);
    h.keys("<CR>");
    // Commit 1 is the change of rows.rs alone: other versions, other folds.
    h.keys(":scope 1<CR>");
    let screen = h.screen();
    assert!(screen.contains("⋯ 26 unchanged lines"), "{screen}");
    h.keys("zRzM:scope 2<CR>:scope all<CR>");
    assert!(!h.screen().contains("26 unchanged"));
}

#[test]
fn hunk_jumps_skip_revealed_lines() {
    let fx = small_folds();
    for (width, layout) in [(160, "side-by-side"), (100, "inline")] {
        let mut h = Harness::open(&fx, width, 50);
        h.keys("zR");
        h.keys("]h");
        assert!(cursor_row(&mut h).contains("let v030"), "{layout}");
        h.keys("]h");
        assert!(cursor_row(&mut h).contains("let v070"), "{layout}");
        h.keys("[h[h");
        assert!(cursor_row(&mut h).contains("let v030"), "{layout}");
        h.keys("[h");
        assert!(h.screen().contains("No previous hunk"), "{layout}");
    }
}

#[test]
fn a_comment_on_a_revealed_line_hangs_under_it_and_keeps_it_out_of_folds() {
    let fx = small_folds();
    let mut h = Harness::open(&fx, 160, 50);
    h.keys("<CR>12jcilook here<Esc><CR>");
    let screen = h.screen();
    let code = screen.lines().position(|l| l.contains("let v013")).unwrap();
    let note = screen
        .lines()
        .position(|l| l.contains("look here"))
        .unwrap();
    assert!(note > code && note <= code + 3, "{screen}");

    h.keys("zM");
    let screen = h.screen();
    assert!(shown(&screen, 13), "the commented line stays:\n{screen}");
    assert!(screen.contains("look here"), "{screen}");
    assert!(screen.contains("⋯ 12 unchanged lines"), "{screen}");
    assert!(screen.contains("⋯ 13 unchanged lines"), "{screen}");
}

#[test]
fn marking_a_file_reviewed_keeps_its_lines_revealed() {
    let fx = two_files();
    let mut h = Harness::open(&fx, 160, 50);
    h.keys("<CR>m");
    let screen = h.screen();
    assert!(!screen.contains("26 unchanged"), "{screen}");
    assert!(screen.contains("reviewed 1/2"), "{screen}");
}

#[test]
fn z_in_the_diff_lists_the_fold_keys_of_the_diff() {
    let fx = small_folds();
    let mut h = Harness::open(&fx, 160, 50);
    assert!(h.screen().contains("Enter/zo: раскрыть"), "hint line");
    h.keys("z").wait(1000);
    let screen = h.screen();
    assert!(screen.contains("which-key: z"), "{screen}");
    assert!(
        screen.contains("дифф: раскрыть свёрнутые строки целиком"),
        "{screen}"
    );
    assert!(
        !screen.contains("каталог") && !screen.contains("раунд"),
        "{screen}"
    );
    h.keys("<Esc>");
    let help = moonhare::hints::help_lines().join("\n");
    assert!(help.contains("zO ") && help.contains("Enter"), "{help}");
}

#[test]
fn a_big_fold_still_opens_from_above_after_folding_back() {
    let fx = big_fold();
    let mut h = Harness::open(&fx, 160, 60);
    // Down into the revealed lines, and fold them back: the cursor jumps up
    // to the fold by itself, which is not the Reviewer walking up.
    h.keys("8jzO10jzc");
    assert!(cursor_row(&mut h).contains("⋯ 83 unchanged lines"));
    h.keys("zo");
    let screen = h.screen();
    assert!(shown(&screen, 14) && !shown(&screen, 96), "{screen}");
}

#[test]
fn walking_and_scrolling_go_through_revealed_lines() {
    let fx = big_fold();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("8jzO");
    assert!(cursor_row(&mut h).contains("let v014"));
    h.keys("j");
    assert!(cursor_row(&mut h).contains("let v015"));
    h.keys("<C-d>");
    assert!(cursor_row(&mut h).contains("let v028"), "half of 27 rows");
    h.keys("<C-f>");
    let screen = h.screen();
    assert!(shown(&screen, 60), "{screen}");
    h.keys("G");
    assert!(cursor_row(&mut h).contains("⋯ 17 unchanged lines"));
    h.keys("gg");
    assert!(cursor_row(&mut h).contains("⋯ 6 unchanged lines"));
}

#[test]
fn new_commits_fold_the_file_again_without_a_panic() {
    let fx = small_folds();
    let mut h = Harness::open(&fx, 160, 50);
    h.keys("zR");
    let text: String = (1..=60)
        .map(|i| format!("let v{i:03} = {i};\n"))
        .collect::<String>()
        + "let tail = 0;\n";
    fx.write("rows.rs", &text).commit("shorter");
    h.wait(5_000).keys("R");
    let screen = h.screen();
    assert!(screen.contains("unchanged lines"), "{screen}");
    h.keys("GzRzMgg");
    h.screen();
}

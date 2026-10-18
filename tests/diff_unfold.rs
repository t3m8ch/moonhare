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
        .filter(|t| !t.is_empty())
        .next_back()?
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
        assert!(screen.contains("⋯ 33 unchanged lines"), "{layout}: the other stays");
        for n in [1, 13, 26] {
            assert_eq!(number_of(&screen, n), Some(n), "{layout}:\n{screen}");
        }
        // A revealed line is highlighted like the context kept around it.
        let frame = h.frame();
        let (x1, y1) = frame.find("let v001").unwrap();
        let (x27, y27) = frame.find("let v027").unwrap();
        assert_eq!(frame.fg_at(x1, y1), frame.fg_at(x27, y27), "{layout}");
        assert_ne!(frame.fg_at(x1 + 4, y1), frame.fg_at(x1, y1), "{layout}: highlighted");
        assert!(cursor_row(&mut h).contains("let v001"), "{layout}");
    }
}

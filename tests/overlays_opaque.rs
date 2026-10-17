//! Overlays clear their whole area: nothing of the diff underneath (text or
//! background colours) shows through a box.

mod support;

use ratatui::style::Color;
use support::*;

/// Cells of the box whose title row holds `title`: from its `┌` to the `└`
/// below it, as (column, row) pairs.
fn box_cells(h: &mut Harness, title: &str) -> Vec<(u16, u16)> {
    let screen = h.screen();
    let rows: Vec<Vec<char>> = screen.lines().map(|l| l.chars().collect()).collect();
    let (at, top) = h
        .frame()
        .find(title)
        .unwrap_or_else(|| panic!("{title}:\n{screen}"));
    let top_row = &rows[top as usize];
    let left = top_row[..at as usize]
        .iter()
        .rposition(|&c| c == '┌')
        .expect("┌") as u16;
    let right = top_row[at as usize..]
        .iter()
        .position(|&c| c == '┐')
        .expect("┐") as u16
        + at;
    let bottom = (top as usize..rows.len())
        .find(|&y| rows[y].get(left as usize) == Some(&'└'))
        .expect("└") as u16;
    let mut cells = Vec::new();
    for y in top..=bottom {
        for x in left..=right {
            cells.push((x, y));
        }
    }
    cells
}

fn assert_opaque(h: &mut Harness, title: &str) {
    assert_opaque_but(h, title, &[]);
}

/// Like [`assert_opaque`], with the overlay's own `own` backgrounds allowed.
fn assert_opaque_but(h: &mut Harness, title: &str, own: &[Color]) {
    let cells = box_cells(h, title);
    let frame = h.frame();
    let screen = h.screen();
    for (x, y) in cells {
        let bg = frame.bg_at(x, y);
        assert!(
            bg == Color::Reset || own.contains(&bg),
            "{bg:?}: diff background shows through at ({x},{y}):\n{screen}"
        );
    }
}

/// A diff with added and removed lines under every overlay.
fn open(width: u16, height: u16) -> Harness {
    let fx = Fixture::new();
    let old: String = (0..60).map(|n| format!("let old_{n} = {n};\n")).collect();
    let new: String = (0..60).map(|n| format!("let new_{n} = {n};\n")).collect();
    fx.write("src/long.rs", &old).commit("initial");
    fx.branch("feature");
    fx.write("src/long.rs", &new).commit("rewrite");
    Harness::open(&fx, width, height)
}

#[test]
fn the_file_picker_hides_the_diff_underneath() {
    let mut h = open(100, 30);
    h.keys("<Space>f");
    assert_opaque_but(&mut h, "Pick a file", &[Color::Indexed(238)]);
}

#[test]
fn the_help_hides_the_diff_underneath() {
    let mut h = open(100, 30);
    h.keys("?");
    assert_opaque(&mut h, "Справка");
    // The help covers column 0 too: no leftover of the sidebar frame.
    let screen = h.screen();
    for (y, row) in screen.lines().take(25).enumerate() {
        assert!(row.starts_with(['┌', '│', '└']), "row {y}:\n{screen}");
    }
}

#[test]
fn which_key_hides_the_diff_underneath() {
    let mut h = open(100, 30);
    h.keys("<Space>").wait(2000);
    let screen = h.screen();
    assert!(screen.contains(" Space "), "{screen}");
    assert_opaque(&mut h, " Space ");
}

#[test]
fn the_scope_picker_hides_the_diff_underneath() {
    let mut h = open(100, 30);
    h.keys("<Space>c");
    assert_opaque_but(
        &mut h,
        "Pick a Scope",
        &[Color::Indexed(24), Color::Indexed(238)],
    );
}

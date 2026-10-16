//! Adaptive layout: the sidebar toggle, narrow terminals, pickers without a
//! sidebar, resizing and colours.

mod support;

use ratatui::style::Color;
use support::{feature_branch, Fixture, Harness};

fn has_sidebar(screen: &str) -> bool {
    screen.contains("Commits") && screen.contains("Files (")
}

#[test]
fn space_e_hides_and_shows_the_sidebar() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    assert!(has_sidebar(&h.screen()));
    h.keys("<Space>e");
    let hidden = h.screen();
    assert!(!has_sidebar(&hidden), "{hidden}");
    assert!(
        hidden.contains("README.md"),
        "file bar names the file:\n{hidden}"
    );
    h.keys("<Space>e");
    assert!(has_sidebar(&h.screen()));
}

#[test]
fn the_file_bar_and_the_status_line_do_not_repeat_each_other() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    for hide in [false, true] {
        if hide {
            h.keys("<Space>e");
        }
        let screen = h.screen();
        let rows: Vec<&str> = screen.lines().collect();
        let bar = rows[0];
        assert!(
            bar.contains("README.md") && bar.contains("file 1/3"),
            "{bar}"
        );
        assert!(bar.contains("modified"), "state in the bar: {bar}");
        let status = rows[rows.len() - 2]; // the last row holds hints
        assert!(!status.contains("file 1/3"), "{status}");
        assert!(
            status.contains("main..feature") && status.contains("scope:all"),
            "{status}"
        );
    }
}

#[test]
fn space_f_picks_a_file_without_a_sidebar() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 80, 30);
    h.keys("<Space>f");
    let screen = h.screen();
    assert!(screen.contains("Pick a file"), "{screen}");
    for path in ["README.md", "src/a.rs", "src/b.rs"] {
        assert!(screen.contains(path), "{path}:\n{screen}");
    }
    h.keys("jj");
    h.keys("<CR>");
    let screen = h.screen();
    assert!(!screen.contains("Pick a file"), "{screen}");
    assert!(screen.contains("file 3/3"), "{screen}");

    h.keys("<Space>f<Esc>");
    let screen = h.screen();
    assert!(
        !screen.contains("Pick a file") && screen.contains("file 3/3"),
        "{screen}"
    );
}

#[test]
fn space_c_picks_a_scope_without_a_sidebar() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 80, 30);
    h.keys("<Space>cjj<CR>");
    let screen = h.screen();
    assert!(screen.contains("scope:2"), "{screen}");
}

fn long_file() -> Fixture {
    let fx = Fixture::new();
    fx.write("README.md", "x\n").commit("initial");
    fx.branch("feature");
    let body: String = (1..=100).map(|i| format!("line number {i:03}\n")).collect();
    fx.write("long.txt", &body).commit("add long file");
    fx.write("other.txt", "o\n").commit("add other");
    fx
}

/// Text of the row whose gutter is highlighted as the cursor.
fn cursor_row(h: &mut Harness) -> String {
    let frame = h.frame();
    let text = frame.text();
    for (y, row) in text.lines().enumerate() {
        let accent = (0..60).any(|x| frame.fg_at(x, y as u16) == Color::Indexed(75));
        if accent && row.contains("line number") {
            return row.to_string();
        }
    }
    panic!("no cursor row on screen:\n{text}");
}

#[test]
fn resizing_both_ways_keeps_the_cursor_scope_and_open_overlays() {
    let fx = long_file();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(":scope 1<CR>");
    h.keys("50j");
    assert!(cursor_row(&mut h).contains("line number 051"));

    // Narrower: the sidebar goes away and the diff turns inline.
    h.resize(80, 20);
    assert!(!has_sidebar(&h.screen()));
    assert!(
        cursor_row(&mut h).contains("line number 051"),
        "{}",
        h.screen()
    );
    assert!(h.screen().contains("scope:1"));

    // Wider again.
    h.resize(170, 40);
    assert!(has_sidebar(&h.screen()));
    assert!(
        cursor_row(&mut h).contains("line number 051"),
        "{}",
        h.screen()
    );
    assert!(h.screen().contains("scope:1"));

    // An open picker survives a resize with its cursor.
    h.keys("<Space>cj").resize(90, 24).resize(150, 30);
    let screen = h.screen();
    assert!(screen.contains("Pick a Scope"), "{screen}");
    h.keys("<CR>");
    assert!(h.screen().contains("scope:2"));

    // So does a half-typed command.
    h.keys(":scope 1").resize(70, 20);
    assert!(h.screen().contains(":scope 1"), "{}", h.screen());
    h.keys("<CR>");
    assert!(h.screen().contains("scope:1"));
}

#[test]
fn a_too_small_terminal_shows_the_notice_and_restores_everything_afterwards() {
    let fx = long_file();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(":scope 1<CR>40j<Space>f");
    h.resize(50, 10);
    let screen = h.screen();
    assert!(screen.contains("Terminal too small"), "{screen}");
    assert!(!screen.contains("line number"), "{screen}");
    h.resize(160, 30);
    let screen = h.screen();
    assert!(
        screen.contains("Pick a file") && screen.contains("scope:1"),
        "{screen}"
    );
    h.keys("<Esc>");
    assert!(cursor_row(&mut h).contains("line number 041"));
}

#[test]
fn colours_are_256_indexed_and_truecolor_switches_them_to_rgb_everywhere() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.frame();
    let (x, y) = screen.find("README.md").expect("file bar");
    assert!(matches!(screen.fg_at(x, y), Color::Indexed(_)));
    h.set_truecolor(true);
    let frame = h.frame();
    for y in 0..30 {
        for x in 0..160 {
            for c in [frame.fg_at(x, y), frame.bg_at(x, y)] {
                assert!(!matches!(c, Color::Indexed(_)), "({x},{y}) {c:?}");
            }
        }
    }
    assert_eq!(frame.fg_at(x, y), Color::Rgb(255, 255, 255));
    h.set_truecolor(false);
    assert!(matches!(h.frame().fg_at(x, y), Color::Indexed(_)));
}

#[test]
fn colorterm_enables_truecolor() {
    use moonhare::color::truecolor_from_colorterm as tc;
    assert!(tc(Some("truecolor")));
    assert!(tc(Some("24bit")));
    assert!(!tc(Some("")));
    assert!(!tc(None));
}

#[test]
fn hiding_the_sidebar_returns_the_keys_to_the_diff() {
    let fx = long_file();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("<Tab><Space>e");
    h.keys("3j");
    assert!(
        cursor_row(&mut h).contains("line number 004"),
        "{}",
        h.screen()
    );
}

#[test]
fn a_narrow_terminal_hides_the_sidebar_by_itself() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 80, 30);
    assert!(!has_sidebar(&h.screen()));
    h.resize(160, 30);
    assert!(has_sidebar(&h.screen()));
    h.resize(80, 30);
    assert!(!has_sidebar(&h.screen()));
}

#[test]
fn the_manual_choice_beats_the_width_until_the_next_toggle() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("<Space>e").resize(80, 30).resize(160, 30);
    assert!(
        !has_sidebar(&h.screen()),
        "hidden by hand stays hidden when wide"
    );
    h.keys("<Space>e").resize(80, 30);
    let screen = h.screen();
    assert!(
        has_sidebar(&screen),
        "shown by hand stays shown when narrow:\n{screen}"
    );
    h.keys("<Space>e");
    assert!(!has_sidebar(&h.screen()));
}

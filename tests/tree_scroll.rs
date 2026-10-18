//! The scroll of the file tree stays where the Reviewer put it: a click on a
//! visible row never moves the tree, the wheel is not undone by what comes
//! after it (the diff, the Groups, a tick), and keys scroll only as much as
//! the cursor needs.
mod support;

use std::sync::Arc;

use moonhare::analyst::{
    Analyst, AnalystError, Group, GroupRequest, Grouping, Summary, SummaryRequest,
};
use support::*;

/// Files of `dir`, `n` of them named `{prefix}_NN.rs`.
fn names(dir: &str, prefix: &str, n: usize) -> Vec<String> {
    (0..n)
        .map(|i| format!("{dir}/{prefix}_{i:02}.rs"))
        .collect()
}

/// `pkg/core/alpha` and `pkg/core/beta`, thirty files each.
fn big() -> Fixture {
    let fx = Fixture::new();
    fx.write("README.md", "x\n").commit("initial");
    fx.branch("feature");
    let mut paths = names("pkg/core/alpha", "a", 30);
    paths.extend(names("pkg/core/beta", "b", 30));
    for path in &paths {
        fx.write(path, &format!("{path}\n"));
    }
    fx.commit("files");
    fx
}

/// The lines of the file panel of the sidebar, the title line first, with
/// the cursor mark taken out: what stays put when nothing scrolls.
fn tree(h: &mut Harness) -> Vec<String> {
    h.screen()
        .lines()
        .map(|l| {
            l.chars()
                .take(32)
                .collect::<String>()
                .replacen("│>", "│ ", 1)
        })
        .skip_while(|l| !l.contains("Files ("))
        .take_while(|l| !l.starts_with('└'))
        .collect()
}

/// The line of the sidebar the cursor is on.
fn cursor_line(h: &mut Harness) -> usize {
    let screen = h.screen();
    screen
        .lines()
        .map(|l| l.chars().take(32).collect::<String>())
        .skip_while(|l| !l.contains("Files ("))
        .position(|l| l.starts_with("│>"))
        .unwrap_or_else(|| panic!("no cursor:\n{screen}"))
}

/// The screen column and row of the first `needle` in the left `width`
/// columns: the sidebar, or the file picker over it.
fn at_in(h: &mut Harness, needle: &str, width: usize) -> (u16, u16) {
    let screen = h.screen();
    for (y, line) in screen.lines().enumerate() {
        let left: String = line.chars().take(width).collect();
        if let Some(byte) = left.find(needle) {
            return (left[..byte].chars().count() as u16, y as u16);
        }
    }
    panic!("no {needle:?}:\n{screen}")
}

/// Where `needle` is in the sidebar.
fn at(h: &mut Harness, needle: &str) -> (u16, u16) {
    at_in(h, needle, 32)
}

/// Opened, with the tree scrolled down by the wheel `notches` times.
fn scrolled(fx: &Fixture, notches: usize) -> Harness {
    let mut h = Harness::open(fx, 160, 40);
    let (x, y) = at(&mut h, "a_05.rs");
    for _ in 0..notches {
        h.wheel_down(x, y);
    }
    h
}

/// The line of the sidebar that holds `needle`.
fn line_of(rows: &[String], needle: &str) -> usize {
    rows.iter()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no {needle:?}:\n{}", rows.join("\n")))
}

#[test]
fn a_click_on_a_file_after_the_wheel_leaves_the_tree_where_it_is() {
    let fx = big();
    let mut h = scrolled(&fx, 3);
    let before = tree(&mut h);
    assert!(
        !before.join("\n").contains("a_00.rs"),
        "scrolled:\n{}",
        before.join("\n")
    );
    let (x, y) = at(&mut h, "a_20.rs");
    h.click(x, y);
    let after = tree(&mut h);
    assert_eq!(after, before, "the tree moved");
    assert_eq!(cursor_line(&mut h), line_of(&before, "a_20.rs"));
    // The diff comes from a worker thread: its arrival moves nothing.
    h.settle().wait(300);
    assert_eq!(tree(&mut h), before);
}

#[test]
fn the_wheel_is_not_undone_by_ticks_or_the_open_file() {
    let fx = big();
    let mut h = scrolled(&fx, 3);
    let before = tree(&mut h);
    h.wait(500).settle();
    assert_eq!(tree(&mut h), before);
}

#[test]
fn a_click_on_a_directory_after_the_wheel_keeps_its_row_in_place() {
    let fx = big();
    let mut h = scrolled(&fx, 9);
    let before = tree(&mut h);
    let line = line_of(&before, "▾ beta");
    let (x, y) = at(&mut h, "▾ beta");
    h.click(x, y);
    let after = tree(&mut h);
    assert_eq!(line_of(&after, "▸ beta"), line, "{}", after.join("\n"));
    assert_eq!(after[..line], before[..line], "{}", after.join("\n"));
}

#[test]
fn a_click_under_pinned_rows_does_not_jump() {
    let fx = big();
    let mut h = scrolled(&fx, 4);
    let before = tree(&mut h);
    assert!(before[1].contains("pkg/core") && before[2].contains("alpha"));
    // Every visible file, top to bottom: none of the clicks moves the tree.
    for name in ["a_14.rs", "a_25.rs", "a_13.rs"] {
        let (x, y) = at(&mut h, name);
        h.click(x, y);
        assert_eq!(tree(&mut h), before, "after {name}");
        assert_eq!(cursor_line(&mut h), line_of(&before, name));
    }
}

#[test]
fn keys_scroll_only_as_far_as_the_cursor_needs() {
    let fx = big();
    let mut h = scrolled(&fx, 3);
    let (x, y) = at(&mut h, "a_20.rs");
    h.click(x, y);
    let before = tree(&mut h);
    // Within the view `j` moves the cursor, not the tree.
    h.keys("j");
    assert_eq!(tree(&mut h), before);
    // Past the bottom edge the tree moves by one row.
    let last = before.iter().rposition(|l| l.contains(".rs")).unwrap();
    let rows_to_edge = last - cursor_line(&mut h);
    h.keys(&"j".repeat(rows_to_edge + 1));
    let after = tree(&mut h);
    assert_eq!(cursor_line(&mut h), last, "{}", after.join("\n"));
    assert_eq!(after[3], before[4], "{}", after.join("\n"));
}

struct FakeAnalyst(Vec<Group>);

impl Analyst for FakeAnalyst {
    fn summarize(&self, _: &SummaryRequest) -> Result<Summary, AnalystError> {
        Ok(Summary::default())
    }

    fn group(&self, _: &GroupRequest) -> Result<Grouping, AnalystError> {
        Ok(Grouping {
            groups: self.0.clone(),
        })
    }
}

#[test]
fn groups_arriving_after_a_click_keep_the_clicked_file_in_place() {
    let fx = big();
    let mut h = scrolled(&fx, 3);
    let (x, y) = at(&mut h, "a_20.rs");
    h.click(x, y);
    let line = cursor_line(&mut h);
    h.session().set_analyst(Arc::new(FakeAnalyst(vec![
        Group {
            name: "Alpha".into(),
            description: "the first".into(),
            mechanical: false,
            files: names("pkg/core/alpha", "a", 30),
        },
        Group {
            name: "Beta".into(),
            description: "the second".into(),
            mechanical: false,
            files: names("pkg/core/beta", "b", 30),
        },
    ])));
    h.session().wait_for_analyst();
    let after = tree(&mut h);
    assert!(after.join("\n").contains("1 Alpha"), "{}", after.join("\n"));
    assert_eq!(cursor_line(&mut h), line, "{}", after.join("\n"));
    assert!(after[line].contains("a_20.rs"), "{}", after.join("\n"));
}

/// The lines of the file picker box, its title first, cursor mark taken out.
fn picker(h: &mut Harness) -> Vec<String> {
    let screen = h.screen();
    let lines: Vec<String> = screen.lines().map(|l| l.replacen('>', " ", 1)).collect();
    let top = lines
        .iter()
        .position(|l| l.contains("Pick a file"))
        .unwrap_or_else(|| panic!("no picker:\n{screen}"));
    // The two bottom rows of the screen are no part of it.
    lines[top..lines.len() - 2].to_vec()
}

#[test]
fn a_click_on_a_directory_in_the_scrolled_picker_keeps_its_row_in_place() {
    let fx = big();
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("<Space>f");
    h.keys(&"j".repeat(38));
    let before = picker(&mut h);
    assert!(
        !before.join("\n").contains("a_00.rs"),
        "{}",
        before.join("\n")
    );
    let line = line_of(&before, "▾ beta");
    assert!(
        line > 3,
        "beta in its place, not pinned:\n{}",
        before.join("\n")
    );
    let (x, y) = at_in(&mut h, "▾ beta", 160);
    h.click(x, y);
    let after = picker(&mut h);
    assert_eq!(line_of(&after, "▸ beta"), line, "{}", after.join("\n"));
    assert_eq!(after[..line], before[..line], "{}", after.join("\n"));
}

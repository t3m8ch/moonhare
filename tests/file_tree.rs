//! The file panel as a directory tree: the sidebar and the file picker draw
//! the files of each Group (or of the whole Range) as directories, chains of
//! single directories joined with `/`, foldable, walked with the usual keys.
mod support;

use std::sync::Arc;

use moonhare::analyst::{
    Analyst, AnalystError, Group, GroupRequest, Grouping, Summary, SummaryRequest,
};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use support::*;

const API: &str = "src/features/course/features/access/api";

/// README.md, two files deep under `API` and one beside the inner chain.
fn deep_branch() -> Fixture {
    let fx = Fixture::new();
    fx.write("README.md", "# demo\n").commit("initial");
    fx.branch("feature");
    fx.write("README.md", "# demo\n\nmore\n")
        .write(&format!("{API}/index.ts"), "export {}\n")
        .write(&format!("{API}/reviews.ts"), "export const r = 1;\n")
        .write("src/features/course/model.ts", "export type M = {};\n")
        .commit("course access api");
    fx
}

/// The file panel of the sidebar: its column from the "Files" title down.
fn sidebar(h: &mut Harness, width: usize) -> String {
    h.screen()
        .lines()
        .map(|l| l.chars().take(width).collect::<String>())
        .skip_while(|l| !l.contains("Files ("))
        .collect::<Vec<_>>()
        .join("\n")
}

fn row_of(text: &str, needle: &str) -> String {
    text.lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no row with {needle:?}:\n{text}"))
        .to_string()
}

fn position(text: &str, needle: &str) -> usize {
    text.find(needle)
        .unwrap_or_else(|| panic!("no {needle:?} in:\n{text}"))
}

fn file_bar(h: &mut Harness) -> String {
    row_of(&h.screen(), "· file ")
}

fn at_file(h: &mut Harness, n: usize, of: usize) -> bool {
    file_bar(h).contains(&format!("file {n}/{of}"))
}

/// The column where the name of the row with `needle` starts.
fn column(text: &str, needle: &str) -> usize {
    let row = row_of(text, needle);
    row[..row.find(needle).unwrap()].chars().count()
}

#[test]
fn the_sidebar_draws_the_files_as_a_tree_with_joined_chains() {
    let fx = deep_branch();
    let mut h = Harness::open(&fx, 160, 30);
    let side = sidebar(&mut h, 32);
    assert!(side.contains("src/features/course"), "{side}");
    assert!(side.contains("features/access/api"), "{side}");
    assert!(!side.contains(API), "never the whole path:\n{side}");
    assert!(!side.contains('…'), "{side}");
    // The leaves carry the file name only, deeper than their directory.
    assert!(column(&side, "reviews.ts") > column(&side, "features/access/api"));
    assert!(column(&side, "features/access/api") > column(&side, "src/features/course"));
    assert!(column(&side, "model.ts") < column(&side, "reviews.ts"));
    // Without Groups the order is the one of the plain list.
    assert!(position(&side, "README.md") < position(&side, "src/features"));
    assert!(position(&side, "reviews.ts") < position(&side, "model.ts"));
    assert!(
        row_of(&side, "src/features/course").contains("0/3"),
        "{side}"
    );
    assert!(
        row_of(&side, "features/access/api").contains("0/2"),
        "{side}"
    );
}

#[test]
fn a_narrow_sidebar_splits_a_chain_into_nested_nodes() {
    let fx = deep_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(":sidebar 20<CR>");
    let side = sidebar(&mut h, 20);
    assert!(!side.contains("src/features/course"), "{side}");
    let src = column(&side, "▾ src");
    let features = side
        .lines()
        .skip_while(|l| !l.contains("▾ src"))
        .nth(1)
        .unwrap_or_default()
        .to_string();
    assert!(features.contains("▾ features"), "{side}");
    assert!(column(&features, "▾ features") > src, "{side}");
    let course = side
        .lines()
        .find(|l| l.contains("▾ course"))
        .unwrap_or_else(|| panic!("{side}"));
    assert!(column(course, "▾ course") > column(&features, "▾ features"));

    // Widening it again joins the chain back.
    h.keys(":sidebar<CR>");
    assert!(sidebar(&mut h, 32).contains("src/features/course"));
}

#[test]
fn a_resize_rebuilds_the_tree_for_the_new_width() {
    let fx = deep_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(":sidebar 20<CR>");
    assert!(!sidebar(&mut h, 20).contains("src/features/course"));
    h.keys("<C-w>>").keys("<C-w>>").keys("<C-w>>");
    assert!(sidebar(&mut h, 32).contains("src/features/course"));
}

#[test]
fn jk_walk_the_visible_rows_and_enter_opens_a_file() {
    let fx = deep_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("<Tab><Tab>");
    assert!(at_file(&mut h, 1, 4), "{}", h.screen());
    // Onto the two directory rows: the open file stays.
    h.keys("j");
    assert!(row_of(&sidebar(&mut h, 32), "src/features/course").starts_with("│>"));
    assert!(at_file(&mut h, 1, 4));
    h.keys("j");
    assert!(row_of(&sidebar(&mut h, 32), "features/access/api").starts_with("│>"));
    h.keys("j");
    assert!(at_file(&mut h, 2, 4), "{}", h.screen());
    assert!(file_bar(&mut h).contains("index.ts"));
    h.session().handle(key(KeyCode::Down));
    assert!(at_file(&mut h, 3, 4), "{}", h.screen());
    h.session().handle(key(KeyCode::Up));
    h.keys("k");
    assert!(row_of(&sidebar(&mut h, 32), "features/access/api").starts_with("│>"));
    h.keys("G");
    assert!(at_file(&mut h, 4, 4));
    h.keys("gg");
    assert!(at_file(&mut h, 1, 4));
    h.keys("jjj<CR>");
    assert!(at_file(&mut h, 2, 4));
    // The focus is in the diff now: j moves the diff, not the file.
    h.keys("j");
    assert!(at_file(&mut h, 2, 4));
}

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn h_l_and_enter_fold_a_directory() {
    let fx = deep_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("<Tab><Tab>jj");
    h.keys("h");
    let side = sidebar(&mut h, 32);
    assert!(!side.contains("reviews.ts"), "{side}");
    assert!(row_of(&side, "features/access/api").contains('▸'), "{side}");
    assert!(side.contains("model.ts"), "{side}");
    h.keys("l");
    assert!(sidebar(&mut h, 32).contains("reviews.ts"));
    h.keys("<CR>");
    assert!(!sidebar(&mut h, 32).contains("reviews.ts"));
    h.keys("<CR>");
    assert!(sidebar(&mut h, 32).contains("reviews.ts"));
    // `h` on a file folds the directory it is in and stands on it.
    h.keys("jh");
    let side = sidebar(&mut h, 32);
    assert!(!side.contains("index.ts"), "{side}");
    assert!(
        row_of(&side, "features/access/api").starts_with("│>"),
        "{side}"
    );
    // Folding the outer one hides everything under it.
    h.keys("kh");
    let side = sidebar(&mut h, 32);
    assert!(
        !side.contains("model.ts") && !side.contains("access"),
        "{side}"
    );
    assert!(side.contains("README.md"), "{side}");
}

#[test]
fn bracket_f_counts_every_file_and_opens_a_folded_directory() {
    let fx = deep_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("<Tab><Tab>jh");
    assert!(!sidebar(&mut h, 32).contains("index.ts"));
    h.keys("<Tab>]f");
    assert!(at_file(&mut h, 2, 4), "{}", h.screen());
    let side = sidebar(&mut h, 32);
    assert!(
        side.contains("index.ts") && side.contains("model.ts"),
        "{side}"
    );
    h.keys("]f]f");
    assert!(at_file(&mut h, 4, 4));
    assert!(file_bar(&mut h).contains("model.ts"));
}

#[test]
fn a_directory_shows_the_progress_of_its_files() {
    let fx = deep_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]fm");
    let side = sidebar(&mut h, 32);
    assert!(
        row_of(&side, "src/features/course").contains("1/3"),
        "{side}"
    );
    assert!(
        row_of(&side, "features/access/api").contains("1/2"),
        "{side}"
    );
    assert!(row_of(&side, "index.ts").contains('✓'), "{side}");
}

#[test]
fn clicks_fold_a_directory_and_open_a_file() {
    let fx = deep_branch();
    let mut h = Harness::open(&fx, 160, 30);
    let (x, y) = h.frame().find("features/access/api").expect("dir row");
    h.click(x, y);
    assert!(
        !sidebar(&mut h, 32).contains("reviews.ts"),
        "{}",
        h.screen()
    );
    assert!(at_file(&mut h, 1, 4));
    h.click(x, y);
    let (x, y) = h.frame().find("reviews.ts").expect("file row");
    h.click(x, y);
    assert!(at_file(&mut h, 3, 4), "{}", h.screen());
}

/// The file picker box, from its title down.
fn picker(h: &mut Harness) -> String {
    let screen = h.screen();
    let lines: Vec<&str> = screen.lines().collect();
    let top = lines
        .iter()
        .position(|l| l.contains("Pick a file"))
        .unwrap_or_else(|| panic!("no picker:\n{screen}"));
    lines[top..].join("\n")
}

#[test]
fn the_file_picker_is_wide_and_draws_the_tree() {
    let fx = deep_branch();
    let mut h = Harness::open(&fx, 120, 40);
    h.keys("<Space>f");
    let frame = h.frame();
    let (title_x, title_y) = frame.find("Pick a file").expect("picker");
    let top = row_of(&frame.text(), "Pick a file");
    let box_width = top.trim().chars().count();
    assert!(box_width >= 100, "{box_width}:\n{}", frame.text());
    assert!(title_x <= 10, "{title_x}");
    let border = title_x as usize - 2;
    let rows = frame
        .text()
        .lines()
        .skip(title_y as usize + 1)
        .take_while(|l| l.chars().nth(border) == Some('│'))
        .count();
    assert!(rows >= 30, "{rows}:\n{}", frame.text());
    let box_ = picker(&mut h);
    assert!(
        box_.contains("src/features/course/features/access/api")
            || box_.contains("src/features/course")
    );
    assert!(position(&box_, "README.md") < position(&box_, "reviews.ts"));
    assert!(column(&box_, "reviews.ts") > column(&box_, "README.md"));
}

#[test]
fn the_file_picker_folds_walks_and_opens() {
    let fx = deep_branch();
    let mut h = Harness::open(&fx, 120, 40);
    // README.md, the course directory, the api directory, then index.ts.
    h.keys("<Space>fjj");
    h.keys("h");
    assert!(!picker(&mut h).contains("reviews.ts"), "{}", h.screen());
    h.keys("l");
    assert!(picker(&mut h).contains("reviews.ts"));
    h.keys("jj<CR>");
    let screen = h.screen();
    assert!(!screen.contains("Pick a file"), "{screen}");
    assert!(at_file(&mut h, 3, 4), "{screen}");
}

#[test]
fn clicking_a_directory_in_the_picker_folds_it() {
    let fx = deep_branch();
    let mut h = Harness::open(&fx, 120, 40);
    h.keys("<Space>f");
    let (x, y) = h.frame().find("features/access/api").expect("dir row");
    h.click(x, y);
    let box_ = picker(&mut h);
    assert!(!box_.contains("reviews.ts"), "{box_}");
    let (x, y) = h.frame().find("model.ts").expect("file row");
    h.click(x, y);
    assert!(!h.screen().contains("Pick a file"));
    assert!(at_file(&mut h, 4, 4));
}

// ------------------------------------------------------------ Groups

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

const LONG: &str = "Access to the course: the API that lists the reviews and \
                    the index that exports it, read before the model because the \
                    model only names the types the API hands out. The end.";

fn grouped(fx: &Fixture, width: u16, height: u16) -> Harness {
    let mut h = Harness::open(fx, width, height);
    h.session().set_analyst(Arc::new(FakeAnalyst(vec![
        Group {
            name: "Access".into(),
            description: LONG.into(),
            mechanical: false,
            // Reading order: reviews before index, the model last.
            files: vec![
                format!("{API}/reviews.ts"),
                "src/features/course/model.ts".into(),
                format!("{API}/index.ts"),
            ],
        },
        Group {
            name: "Docs".into(),
            description: "the readme".into(),
            mechanical: false,
            files: vec!["README.md".into()],
        },
    ])));
    h.session().wait_for_analyst();
    h
}

#[test]
fn within_a_group_the_tree_keeps_the_reading_order_of_the_analyst() {
    let fx = deep_branch();
    let mut h = grouped(&fx, 160, 30);
    let side = sidebar(&mut h, 32);
    assert!(position(&side, "Access") < position(&side, "src/features"));
    assert!(
        position(&side, "reviews.ts") < position(&side, "index.ts"),
        "{side}"
    );
    assert!(
        position(&side, "index.ts") < position(&side, "model.ts"),
        "{side}"
    );
    assert!(
        position(&side, "model.ts") < position(&side, "Docs"),
        "{side}"
    );
    // ]f walks in the order of the tree.
    h.keys("gg[f[f[f[f");
    assert!(file_bar(&mut h).contains("reviews.ts"), "{}", h.screen());
    h.keys("]f");
    assert!(file_bar(&mut h).contains("index.ts"), "{}", h.screen());
}

#[test]
fn the_description_of_a_group_is_hidden_until_asked_for() {
    let fx = deep_branch();
    let mut h = grouped(&fx, 160, 30);
    let screen = h.screen();
    assert!(!screen.contains("Access to the course"), "{screen}");

    // In the files window the hint line names the Group under the cursor.
    h.keys("<Tab><Tab>gg");
    let hints = h.screen().lines().last().unwrap().to_string();
    assert!(
        hints.contains("Access") && hints.contains("Access to the course"),
        "{hints}"
    );
    assert!(hints.contains("Space i"), "{hints}");
    h.keys("G");
    let hints = h.screen().lines().last().unwrap().to_string();
    assert!(hints.contains("the readme"), "{hints}");

    // Space i shows it whole; again, or Esc, hides it.
    h.keys("gg<Space>i");
    let screen = h.screen();
    assert!(screen.contains("The end."), "{screen}");
    h.keys("<Space>i");
    assert!(!h.screen().contains("The end."));
    h.keys("<Space>i<Esc>");
    assert!(!h.screen().contains("The end."));
}

#[test]
fn the_description_key_is_in_the_help_and_which_key() {
    let fx = deep_branch();
    let mut h = grouped(&fx, 160, 60);
    h.keys("<Space>").wait(400);
    let screen = h.screen();
    assert!(row_of(&screen, " i ").contains("описание"), "{screen}");
    h.keys("<Esc>?");
    assert!(h.screen().contains("Space i"), "{}", h.screen());
}

#[test]
fn the_picker_shows_the_description_of_the_group_under_its_cursor() {
    let fx = deep_branch();
    let mut h = grouped(&fx, 120, 40);
    h.keys("<Space>f");
    let box_ = picker(&mut h);
    assert!(!box_.contains("Access to the course"), "{box_}");
    let hints = h.screen().lines().last().unwrap().to_string();
    assert!(hints.contains("the readme"), "on README.md: {hints}");
    h.keys("gg");
    let hints = h.screen().lines().last().unwrap().to_string();
    assert!(hints.contains("Access to the course"), "{hints}");
    h.keys("<Space>i");
    assert!(h.screen().contains("The end."), "{}", h.screen());
    h.keys("<Esc>");
    assert!(h.screen().contains("Pick a file"), "the picker stays");
}

#[test]
fn clicking_a_group_header_opens_its_first_file() {
    let fx = deep_branch();
    let mut h = grouped(&fx, 160, 30);
    let (x, y) = h.frame().find("1 Access").expect("header");
    h.click(x, y);
    assert!(file_bar(&mut h).contains("reviews.ts"), "{}", h.screen());
}

//! Pinned rows of the file tree: while a long tree scrolls, the Group header
//! and the directories above the first row stay at the top of the panel (and
//! of the file picker). And the `z` keys of the tree: `zM` folds every
//! directory, `zR` unfolds them, `za`/`zc`/`zo` the one under the cursor.
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

/// A branch that adds `paths`, one line each.
fn branch_with(paths: &[String]) -> Fixture {
    let fx = Fixture::new();
    fx.write("README.md", "x\n").commit("initial");
    fx.branch("feature");
    for path in paths {
        fx.write(path, &format!("{path}\n"));
    }
    fx.commit("files");
    fx
}

/// `pkg/core/alpha` and `pkg/core/beta`, thirty files each.
fn big_paths() -> Vec<String> {
    let mut paths = names("pkg/core/alpha", "a", 30);
    paths.extend(names("pkg/core/beta", "b", 30));
    paths
}

fn big() -> Fixture {
    branch_with(&big_paths())
}

/// The rows of the file panel of the sidebar, the title row first.
fn sidebar(h: &mut Harness) -> Vec<String> {
    h.screen()
        .lines()
        .map(|l| l.chars().take(32).collect::<String>())
        .skip_while(|l| !l.contains("Files ("))
        .collect()
}

fn cursor_row(rows: &[String]) -> usize {
    rows.iter()
        .position(|l| l.starts_with("│>"))
        .unwrap_or_else(|| panic!("no cursor:\n{}", rows.join("\n")))
}

fn text(rows: &[String]) -> String {
    rows.join("\n")
}

fn file_bar(h: &mut Harness) -> String {
    h.screen()
        .lines()
        .find(|l| l.contains("· file "))
        .unwrap_or_else(|| panic!("no file bar"))
        .to_string()
}

#[test]
fn deep_in_a_directory_its_chain_is_pinned_at_the_top() {
    let fx = big();
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("<Tab><Tab>");
    h.keys(&"j".repeat(28));
    let side = sidebar(&mut h);
    assert!(!text(&side).contains("a_00.rs"), "{}", text(&side));
    assert!(side[1].contains("▾ pkg/core"), "{}", text(&side));
    assert!(side[2].contains("▾ alpha"), "{}", text(&side));
    assert!(side[3].contains("a_"), "{}", text(&side));
    // The pinned rows keep the indent and the progress of the tree.
    assert!(
        side[2].find("alpha") > side[1].find("pkg"),
        "{}",
        text(&side)
    );
    assert!(side[1].contains("0/60"), "{}", text(&side));
    assert!(side[2].contains("0/30"), "{}", text(&side));
    // The cursor is never under them.
    assert!(cursor_row(&side) > 2, "{}", text(&side));
    assert!(side[cursor_row(&side)].contains("a_28.rs"));
}

#[test]
fn the_next_directory_pushes_the_pinned_one_out() {
    let fx = big();
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("<Tab><Tab>");
    let mut pushed = false;
    let mut beta_pinned = false;
    for _ in 0..62 {
        h.keys("j");
        let side = sidebar(&mut h);
        let all = text(&side);
        let cursor = cursor_row(&side);
        // Neither the cursor nor a row of the tree hides under the pins.
        assert!(cursor > 0, "{all}");
        if side[1].contains("pkg/core") && !all.contains("a_00.rs") {
            // A pinned directory is followed by what is in it, or by the
            // next directory pushing it out.
            if side[2].contains("alpha") {
                assert!(
                    side[3].contains("a_") || side[3].contains("▾ beta"),
                    "{all}"
                );
                pushed |= side[3].contains("▾ beta");
            }
            if side[2].contains("▾ beta") {
                assert!(!all.contains("alpha"), "{all}");
                beta_pinned |= side[3].contains("b_");
            }
        }
    }
    assert!(pushed, "beta never pushed alpha out");
    assert!(beta_pinned, "beta was never pinned");
    let side = sidebar(&mut h);
    assert!(
        side[cursor_row(&side)].contains("b_29.rs"),
        "{}",
        text(&side)
    );
}

#[test]
fn at_the_top_of_the_tree_nothing_is_pinned() {
    let fx = big();
    let mut h = Harness::open(&fx, 160, 40);
    let side = sidebar(&mut h);
    assert!(side[1].contains("pkg/core"), "{}", text(&side));
    assert!(side[2].contains("alpha"), "{}", text(&side));
    assert!(side[3].contains("a_00.rs"), "{}", text(&side));
}

#[test]
fn deep_nesting_pins_the_nearest_directories_and_folds_the_rest() {
    let mut paths: Vec<String> = Vec::new();
    let mut dir = String::new();
    for level in 1..=14 {
        if !dir.is_empty() {
            dir.push('/');
        }
        dir.push_str(&format!("lvl{level}"));
        paths.push(format!("{dir}/f{level}.rs"));
    }
    paths.extend(names(&dir, "leaf", 40));
    let fx = branch_with(&paths);
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("<Tab><Tab>G");
    let side = sidebar(&mut h);
    let all = text(&side);
    assert!(side[1].contains('…') && !side[1].contains("lvl"), "{all}");
    assert!(all.contains("▾ lvl14"), "{all}");
    assert!(!all.contains("▾ lvl1 "), "{all}");
    let pinned = side[1..]
        .iter()
        .take_while(|l| !l.contains("leaf_"))
        .count();
    let capacity = side.len().saturating_sub(4);
    assert!(pinned <= capacity / 3 + 1, "{pinned} of {capacity}:\n{all}");
    assert!(side[cursor_row(&side)].contains("leaf_39.rs"), "{all}");
}

#[test]
fn a_narrow_panel_pins_rows_cut_to_its_width() {
    let fx = branch_with(&{
        let mut p = names("pkg/core/a_rather_long_directory_name", "a", 30);
        p.extend(names("pkg/core/beta", "b", 2));
        p
    });
    let mut h = Harness::open(&fx, 160, 40);
    h.keys(":sidebar 18<CR><Tab><Tab>");
    h.keys(&"j".repeat(28));
    let screen = h.screen();
    let side: Vec<String> = screen
        .lines()
        .map(|l| l.chars().take(18).collect::<String>())
        .skip_while(|l| !l.contains("Files"))
        .collect();
    let all = text(&side);
    let pinned: Vec<&String> = side[1..]
        .iter()
        .take_while(|l| !l.contains("· A"))
        .collect();
    assert!(pinned[0].contains("pkg"), "{all}");
    let last = pinned.last().unwrap();
    assert!(last.contains("a_rather") && last.contains('…'), "{all}");
    assert!(cursor_row(&side) > pinned.len(), "{all}");
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

fn grouped(fx: &Fixture, width: u16, height: u16) -> Harness {
    let mut h = Harness::open(fx, width, height);
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
    h
}

#[test]
fn with_groups_the_group_header_is_pinned_above_the_directories() {
    let fx = big();
    let mut h = grouped(&fx, 160, 40);
    h.keys("<Tab><Tab>G");
    let side = sidebar(&mut h);
    let all = text(&side);
    assert!(side[1].contains("2 Beta"), "{all}");
    assert!(side[2].contains("▾ pkg/core/beta"), "{all}");
    assert!(side[3].contains("b_"), "{all}");
    assert!(!all.contains("Alpha"), "{all}");
    assert!(side[cursor_row(&side)].contains("b_29.rs"), "{all}");
}

#[test]
fn clicking_a_pinned_group_header_opens_the_group() {
    let fx = big();
    let mut h = grouped(&fx, 160, 40);
    h.keys("<Tab><Tab>G");
    assert!(file_bar(&mut h).contains("b_29.rs"), "{}", h.screen());
    let side = sidebar(&mut h);
    assert!(side[1].contains("2 Beta"), "{}", text(&side));
    let (x, y) = h.frame().find("2 Beta").expect("the pinned header");
    h.click(x, y);
    assert!(file_bar(&mut h).contains("b_00.rs"), "{}", h.screen());
}

// ------------------------------------------------------------ clicks

#[test]
fn clicking_a_pinned_directory_puts_the_cursor_on_it() {
    let fx = big();
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("<Tab><Tab>G");
    let before = file_bar(&mut h);
    let (x, y) = h.frame().find("▾ beta").expect("the pinned directory");
    h.click(x, y);
    let side = sidebar(&mut h);
    assert!(side[cursor_row(&side)].contains("beta"), "{}", text(&side));
    assert_eq!(file_bar(&mut h), before, "the open file stays");
    let (x, y) = h.frame().find("▾ pkg/core").expect("the root directory");
    h.click(x, y);
    let side = sidebar(&mut h);
    assert!(
        side[cursor_row(&side)].contains("pkg/core"),
        "{}",
        text(&side)
    );
}

// ------------------------------------------------------------ the picker

/// The rows of the file picker box, its title first.
fn picker(h: &mut Harness) -> Vec<String> {
    let screen = h.screen();
    let lines: Vec<String> = screen.lines().map(str::to_string).collect();
    let top = lines
        .iter()
        .position(|l| l.contains("Pick a file"))
        .unwrap_or_else(|| panic!("no picker:\n{screen}"));
    lines[top..].to_vec()
}

#[test]
fn the_picker_pins_the_chain_too_and_a_click_moves_to_it() {
    let fx = big();
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("<Space>fG");
    let rows = picker(&mut h);
    let all = text(&rows);
    assert!(rows[1].contains("▾ pkg/core"), "{all}");
    assert!(rows[2].contains("▾ beta"), "{all}");
    assert!(!all.contains("alpha"), "{all}");
    assert!(
        rows.iter()
            .any(|r| r.contains(">") && r.contains("b_29.rs")),
        "{all}"
    );
    let (x, y) = h.frame().find("▾ beta").expect("the pinned directory");
    h.click(x, y);
    let rows = picker(&mut h);
    assert!(
        rows.iter()
            .any(|r| r.contains(">  ▾ beta") || r.contains("> ▾ beta")),
        "{}",
        text(&rows)
    );
}

// ------------------------------------------------------------ z keys

/// The big tree with a Mechanical lock file.
fn with_mechanical() -> Fixture {
    let mut paths = big_paths();
    paths.push("web/package-lock.json".into());
    paths.push("web/app.js".into());
    branch_with(&paths)
}

#[test]
fn z_shift_m_folds_every_directory_and_z_shift_r_unfolds_them() {
    let fx = with_mechanical();
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("]f]f");
    let before = file_bar(&mut h);
    h.keys("<Tab><Tab>zM");
    let all = text(&sidebar(&mut h));
    assert!(!all.contains('▾'), "{all}");
    assert!(!all.contains(".rs") && !all.contains(".json"), "{all}");
    assert_eq!(all.matches("▸ web").count(), 2, "both web trees:\n{all}");
    assert!(all.contains("▸ pkg/core"), "{all}");
    // The cursor is on the nearest visible directory of the open file.
    let side = sidebar(&mut h);
    assert!(side[cursor_row(&side)].contains("pkg/core"), "{all}");
    assert_eq!(file_bar(&mut h), before, "the open file stays");
    h.keys("zR");
    let all = text(&sidebar(&mut h));
    assert!(!all.contains('▸'), "{all}");
    assert!(all.contains("a_00.rs"), "{all}");
}

#[test]
fn bracket_f_after_z_shift_m_unfolds_the_directory_of_the_file() {
    let fx = big();
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("<Tab><Tab>zM<Tab>]f");
    assert!(file_bar(&mut h).contains("a_01.rs"), "{}", h.screen());
    let all = text(&sidebar(&mut h));
    assert!(all.contains("a_01.rs"), "{all}");
    assert!(all.contains("▾ alpha"), "{all}");
}

#[test]
fn za_zc_zo_fold_the_directory_under_the_cursor() {
    let fx = big();
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("<Tab><Tab>k");
    let side = sidebar(&mut h);
    assert!(side[cursor_row(&side)].contains("alpha"), "{}", text(&side));
    h.keys("zc");
    assert!(!text(&sidebar(&mut h)).contains("a_00.rs"));
    h.keys("zo");
    assert!(text(&sidebar(&mut h)).contains("a_00.rs"));
    h.keys("za");
    assert!(!text(&sidebar(&mut h)).contains("a_00.rs"));
    h.keys("za");
    assert!(text(&sidebar(&mut h)).contains("a_00.rs"));
    // On a file `zc` folds the directory it is in.
    h.keys("jzc");
    let side = sidebar(&mut h);
    assert!(!text(&side).contains("a_00.rs"));
    assert!(side[cursor_row(&side)].contains("alpha"), "{}", text(&side));
}

#[test]
fn z_shift_m_works_in_every_group_and_in_the_picker() {
    let fx = big();
    let mut h = grouped(&fx, 160, 40);
    h.keys("<Space>fzM");
    let rows = picker(&mut h);
    let all = text(&rows);
    assert!(!all.contains('▾') && !all.contains(".rs"), "{all}");
    assert_eq!(all.matches("▸ pkg/core").count(), 2, "{all}");
    assert!(
        rows.iter()
            .any(|r| r.contains('>') && r.contains("pkg/core/alpha")),
        "{all}"
    );
    h.keys("zR");
    assert!(text(&picker(&mut h)).contains("a_00.rs"));
    h.keys("<Esc>");
    h.keys("<Tab><Tab>zM");
    let all = text(&sidebar(&mut h));
    assert_eq!(all.matches("▸ pkg/core").count(), 2, "{all}");
}

#[test]
fn z_is_the_tree_s_prefix_in_the_file_panel_and_rounds_in_the_commits() {
    let fx = big();
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("<Tab><Tab>z").wait(1000);
    let screen = h.screen();
    assert!(screen.contains("свернуть все каталоги"), "{screen}");
    assert!(!screen.contains("раунд"), "{screen}");
    h.keys("<Esc><Tab><Tab>z").wait(1000);
    let screen = h.screen();
    assert!(screen.contains("раунд"), "{screen}");
    assert!(!screen.contains("свернуть все каталоги"), "{screen}");
    let help = moonhare::hints::help_lines().join("\n");
    assert!(help.contains("zM ") && help.contains("zR "), "{help}");
}

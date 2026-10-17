//! Leader, which-key, hints and help.
mod support;
use support::*;

#[test]
fn space_pause_opens_which_key_listing_continuations() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 30);
    h.keys("<Space>").wait(350);
    let screen = h.screen();
    assert!(screen.contains("which-key"), "{screen}");
    assert!(screen.contains("выбор коммита"), "{screen}");
}

#[test]
fn which_key_and_help_list_the_sidebar_and_file_picker_keys() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 60);
    h.keys("<Space>").wait(350);
    let screen = h.screen();
    assert!(screen.contains("сайдбар"), "{screen}");
    assert!(screen.contains("выбор файла"), "{screen}");
    h.keys("<Esc>?");
    let screen = h.screen();
    assert!(screen.contains("Space e"), "{screen}");
    assert!(screen.contains("Space f"), "{screen}");
}

#[test]
fn help_lists_the_reviewed_marks() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 60);
    h.keys("?");
    let screen = h.screen();
    assert!(screen.contains("отревьюено"), "{screen}");
    assert!(
        screen
            .lines()
            .any(|l| l.contains(" m   ") && l.contains("отревьюено")),
        "{screen}"
    );
    assert!(
        screen
            .lines()
            .any(|l| l.contains(" M   ") && l.contains("следующему")),
        "{screen}"
    );
}

#[test]
fn which_key_and_help_list_unreviewed_jumps_and_reset() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 90);
    h.keys("]").wait(350);
    assert!(h.screen().contains("неотревьюенный"), "{}", h.screen());
    h.keys("<Esc>[").wait(350);
    assert!(h.screen().contains("неотревьюенный"));
    h.keys("<Esc>?");
    let screen = h.screen();
    assert!(screen.contains("]u"), "{screen}");
    assert!(screen.contains(":reset"), "{screen}");
}

#[test]
fn help_lists_the_diagram_command_and_the_overlay_keys() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 80);
    h.keys("?");
    let screen = h.screen();
    assert!(screen.contains(":diagram"), "{screen}");
    assert!(screen.contains("диаграмм"), "{screen}");
}

#[test]
fn which_key_and_help_list_the_unreviewed_filter() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 60);
    h.keys("<Space>").wait(350);
    let screen = h.screen();
    assert!(screen.contains("непросмотренн"), "{screen}");
    h.keys("<Esc>?");
    assert!(h.screen().contains("Space u"), "{}", h.screen());
}

#[test]
fn the_visual_badge_shows_only_while_the_commit_list_has_the_focus() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("<Tab>jv");
    assert!(h.screen().contains("VISUAL"));
    h.keys("<Space>e");
    let screen = h.screen();
    assert!(!screen.contains("VISUAL"), "{screen}");
}

#[test]
fn which_key_and_help_list_the_analyst_summary() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 60);
    h.keys("<Space>").wait(350);
    assert!(h.screen().contains("саммари"), "{}", h.screen());
    h.keys("<Esc>?");
    assert!(h.screen().contains("Space s"), "{}", h.screen());
}

#[test]
fn which_key_and_help_list_the_groups_toggle() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 60);
    h.keys("<Space>").wait(350);
    assert!(h.screen().contains("группы / дерево"), "{}", h.screen());
    h.keys("<Esc>?");
    assert!(h.screen().contains("Space g"), "{}", h.screen());
}

#[test]
fn which_key_and_help_list_the_analyst_explanations() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 90);
    h.keys("<Space>").wait(350);
    assert!(h.screen().contains("объяснить хунк"), "{}", h.screen());
    h.keys("<Esc>?");
    let screen = h.screen();
    assert!(screen.contains("Space x"), "{screen}");
    assert!(screen.contains("Space X"), "{screen}");
}

#[test]
fn help_lists_enter_for_collapsed_mechanical_files() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 120, 60);
    h.keys("?");
    let screen = h.screen();
    assert!(
        screen
            .lines()
            .any(|l| l.contains("развернуть свёрнутый механический файл")),
        "{screen}"
    );
}

#[test]
fn no_which_key_before_the_pause() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 30);
    h.keys("<Space>").wait(200);
    assert!(!h.screen().contains("which-key"));
    h.wait(150);
    assert!(h.screen().contains("which-key"));
}

#[test]
fn escape_cancels_which_key_and_the_prefix() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 30);
    h.keys("<Space>").wait(350).keys("<Esc>");
    assert!(!h.screen().contains("which-key"));
    h.keys("c").wait(350);
    let screen = h.screen();
    assert!(!screen.contains("Pick a Scope"), "{screen}");
}

#[test]
fn g_bracket_prefixes_open_which_key_with_their_continuations() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 30);
    h.keys("g").wait(350);
    let screen = h.screen();
    assert!(screen.contains("which-key: g"), "{screen}");
    h.keys("<Esc>").keys("]").wait(350);
    let screen = h.screen();
    assert!(screen.contains("which-key: ]"), "{screen}");
    assert!(screen.contains("следующий хунк"), "{screen}");
    h.keys("<Esc>").keys("[").wait(350);
    assert!(h.screen().contains("предыдущий файл"));
}

/// The Scope after `keys` and a `jj<CR>` aimed at the focused window.
fn scope_after(keys: &str) -> String {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 30);
    h.keys(keys).keys("jj<CR>");
    let screen = h.screen();
    screen
        .split_whitespace()
        .find(|w| w.starts_with("scope:"))
        .unwrap_or_else(|| panic!("no scope in status:\n{screen}"))
        .to_string()
}

#[test]
fn ctrl_w_moves_between_windows() {
    assert_eq!(scope_after("<C-w>h"), "scope:2", "to the commit list");
    assert_eq!(scope_after("<C-w>h<C-w>l"), "scope:all", "back to the diff");
    assert_eq!(scope_after("<C-w>k"), "scope:2");
    assert_eq!(scope_after("<C-w>w"), "scope:2");
}

#[test]
fn tab_and_ctrl_hjkl_aliases_move_between_windows() {
    assert_eq!(scope_after("<Tab>"), "scope:2");
    assert_eq!(scope_after("<Tab><Tab><Tab>"), "scope:all");
    assert_eq!(scope_after("<C-h>"), "scope:2");
    assert_eq!(scope_after("<C-h><C-l>"), "scope:all");
    assert_eq!(scope_after("<C-k>"), "scope:2");
    assert_eq!(scope_after("<C-k><C-l>"), "scope:all");
}

#[test]
fn a_hidden_sidebar_never_gets_the_focus() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 80, 30);
    for keys in ["<Tab>", "<C-w>h", "<C-w>k", "<C-w>w", "<C-h>", "<C-k>"] {
        h.keys(keys).keys("jj<CR>");
        let screen = h.screen();
        assert!(screen.contains("scope:all"), "{keys}:\n{screen}");
        let row = hint_row(&mut h);
        assert!(!row.contains("Enter: срез"), "{keys}: {row}");
    }
}

#[test]
fn hiding_the_sidebar_returns_the_focus_to_the_diff() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("<Tab><Space>e");
    let row = hint_row(&mut h);
    assert!(!row.contains("Enter: срез"), "{row}");
    h.keys("jj<CR>");
    assert!(h.screen().contains("scope:all"));
}

#[test]
fn pickers_leave_the_status_and_hint_rows_alone() {
    let fx = support::Fixture::new();
    fx.write("README.md", "x\n").commit("initial");
    fx.branch("feature");
    for i in 0..30 {
        fx.write(&format!("f{i:02}.txt"), "x\n")
            .commit(&format!("add {i}"));
    }
    for keys in ["<Space>c", "<Space>f"] {
        let mut h = Harness::open(&fx, 100, 16);
        h.keys(keys);
        let screen = h.screen();
        let rows: Vec<&str> = screen.lines().collect();
        assert!(rows[rows.len() - 2].contains("scope:"), "{keys}:\n{screen}");
        assert!(rows[rows.len() - 1].contains("Space"), "{keys}:\n{screen}");
    }
}

#[test]
fn plain_hjkl_never_switch_windows() {
    assert_eq!(scope_after("hkl"), "scope:all", "from the diff");
    assert_eq!(scope_after("<Tab>hl"), "scope:2", "from the commit list");
}

fn hint_row(h: &mut Harness) -> String {
    let screen = h.screen();
    screen.lines().last().unwrap_or_default().to_string()
}

#[test]
fn hint_line_names_chains_in_the_diff_window() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 120, 30);
    let row = hint_row(&mut h);
    assert!(row.contains("Ctrl-w + h/j/k/l: окна"), "{row}");
    assert!(row.contains("?: справка"), "{row}");
}

#[test]
fn hint_line_follows_the_window_and_mode() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 120, 30);
    h.keys("<Tab>");
    let row = hint_row(&mut h);
    assert!(row.contains("Enter: срез"), "{row}");
    assert!(!row.contains("]h"), "{row}");

    h.keys("jv");
    let row = hint_row(&mut h);
    assert!(row.contains("VISUAL"), "{row}");
    assert!(row.contains("Esc: отмена"), "{row}");

    h.keys("<Esc><Tab>:");
    let row = hint_row(&mut h);
    assert!(
        row.starts_with(":"),
        "command line owns the bottom row: {row}"
    );
}

#[test]
fn hint_line_labels_a_pending_chain() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 120, 30);
    h.keys("]");
    let row = hint_row(&mut h);
    assert!(row.contains("] + h: следующий хунк"), "{row}");
}

#[test]
fn question_mark_shows_help_with_all_keys_and_zellij_caveat() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 80);
    h.keys("?");
    let screen = h.screen();
    for expected in [
        "Справка",
        "Space c",
        "Ctrl-w h",
        "]h",
        "Tab",
        "Ctrl-h/j/k/l",
        "zellij",
    ] {
        assert!(screen.contains(expected), "{expected} missing:\n{screen}");
    }
    h.keys("<Esc>");
    assert!(!h.screen().contains("Справка"));
    h.keys("?q");
    assert!(!h.screen().contains("Справка"), "q closes help too");
    assert!(!h.quit_requested());
}

#[test]
fn help_warns_that_ctrl_h_often_arrives_as_backspace() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 60);
    h.keys("?");
    assert!(h.screen().contains("Backspace"));
}

#[test]
fn the_mouse_does_nothing_under_the_help() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("?").click(5, 8);
    h.keys("<Esc>");
    let screen = h.screen();
    assert!(screen.contains("file 1/3"), "{screen}");
}

#[test]
fn which_key_and_help_fit_the_smallest_terminal() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 60, 15);
    for prefix in ["<Space>", "g", "]", "[", "<C-w>"] {
        h.keys(prefix).wait(350);
        let screen = h.screen();
        assert_eq!(screen.lines().count(), 15, "{prefix}:\n{screen}");
        h.keys("<Esc>");
    }
    h.keys("?jjjjj");
    assert_eq!(h.screen().lines().count(), 15);
}

#[test]
fn next_tick_is_due_only_while_a_prefix_waits_for_the_pause() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 30);
    assert_eq!(h.next_tick_in(), None);
    h.keys("<Space>");
    assert_eq!(
        h.next_tick_in(),
        Some(std::time::Duration::from_millis(300))
    );
    h.wait(120);
    assert_eq!(
        h.next_tick_in(),
        Some(std::time::Duration::from_millis(180))
    );
    h.wait(200);
    assert_eq!(h.next_tick_in(), None, "the window is open already");
}

#[test]
fn help_scrolls_on_a_short_terminal() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 20);
    h.keys("?");
    assert!(!h.screen().contains("Ctrl-w W"));
    h.keys(&"j".repeat(80));
    assert!(h.screen().contains("Ctrl-w W"));
}

#[test]
fn ctrl_w_opens_which_key() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 30);
    h.keys("<C-w>").wait(350);
    let screen = h.screen();
    assert!(screen.contains("which-key: Ctrl-w"), "{screen}");
    assert!(screen.contains("окно справа"), "{screen}");
}

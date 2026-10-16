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
    assert_eq!(scope_after("<Tab><Tab>"), "scope:all");
    assert_eq!(scope_after("<C-h>"), "scope:2");
    assert_eq!(scope_after("<C-h><C-l>"), "scope:all");
    assert_eq!(scope_after("<C-k>"), "scope:2");
    assert_eq!(scope_after("<C-k><C-j>"), "scope:all");
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
    let mut h = Harness::open(&fx, 100, 60);
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
fn help_scrolls_on_a_short_terminal() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 20);
    h.keys("?");
    assert!(!h.screen().contains("Ctrl-w w"));
    h.keys("jjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjj");
    assert!(h.screen().contains("Ctrl-w w"));
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

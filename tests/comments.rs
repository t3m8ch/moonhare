//! Comments: three kinds, the in-app editor, states, hidden-Comments cue and
//! persistence between runs.

mod support;

use support::{feature_branch, Harness};

/// Index of the first screen row containing `needle`.
fn row_of(screen: &str, needle: &str) -> usize {
    screen
        .lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no row with {needle:?}:\n{screen}"))
}

#[test]
fn c_opens_an_editor_and_a_saved_line_comment_hangs_under_its_line() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    // README.md: "# demo", "", "more".
    h.keys("jjcifix this<Esc><CR>");
    let screen = h.screen();
    let code = row_of(&screen, "more");
    let comment = row_of(&screen, "fix this");
    assert!(
        comment > code && comment <= code + 3,
        "comment is not under its line:\n{screen}"
    );
    assert!(screen.contains("NORMAL"), "{screen}");
}

#[test]
fn a_second_esc_with_text_asks_before_losing_the_draft() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("jjcidraft text<Esc><Esc>");
    let screen = h.screen();
    assert!(screen.contains("Discard"), "{screen}");
    assert!(screen.contains("draft text"), "{screen}");

    // `n` goes back to the editor with the text intact.
    h.keys("n");
    assert_eq!(badge(&mut h), "COMMENT");
    assert!(h.screen().contains("draft text"));

    // `y` throws the draft away; nothing is saved.
    h.keys("<Esc>y");
    let screen = h.screen();
    assert!(!screen.contains("draft text"), "{screen}");
    assert_eq!(badge(&mut h), "NORMAL");
}

#[test]
fn esc_on_an_empty_draft_cancels_at_once() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("jjc<Esc>");
    assert_eq!(badge(&mut h), "NORMAL");
    h.keys("ci<Esc><Esc>");
    assert_eq!(badge(&mut h), "NORMAL");
}

#[test]
fn c_in_visual_comments_the_whole_selection_under_its_last_line() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("jvj");
    assert_eq!(badge(&mut h), "VISUAL");
    h.keys("cirange note<Esc><CR>");
    let screen = h.screen();
    assert_eq!(badge(&mut h), "NORMAL");
    assert!(screen.contains("new 2-3"), "{screen}");
    let code = row_of(&screen, "more");
    assert!(row_of(&screen, "range note") > code, "{screen}");
}

#[test]
fn capital_c_comments_the_whole_file_in_a_frame_above_it() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("Cithe whole file is off<Esc><CR>");
    let screen = h.screen();
    let note = row_of(&screen, "the whole file is off");
    assert!(note > row_of(&screen, "README.md"), "{screen}");
    // Above the first code line (the header row of the Sidebar shares rows,
    // so look at the diff pane only).
    let first_code = screen
        .lines()
        .position(|l| l.chars().skip(32).collect::<String>().contains("# demo"))
        .unwrap();
    assert!(note < first_code, "{screen}");
}

/// The sidebar part (first 32 columns) of every screen row.
fn sidebar(screen: &str) -> Vec<String> {
    screen
        .lines()
        .map(|l| l.chars().take(32).collect())
        .collect()
}

#[test]
fn comment_command_puts_a_range_comment_in_a_frame_under_the_whole_range_row() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(":comment<CR>");
    assert!(h.screen().contains("Comment on the whole Range"));
    h.keys("iextract retry<Esc><CR>");
    let screen = h.screen();
    let rows = sidebar(&screen);
    let whole = rows.iter().position(|r| r.contains("Весь Range")).unwrap();
    let note = rows
        .iter()
        .position(|r| r.contains("extract retry"))
        .unwrap();
    assert_eq!(note, whole + 2, "{screen}");
    // The commits follow the frame.
    let first = rows.iter().position(|r| r.contains("add a")).unwrap();
    assert!(first > note, "{screen}");
}

#[test]
fn without_the_sidebar_a_clickable_range_counter_sits_in_the_status_line() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(":comment<CR>ione<Esc><CR>:comment<CR>itwo<Esc><CR>");
    assert!(!h.screen().contains("Range ✎"), "{}", h.screen());
    h.keys("<Space>e");
    let screen = h.screen();
    let last = screen.lines().nth(screen.lines().count() - 2).unwrap();
    assert!(last.contains("Range ✎2"), "{screen}");

    let (col, row) = h.frame().find("Range ✎2").unwrap();
    assert_eq!(row as usize, screen.lines().count() - 2);
    h.click(col + 2, row);
    let screen = h.screen();
    assert!(screen.contains("Pick a Scope"), "{screen}");
    // The picker lists the Range Comments under its Range row.
    assert!(screen.contains("one") && screen.contains("two"), "{screen}");
}

/// The diff pane part (from column 32) of every screen row.
fn diff_pane(screen: &str) -> Vec<String> {
    screen
        .lines()
        .map(|l| l.chars().skip(32).collect())
        .collect()
}

#[test]
fn a_comment_on_a_removed_line_belongs_to_the_old_side_and_hangs_under_it() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    // Commit 3 turns `    1` into `    10` in src/a.rs.
    h.keys(":scope 3<CR>sjcigone<Esc><CR>");
    let screen = h.screen();
    assert!(screen.contains("old 2"), "{screen}");
    let rows = diff_pane(&screen);
    let removed = rows
        .iter()
        .position(|r| r.trim_end().ends_with("    1"))
        .unwrap();
    let added = rows
        .iter()
        .position(|r| r.trim_end().ends_with("    10"))
        .unwrap();
    let note = rows.iter().position(|r| r.contains("gone")).unwrap();
    assert!(removed < note && note < added, "{screen}");
}

#[test]
fn x_closes_the_comment_under_the_cursor_and_o_reopens_it() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("jjcicheck<Esc><CR>");
    assert!(h.screen().contains("Comment · open"), "{}", h.screen());
    h.keys("x");
    let screen = h.screen();
    assert!(screen.contains("Comment · closed"), "{screen}");
    assert!(!screen.contains("Comment · open"), "{screen}");
    h.keys("o");
    assert!(h.screen().contains("Comment · open"), "{}", h.screen());
}

#[test]
fn x_and_o_work_on_file_and_range_comments_too() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("Cifile note<Esc><CR>x");
    assert!(h.screen().contains("Comment · closed"), "{}", h.screen());
    h.keys("o:comment<CR>irange note<Esc><CR><Tab>x");
    let screen = h.screen();
    assert!(
        sidebar(&screen).iter().any(|r| r.contains("closed")),
        "{screen}"
    );
}

#[test]
fn comments_that_fell_out_of_the_scope_are_kept_and_counted_as_hidden() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    // src/a.rs is not touched by commit 2.
    h.keys("]fCion a<Esc><CR>");
    assert!(!h.screen().contains("скрытых"), "{}", h.screen());
    h.keys(":scope 2<CR>");
    let screen = h.screen();
    assert!(screen.contains("1 скрытых Comments"), "{screen}");
    assert!(!screen.contains("on a"), "{screen}");

    h.keys(":scope all<CR>]f");
    let screen = h.screen();
    assert!(!screen.contains("скрытых"), "{screen}");
    assert!(screen.contains("on a"), "{screen}");
}

#[test]
fn comments_and_their_states_survive_a_restart() {
    let fx = feature_branch();
    {
        let mut h = Harness::open(&fx, 160, 30);
        h.keys("jjcia line note<Esc><CR>");
        h.keys("Cia file note<Esc><CR>x");
        h.keys(":comment<CR>ia range note<Esc><CR>");
    }
    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    assert!(screen.contains("a line note"), "{screen}");
    assert!(screen.contains("a file note"), "{screen}");
    assert!(screen.contains("a range note"), "{screen}");
    // `x` closed the note on the cursor line; that state was saved too.
    assert!(screen.contains("Comment · open · file"), "{screen}");
    assert!(screen.contains("Comment · closed · new 3"), "{screen}");
}

#[test]
fn a_second_review_does_not_see_comments_of_another_branch() {
    let fx = feature_branch();
    {
        let mut h = Harness::open(&fx, 160, 30);
        h.keys(":comment<CR>ia range note<Esc><CR>");
    }
    fx.branch("other");
    fx.write("src/c.rs", "fn c() {}\n").commit("add c");
    let mut h = Harness::open(&fx, 160, 30);
    assert!(!h.screen().contains("a range note"), "{}", h.screen());
}

#[test]
fn a_comment_follows_its_code_when_lines_move() {
    let fx = feature_branch();
    {
        let mut h = Harness::open(&fx, 160, 30);
        h.keys("jjcimind this<Esc><CR>");
    }
    // A new commit pushes `more` down by three lines.
    fx.write("README.md", "# demo\n\nintro\nintro2\nintro3\nmore\n")
        .commit("intro");
    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    assert!(!screen.contains("скрытых"), "{screen}");
    let pane = diff_pane(&screen);
    let code = pane.iter().position(|r| r.contains(" more")).unwrap();
    let note = pane.iter().position(|r| r.contains("mind this")).unwrap();
    assert_eq!(note, code + 2, "{screen}");
    assert!(screen.contains("new 6"), "{screen}");
}

#[test]
fn a_comment_whose_code_is_gone_is_hidden_not_lost() {
    let fx = feature_branch();
    {
        let mut h = Harness::open(&fx, 160, 30);
        h.keys("jjcimind this<Esc><CR>");
    }
    fx.write("README.md", "# demo\n\nrewritten\n")
        .commit("rewrite");
    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    assert!(screen.contains("1 скрытых Comments"), "{screen}");
    assert!(!screen.contains("mind this"), "{screen}");
}

#[test]
fn an_unsaved_draft_survives_a_resize() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("jjcihalf written");
    h.resize(90, 25);
    assert!(h.screen().contains("half written"), "{}", h.screen());
    h.resize(160, 30);
    assert_eq!(badge(&mut h), "INSERT");
    assert!(h.screen().contains("half written"), "{}", h.screen());
}

#[test]
fn reset_forgets_the_marks_but_never_the_comments() {
    let fx = feature_branch();
    {
        let mut h = Harness::open(&fx, 160, 30);
        h.keys(":comment<CR>ihand written<Esc><CR>m");
        h.keys(":reset<CR>");
        assert!(h.screen().contains("hand written"), "{}", h.screen());
    }
    let mut h = Harness::open(&fx, 160, 30);
    assert!(h.screen().contains("hand written"), "{}", h.screen());
}

#[test]
fn comments_clear_asks_first_and_then_removes_every_comment_for_good() {
    let fx = feature_branch();
    {
        let mut h = Harness::open(&fx, 160, 30);
        h.keys(":comment<CR>iprecious<Esc><CR>");
        h.keys(":comments clear<CR>");
        assert!(h.screen().contains("Delete all"), "{}", h.screen());
        h.keys("n");
        assert!(h.screen().contains("precious"), "{}", h.screen());
        h.keys(":comments clear<CR>y");
        assert!(!h.screen().contains("precious"), "{}", h.screen());
    }
    let mut h = Harness::open(&fx, 160, 30);
    assert!(!h.screen().contains("precious"), "{}", h.screen());
}

/// The mode badge at the start of the status line.
fn badge(h: &mut Harness) -> String {
    let screen = h.screen();
    let last = screen.lines().nth(screen.lines().count() - 2).unwrap().to_string();
    last.split_whitespace().next().unwrap_or("").to_string()
}

#[test]
fn the_status_line_names_the_editor_mode() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("jjc");
    assert_eq!(badge(&mut h), "COMMENT");
    h.keys("i");
    assert_eq!(badge(&mut h), "INSERT");
    h.keys("<Esc>");
    assert_eq!(badge(&mut h), "COMMENT");
    h.keys("<CR>");
    assert_eq!(badge(&mut h), "NORMAL");
}

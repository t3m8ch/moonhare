//! New commits and rewritten history: the tool notices them without
//! rebuilding the screen, applies them on `:reload` / `R`, and keeps the
//! Reviewed marks whose file content did not change.

mod support;

use support::{feature_branch, Harness};

/// Long enough for the tool to look at the branch again.
const LATER: u64 = 5_000;

/// The screen without its status line, which may legitimately change.
fn without_status(h: &mut Harness) -> String {
    h.screen()
        .lines()
        .filter(|l| !l.contains("NORMAL"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_new_commit_is_announced_in_the_status_and_the_screen_stays_as_it_was() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]f");
    let before = without_status(&mut h);
    assert!(!h.screen().contains("new commit"), "{}", h.screen());

    fx.write("src/c.rs", "fn c() {}\n").commit("add c");
    h.wait(LATER);

    assert!(h.screen().contains("+1 new commit"), "{}", h.screen());
    assert_eq!(without_status(&mut h), before);
}

/// The sidebar (first 32 columns).
fn sidebar(h: &mut Harness) -> String {
    h.screen()
        .lines()
        .map(|l| l.chars().take(32).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn reload_shows_the_new_commits_and_their_files() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    fx.write("src/c.rs", "fn c() {}\n").commit("add c");
    h.wait(LATER);
    assert!(!sidebar(&mut h).contains("add c"));

    h.keys(":reload<CR>");

    let side = sidebar(&mut h);
    assert!(side.contains("add c"), "{side}");
    assert!(
        side.contains("src/c.rs") && side.contains("Files (4)"),
        "{side}"
    );
    assert!(side.contains("Commits 4"), "{side}");
    assert!(!h.screen().contains("new commit"), "{}", h.screen());
}

#[test]
fn capital_r_reloads_too_and_keeps_the_selected_file() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]f]f");
    fx.write("src/c.rs", "fn c() {}\n").commit("add c");
    h.wait(LATER);

    h.keys("R");

    let side = sidebar(&mut h);
    assert!(
        side.contains("Files (4)") && side.contains("add c"),
        "{side}"
    );
    let cursor = side.lines().find(|l| l.starts_with("│>")).unwrap();
    assert!(cursor.contains("src/b.rs"), "{side}");
}

#[test]
fn reload_without_news_says_so() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(":reload<CR>");
    assert!(h.screen().contains("Nothing to reload"), "{}", h.screen());
}

/// Mark every file of the Range (README.md, src/a.rs, src/b.rs) in `h`.
fn mark_everything(h: &mut Harness) {
    h.keys("m]fm]fm");
    assert!(h.screen().contains("reviewed 3/3"), "{}", h.screen());
}

#[test]
fn an_amend_that_leaves_the_content_alone_keeps_the_marks() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    mark_everything(&mut h);

    fx.git(&["commit", "-q", "--amend", "-m", "reword a"]);
    h.wait(LATER);
    h.keys(":reload<CR>");

    let screen = h.screen();
    assert!(screen.contains("reword a"), "{screen}");
    assert!(screen.contains("reviewed 3/3"), "{screen}");
}

#[test]
fn squashing_the_commits_keeps_the_marks_of_files_whose_final_content_is_the_same() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    mark_everything(&mut h);

    fx.git(&["reset", "-q", "--soft", "main"]);
    fx.git(&["commit", "-q", "-m", "squashed"]);
    h.wait(LATER);
    h.keys(":reload<CR>");

    let screen = h.screen();
    assert!(screen.contains("squashed"), "{screen}");
    assert!(screen.contains("Commits 1"), "{screen}");
    assert!(screen.contains("reviewed 3/3"), "{screen}");
}

/// The diff pane only (right of the sidebar).
fn diff_pane(h: &mut Harness) -> String {
    h.screen()
        .lines()
        .map(|l| l.chars().skip(33).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_file_changed_by_an_amend_returns_to_unreviewed_with_the_delta_from_the_reviewed_version() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    mark_everything(&mut h);

    // The last commit now makes `a` return 20 where the Reviewer saw 10.
    fx.write("src/a.rs", "fn a() {\n    20\n}\n");
    fx.git(&["commit", "-q", "-a", "--amend", "--no-edit"]);
    h.wait(LATER);
    h.keys(":reload<CR>");
    let side = sidebar(&mut h);
    assert!(side.contains("reviewed 2/3"), "{}", h.screen());

    h.keys(" u");
    let side = sidebar(&mut h);
    assert!(
        side.contains("Files (1)") && side.contains("src/a.rs"),
        "{side}"
    );
    let pane = diff_pane(&mut h);
    assert!(pane.contains("modified"), "{pane}");
    assert!(pane.contains("10") && pane.contains("20"), "{pane}");
}

#[test]
fn a_force_pushed_branch_with_a_new_shape_keeps_the_marks_of_unchanged_files() {
    let fx = feature_branch();
    fx.checkout("main");
    let target = moonhare::git::Target::Branch("feature".into());
    let mut h = Harness::open_target(&fx, &target, 160, 30);
    mark_everything(&mut h);

    // The agent starts over: other commits, other messages, `b` reworked.
    fx.checkout("feature");
    fx.git(&["reset", "-q", "--hard", "main"]);
    fx.write("README.md", "# demo\n\nmore\n").commit("docs");
    fx.write("src/a.rs", "fn a() {\n    10\n}\n")
        .commit("a, finished");
    fx.write("src/b.rs", "fn b() {\n    3\n}\n")
        .commit("b, reworked");
    fx.checkout("main");
    h.wait(LATER);
    assert!(h.screen().contains("rewritten"), "{}", h.screen());
    h.keys(":reload<CR>");

    let screen = h.screen();
    assert!(screen.contains("b, reworked"), "{screen}");
    assert!(screen.contains("reviewed 2/3"), "{screen}");
    h.keys(" u");
    let side = sidebar(&mut h);
    assert!(
        side.contains("Files (1)") && side.contains("src/b.rs"),
        "{side}"
    );
    let pane = diff_pane(&mut h);
    assert!(pane.contains("modified"), "{pane}");
    assert!(pane.contains('2') && pane.contains('3'), "{pane}");
}

#[test]
fn a_squash_that_changes_the_content_is_not_reviewed() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    mark_everything(&mut h);

    fx.write("src/a.rs", "fn a() {\n    99\n}\n");
    fx.git(&["reset", "-q", "--soft", "main"]);
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "squashed"]);
    h.wait(LATER);
    h.keys(":reload<CR>");

    assert!(h.screen().contains("reviewed 2/3"), "{}", h.screen());
}

#[test]
fn an_amend_is_announced_as_a_rewrite_not_as_one_new_commit() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    fx.git(&["commit", "-q", "--amend", "-m", "reword a"]);
    h.wait(LATER);
    let screen = h.screen();
    assert!(screen.contains("rewritten"), "{screen}");
    assert!(!screen.contains("+1 new commit"), "{screen}");
}

#[test]
fn a_switch_of_the_cloned_branch_is_not_taken_for_new_commits() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    fx.checkout("main");
    fx.branch("other");
    fx.write("o.txt", "o\n").commit("other work");
    h.wait(LATER);
    let screen = h.screen();
    assert!(screen.contains("switched to other"), "{screen}");
    assert!(!screen.contains("new commit"), "{screen}");

    h.keys("R");
    let screen = h.screen();
    assert!(screen.contains("add a"), "{screen}");
    assert!(!screen.contains("other work"), "{screen}");
}

#[test]
fn the_branch_is_watched_even_when_input_never_pauses() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    fx.write("src/c.rs", "fn c() {}\n").commit("add c");
    h.keys("j");
    h.session().watch(std::time::Duration::from_secs(1));
    assert!(!h.screen().contains("new commit"), "{}", h.screen());
    h.keys("j");
    h.session().watch(std::time::Duration::from_secs(1));
    assert!(h.screen().contains("+1 new commit"), "{}", h.screen());
}

#[test]
fn an_idle_tick_asks_for_a_redraw_only_when_something_changed() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    let tick = std::time::Duration::from_secs(2);
    assert!(!h.session().elapse(tick));
    assert!(!h.session().elapse(tick));

    fx.write("src/c.rs", "fn c() {}\n").commit("add c");
    assert!(h.session().elapse(tick), "a new commit changes the status");
    assert!(!h.session().elapse(tick), "and only once");
}

#[test]
fn a_line_comment_stays_with_its_code_across_a_reload() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("jjcifix this<Esc><CR>");
    assert!(h.screen().contains("fix this"), "{}", h.screen());

    fx.write("src/c.rs", "fn c() {}\n").commit("add c");
    h.wait(LATER);
    h.keys(":reload<CR>");

    let screen = h.screen();
    assert!(screen.contains("add c"), "{screen}");
    assert!(screen.contains("fix this"), "{screen}");
}

struct ByHand;

impl moonhare::analyst::Analyst for ByHand {
    fn summarize(
        &self,
        _: &moonhare::analyst::SummaryRequest,
    ) -> Result<moonhare::analyst::Summary, moonhare::analyst::AnalystError> {
        Ok(moonhare::analyst::Summary { text: "ok".into() })
    }

    fn group(
        &self,
        _: &moonhare::analyst::GroupRequest,
    ) -> Result<moonhare::analyst::Grouping, moonhare::analyst::AnalystError> {
        let group = |name: &str, files: &[&str]| moonhare::analyst::Group {
            name: name.into(),
            description: "d".into(),
            mechanical: false,
            files: files.iter().map(|f| f.to_string()).collect(),
        };
        Ok(moonhare::analyst::Grouping {
            groups: vec![
                group("Core", &["src/a.rs", "src/b.rs"]),
                group("Docs", &["README.md"]),
            ],
        })
    }
}

#[test]
fn group_progress_survives_a_reload_and_a_rewrite() {
    let fx = feature_branch();
    let mut session = moonhare::session::Session::open(fx.path(), 160, 30).unwrap();
    session.set_analyst(std::sync::Arc::new(ByHand));
    let mut h = Harness::from_session(session);
    h.session().wait_for_analyst();
    h.keys("m");
    let row = |h: &mut Harness, name: &str| {
        sidebar(h)
            .lines()
            .find(|l| l.contains(name))
            .unwrap_or_else(|| panic!("no {name}:\n{}", h.screen()))
            .to_string()
    };
    assert!(row(&mut h, "Docs").contains("1/1"), "{}", h.screen());

    fx.git(&["commit", "-q", "--amend", "-m", "reword a"]);
    h.wait(LATER);
    h.keys(":reload<CR>");

    assert!(row(&mut h, "Docs").contains("1/1"), "{}", h.screen());
    assert!(row(&mut h, "Core").contains("0/2"), "{}", h.screen());
}

#[test]
fn help_lists_reload() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 100, 60);
    h.keys("?");
    let screen = h.screen();
    assert!(
        screen
            .lines()
            .any(|l| l.contains(" R   ") && l.contains("новые коммиты")),
        "{screen}"
    );
    assert!(
        screen
            .lines()
            .any(|l| l.contains(":reload") && l.contains("новые коммиты")),
        "{screen}"
    );
}

#[test]
fn a_mark_that_survived_a_squash_can_still_be_taken_back() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    mark_everything(&mut h);
    fx.git(&["reset", "-q", "--soft", "main"]);
    fx.git(&["commit", "-q", "-m", "squashed"]);
    h.wait(LATER);
    h.keys(":reload<CR>");
    assert!(h.screen().contains("reviewed 3/3"), "{}", h.screen());

    h.keys("[fm");

    let screen = h.screen();
    assert!(screen.contains("reviewed 2/3"), "{screen}");
}

#[test]
fn the_reviewed_version_is_remembered_between_runs() {
    let fx = feature_branch();
    mark_everything(&mut Harness::open(&fx, 160, 30));

    fx.write("src/a.rs", "fn a() {\n    20\n}\n");
    fx.git(&["commit", "-q", "-a", "--amend", "--no-edit"]);
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(" u");

    let pane = diff_pane(&mut h);
    assert!(pane.contains("modified"), "{pane}");
    assert!(pane.contains("10") && pane.contains("20"), "{pane}");
}

#[test]
fn reload_keeps_a_commit_scope_whose_commits_are_still_there_and_drops_one_that_is_not() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(":scope 2<CR>");
    fx.write("src/c.rs", "fn c() {}\n").commit("add c");
    h.wait(LATER);
    h.keys(":reload<CR>");
    assert!(h.screen().contains("Commits 2/4"), "{}", h.screen());

    // History is rewritten from the second commit on.
    fx.git(&["reset", "-q", "--hard", "HEAD~3"]);
    fx.write("src/b.rs", "fn b() {}\n").commit("b again");
    h.wait(LATER);
    h.keys(":reload<CR>");
    assert!(h.screen().contains("Commits 2 "), "{}", h.screen());
}

#[test]
fn a_rebase_onto_a_moved_base_keeps_the_marks_of_untouched_files() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    mark_everything(&mut h);

    fx.checkout("main");
    fx.write("docs.md", "docs\n").commit("docs on main");
    fx.checkout("feature");
    fx.git(&["rebase", "-q", "main"]);
    h.wait(LATER);
    h.keys(":reload<CR>");

    let screen = h.screen();
    assert!(screen.contains("reviewed 3/3"), "{screen}");
    assert!(!screen.contains("docs.md"), "{screen}");
}

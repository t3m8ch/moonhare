//! The review session and the Groups of the Analyst, with a fake standing in
//! for the Analyst.

mod support;

use std::sync::{Arc, Mutex};

use moonhare::analyst::{
    Analyst, AnalystError, Group, GroupRequest, Grouping, Summary, SummaryRequest,
};
use moonhare::session::Session;
use support::{feature_branch, Fixture, Harness};

type Answer = Box<dyn Fn(&GroupRequest) -> Result<Grouping, AnalystError> + Send + Sync>;

/// An Analyst that groups from memory and remembers what it was asked.
struct FakeAnalyst {
    answer: Answer,
    requests: Mutex<Vec<GroupRequest>>,
}

impl FakeAnalyst {
    fn with(
        answer: impl Fn(&GroupRequest) -> Result<Grouping, AnalystError> + Send + Sync + 'static,
    ) -> Arc<FakeAnalyst> {
        Arc::new(FakeAnalyst {
            answer: Box::new(answer),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn grouping(groups: Vec<Group>) -> Arc<FakeAnalyst> {
        Self::with(move |_| {
            Ok(Grouping {
                groups: groups.clone(),
            })
        })
    }

    fn requests(&self) -> Vec<GroupRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Analyst for FakeAnalyst {
    fn summarize(&self, _: &SummaryRequest) -> Result<Summary, AnalystError> {
        Ok(Summary { text: "ok".into() })
    }

    fn group(&self, request: &GroupRequest) -> Result<Grouping, AnalystError> {
        self.requests.lock().unwrap().push(request.clone());
        (self.answer)(request)
    }
}

fn group(name: &str, description: &str, mechanical: bool, files: &[&str]) -> Group {
    Group {
        name: name.into(),
        description: description.into(),
        mechanical,
        files: files.iter().map(|f| f.to_string()).collect(),
    }
}

/// `Core` (the two modules) to read first, then `Docs`.
fn core_then_docs() -> Arc<FakeAnalyst> {
    FakeAnalyst::grouping(vec![
        group("Core", "the modules", false, &["src/a.rs", "src/b.rs"]),
        group("Docs", "the readme", false, &["README.md"]),
    ])
}

fn review_with(fx: &Fixture, analyst: &Arc<FakeAnalyst>) -> Harness {
    let mut session = Session::open(fx.path(), 160, 30).expect("session opens");
    session.set_analyst(analyst.clone());
    Harness::from_session(session)
}

/// The sidebar (first 40 columns) of the screen.
fn sidebar(h: &mut Harness) -> String {
    h.screen()
        .lines()
        .map(|l| l.chars().take(40).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn position(text: &str, needle: &str) -> usize {
    text.find(needle)
        .unwrap_or_else(|| panic!("no {needle:?} in:\n{text}"))
}

#[test]
fn once_the_analysis_is_ready_the_sidebar_shows_groups_in_reading_order_with_progress() {
    let fx = feature_branch();
    let mut h = review_with(&fx, &core_then_docs());
    h.session().wait_for_analyst();
    let side = sidebar(&mut h);
    assert!(
        position(&side, "Core") < position(&side, "src/a.rs"),
        "{side}"
    );
    assert!(
        position(&side, "src/b.rs") < position(&side, "Docs"),
        "{side}"
    );
    assert!(
        position(&side, "Docs") < position(&side, "README.md"),
        "{side}"
    );
    assert!(side.contains("0/2"), "{side}");
    assert!(side.contains("0/1"), "{side}");
    assert!(side.contains("the modules"), "{side}");
}

/// The sidebar line that names `needle`.
fn row_of(h: &mut Harness, needle: &str) -> String {
    sidebar(h)
        .lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no row with {needle:?}:\n{}", h.screen()))
        .to_string()
}

#[test]
fn group_progress_counts_the_reviewed_marks() {
    let fx = feature_branch();
    let mut h = review_with(&fx, &core_then_docs());
    h.session().wait_for_analyst();
    assert!(row_of(&mut h, "Core").contains("0/2"));
    // The cursor stays on README.md, which is now last in reading order.
    h.keys("m");
    assert!(row_of(&mut h, "Docs").contains("1/1"), "{}", h.screen());
    assert!(row_of(&mut h, "Core").contains("0/2"), "{}", h.screen());
    h.keys("[f[fm");
    assert!(row_of(&mut h, "Core").contains("1/2"), "{}", h.screen());
    assert!(row_of(&mut h, "Docs").contains("1/1"), "{}", h.screen());
}

#[test]
fn a_narrower_scope_narrows_the_groups_and_their_progress() {
    let fx = feature_branch();
    let mut h = review_with(&fx, &core_then_docs());
    h.session().wait_for_analyst();
    h.keys("[f[fm");
    assert!(row_of(&mut h, "Core").contains("1/2"), "{}", h.screen());

    // Commit 1 adds src/a.rs alone: Docs has no files there.
    h.keys(":scope 1<CR>");
    let side = sidebar(&mut h);
    assert!(row_of(&mut h, "Core").contains("1/1"), "{side}");
    assert!(!side.contains("Docs"), "{side}");

    // Commit 2 adds src/b.rs and touches README.md; src/a.rs is not in it.
    h.keys(":scope 2<CR>");
    let side = sidebar(&mut h);
    assert!(row_of(&mut h, "Core").contains("0/1"), "{side}");
    assert!(row_of(&mut h, "Docs").contains("0/1"), "{side}");
    assert!(!side.contains("src/a.rs"), "{side}");
}

#[test]
fn until_the_analysis_is_ready_the_sidebar_is_the_plain_file_list() {
    let fx = feature_branch();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let gate = Mutex::new(gate);
    let analyst = FakeAnalyst::with(move |_| {
        gate.lock().unwrap().recv().unwrap();
        Ok(Grouping {
            groups: vec![group("Everything", "all of it", false, &["README.md"])],
        })
    });
    let mut h = review_with(&fx, &analyst);
    let side = sidebar(&mut h);
    assert!(
        side.contains("src/a.rs") && side.contains("README.md"),
        "{side}"
    );
    assert!(!side.contains("Everything"), "{side}");

    release.send(()).unwrap();
    h.session().wait_for_analyst();
    assert!(sidebar(&mut h).contains("Everything"));
}

#[test]
fn space_g_switches_between_groups_and_the_directory_tree() {
    let fx = feature_branch();
    let mut h = review_with(&fx, &core_then_docs());
    h.session().wait_for_analyst();
    assert!(sidebar(&mut h).contains("Core"));

    h.keys("<Space>g");
    let side = sidebar(&mut h);
    assert!(!side.contains("Core") && !side.contains("Docs"), "{side}");
    assert!(
        position(&side, "README.md") < position(&side, "src/a.rs"),
        "{side}"
    );

    h.keys("<Space>g");
    assert!(sidebar(&mut h).contains("Core"));
}

#[test]
fn every_file_is_in_exactly_one_group_whatever_the_analyst_answers() {
    let fx = feature_branch();
    // src/a.rs twice (the first wins), a file the Range does not change, and
    // README.md left out.
    let analyst = FakeAnalyst::grouping(vec![
        group(
            "Core",
            "the modules",
            false,
            &["src/a.rs", "ghost.rs", "src/b.rs"],
        ),
        group("Again", "repeats a", false, &["src/a.rs"]),
    ]);
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    let side = sidebar(&mut h);
    assert_eq!(side.matches("src/a.rs").count(), 1, "{side}");
    assert!(!side.contains("ghost"), "{side}");
    assert!(!side.contains("Again"), "{side}");
    assert!(
        position(&side, "Other") < position(&side, "README.md"),
        "{side}"
    );
    assert!(
        position(&side, "src/b.rs") < position(&side, "Other"),
        "{side}"
    );
    assert!(row_of(&mut h, "Other").contains("0/1"), "{side}");
}

#[test]
fn headers_show_the_reading_order_and_mark_mechanical_groups() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::grouping(vec![
        group("Core", "the modules", false, &["src/a.rs", "src/b.rs"]),
        group("Docs", "the readme", true, &["README.md"]),
    ]);
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    let core = row_of(&mut h, "Core");
    let docs = row_of(&mut h, "Docs");
    assert!(
        core.contains("1 Core") && !core.contains("[mech]"),
        "{core}"
    );
    assert!(docs.contains("2 Docs") && docs.contains("[mech]"), "{docs}");
}

#[test]
fn new_commits_mark_the_groups_stale_and_analyze_rebuilds_them() {
    let fx = feature_branch();
    let analyst = core_then_docs();
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    assert!(!h.screen().contains("stale"));

    fx.write("src/c.rs", "fn c() {}\n").commit("add c");
    // Opening the Summary looks at where the head of the Range is now.
    h.keys("<Space>s<Esc>");
    let screen = h.screen();
    assert!(sidebar(&mut h).contains("Core"), "the old Groups stay");
    assert!(screen.contains("groups stale"), "{screen}");

    h.keys(":analyze<CR>");
    h.session().wait_for_analyst();
    let new_head = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    let requests = analyst.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].head, new_head);
    assert!(!h.screen().contains("stale"), "{}", h.screen());
}

#[test]
fn groups_are_cached_per_language() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::with(|r| {
        Ok(Grouping {
            groups: vec![group(
                &format!("All [{}]", r.language),
                "everything",
                false,
                &["README.md", "src/a.rs", "src/b.rs"],
            )],
        })
    });
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    assert!(sidebar(&mut h).contains("All [ru]"));

    h.keys(":lang en<CR>");
    h.session().wait_for_analyst();
    assert!(sidebar(&mut h).contains("All [en]"), "{}", h.screen());

    h.keys(":lang ru<CR>");
    h.session().wait_for_analyst();
    assert!(sidebar(&mut h).contains("All [ru]"));
    assert_eq!(analyst.requests().len(), 2, "ru came from the cache");
}

#[test]
fn when_grouping_fails_the_tree_stays_and_the_reason_is_in_the_status() {
    let fx = feature_branch();
    let analyst =
        FakeAnalyst::with(|_| Err(AnalystError::Failed("claude gave a broken answer".into())));
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    let screen = h.screen();
    let status = screen.lines().rev().nth(1).unwrap(); // the last row holds hints
    assert!(status.contains("claude gave a broken answer"), "{status}");
    let side = sidebar(&mut h);
    assert!(
        side.contains("README.md") && side.contains("src/b.rs"),
        "{side}"
    );
}

#[test]
fn clicking_a_file_under_a_group_header_selects_that_file() {
    let fx = feature_branch();
    let mut h = review_with(&fx, &core_then_docs());
    h.session().wait_for_analyst();
    let (_, row) = h.frame().find("src/b.rs").expect("src/b.rs is listed");
    h.click(5, row);
    let screen = h.screen();
    assert!(screen.contains("file 2/3"), "{screen}");
}

#[test]
fn with_the_unreviewed_filter_groups_keep_counting_the_whole_scope() {
    let fx = feature_branch();
    let mut h = review_with(&fx, &core_then_docs());
    h.session().wait_for_analyst();
    h.keys("[f[fm<Space>u");
    let side = sidebar(&mut h);
    assert!(!side.contains("src/a.rs"), "{side}");
    assert!(row_of(&mut h, "Core").contains("1/2"), "{side}");
}

#[test]
fn an_analyst_that_panics_does_not_hang_the_session() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::with(|_| panic!("boom"));
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    let side = sidebar(&mut h);
    assert!(side.contains("README.md"), "{side}");
}

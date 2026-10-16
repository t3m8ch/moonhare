//! The review session and the Analyst, with a fake standing in for the Analyst.

mod support;

use std::path::Path;
use std::sync::{Arc, Mutex};

use moonhare::analyst::{Analyst, AnalystError, Summary, SummaryRequest};
use moonhare::forge::{Forge, ForgeError, PullRequest};
use moonhare::git::Target;
use moonhare::session::Session;
use support::{feature_branch, Fixture, Harness};

type Answer = Box<dyn Fn(&SummaryRequest) -> Result<Summary, AnalystError> + Send + Sync>;

/// An Analyst that answers from memory and remembers what it was asked.
struct FakeAnalyst {
    answer: Answer,
    requests: Mutex<Vec<SummaryRequest>>,
}

impl FakeAnalyst {
    fn answering(text: &str) -> Arc<FakeAnalyst> {
        let text = text.to_string();
        Self::with(move |_| Ok(Summary {
                text: text.clone(),
                ..Summary::default()
            }))
    }

    fn failing(error: AnalystError) -> Arc<FakeAnalyst> {
        Self::with(move |_| Err(error.clone()))
    }

    fn with(
        answer: impl Fn(&SummaryRequest) -> Result<Summary, AnalystError> + Send + Sync + 'static,
    ) -> Arc<FakeAnalyst> {
        Arc::new(FakeAnalyst {
            answer: Box::new(answer),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn requests(&self) -> Vec<SummaryRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Analyst for FakeAnalyst {
    fn summarize(&self, request: &SummaryRequest) -> Result<Summary, AnalystError> {
        self.requests.lock().unwrap().push(request.clone());
        (self.answer)(request)
    }
}

/// A Review of the `feature` branch with the Analyst already asked once.
fn review_with(fx: &Fixture, analyst: &Arc<FakeAnalyst>) -> Harness {
    let mut session = Session::open(fx.path(), 100, 30).expect("session opens");
    session.set_analyst(analyst.clone());
    Harness::from_session(session)
}

#[test]
fn the_summary_is_built_in_the_background_while_the_diff_is_readable() {
    let fx = feature_branch();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let gate = Mutex::new(gate);
    let analyst = FakeAnalyst::with(move |_| {
        gate.lock().unwrap().recv().unwrap();
        Ok(Summary {
            text: "Finally ready".into(),
            ..Summary::default()
        })
    });
    let mut h = review_with(&fx, &analyst);

    h.keys("j");
    assert!(h.screen().contains("README.md"));
    h.keys("<Space>s");
    assert!(h.screen().contains("being built"), "{}", h.screen());
    h.keys("<Esc>");

    release.send(()).unwrap();
    h.session().wait_for_analyst();
    h.keys("<Space>s");
    assert!(h.screen().contains("Finally ready"), "{}", h.screen());
}

#[test]
fn the_analyst_is_asked_about_the_range_in_the_default_language() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::answering("ok");
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    let requests = analyst.requests();
    assert_eq!(requests.len(), 1);
    let r = &requests[0];
    assert_eq!(r.language, "ru");
    assert_eq!(r.head, fx.git(&["rev-parse", "feature"]).trim());
    assert_eq!(r.base, fx.git(&["rev-parse", "main"]).trim());
    assert_eq!(r.pull_request, None);
}

/// A Forge that knows one pull request, or fails.
struct FakeForge(Result<Option<PullRequest>, ForgeError>);

impl Forge for FakeForge {
    fn pull_request_for_branch(&self, _: &str) -> Result<Option<PullRequest>, ForgeError> {
        self.0.clone()
    }
    fn pull_request(&self, _: u64) -> Result<PullRequest, ForgeError> {
        unreachable!()
    }
    fn fetch_pull_request_head(&self, _: &Path, _: u64) -> Result<String, ForgeError> {
        unreachable!()
    }
}

fn review_with_forge(fx: &Fixture, forge: FakeForge, analyst: &Arc<FakeAnalyst>) -> Harness {
    let mut session = Session::open_target_with_forge(fx.path(), &Target::Current, &forge, 100, 30)
        .expect("session opens");
    session.set_analyst(analyst.clone());
    Harness::from_session(session)
}

#[test]
fn the_pull_request_title_and_description_are_context_for_the_analyst() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::answering("ok");
    let pr = PullRequest {
        number: 12,
        title: "Add a and b".into(),
        body: "Needed for the release".into(),
        base_branch: "main".into(),
    };
    let mut h = review_with_forge(&fx, FakeForge(Ok(Some(pr.clone()))), &analyst);
    h.session().wait_for_analyst();
    assert_eq!(analyst.requests()[0].pull_request, Some(pr));
}

#[test]
fn without_forge_details_the_analyst_works_from_the_diff_alone() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::answering("ok");
    let forge = FakeForge(Err(ForgeError::Unavailable("gh is not installed".into())));
    let mut h = review_with_forge(&fx, forge, &analyst);
    h.session().wait_for_analyst();
    assert_eq!(analyst.requests().len(), 1);
    assert_eq!(analyst.requests()[0].pull_request, None);
}

#[test]
fn when_the_analyst_fails_the_reason_is_in_the_status_and_the_rest_works() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::failing(AnalystError::NotLoggedIn(
        "claude is not logged in: run `claude /login`".into(),
    ));
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    let screen = h.screen();
    let status = screen.lines().last().unwrap();
    assert!(status.contains("claude /login"), "{status}");
    h.keys("]f");
    assert!(h.screen().contains("src/a.rs"));
    h.keys("<Space>s");
    let screen = h.screen();
    assert!(screen.contains("claude /login"), "{screen}");
}

/// An Analyst whose Summary names the head it was built for.
fn head_naming_analyst() -> Arc<FakeAnalyst> {
    FakeAnalyst::with(|r| {
        Ok(Summary {
            text: format!("Built for {}", &r.head[..7]),
            ..Summary::default()
        })
    })
}

#[test]
fn new_commits_mark_the_summary_stale_and_analyze_rebuilds_it() {
    let fx = feature_branch();
    let analyst = head_naming_analyst();
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    let old_head = fx
        .git(&["rev-parse", "--short=7", "HEAD"])
        .trim()
        .to_string();

    fx.write("src/c.rs", "fn c() {}\n").commit("add c");
    let new_head = fx
        .git(&["rev-parse", "--short=7", "HEAD"])
        .trim()
        .to_string();

    h.keys("<Space>s");
    let screen = h.screen();
    assert!(
        screen.contains(&format!("Built for {old_head}")),
        "{screen}"
    );
    assert!(screen.contains("stale"), "{screen}");
    h.keys("<Esc>:analyze<CR>");
    h.session().wait_for_analyst();
    h.keys("<Space>s");
    let screen = h.screen();
    assert!(
        screen.contains(&format!("Built for {new_head}")),
        "{screen}"
    );
    assert!(!screen.contains("stale"), "{screen}");
    assert_eq!(analyst.requests().len(), 2);
}

#[test]
fn analyze_rebuilds_even_when_nothing_changed() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::answering("same");
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    h.keys(":analyze<CR>");
    h.session().wait_for_analyst();
    assert_eq!(analyst.requests().len(), 2);
}

/// An Analyst whose Summary names the language it was asked for.
fn language_naming_analyst() -> Arc<FakeAnalyst> {
    FakeAnalyst::with(|r| {
        Ok(Summary {
            text: format!("Summary in {}", r.language),
            ..Summary::default()
        })
    })
}

#[test]
fn the_language_parameter_sets_the_language_of_the_first_request() {
    let fx = feature_branch();
    let analyst = language_naming_analyst();
    let mut session = Session::open(fx.path(), 100, 30).unwrap();
    session.set_language("en");
    session.set_analyst(analyst.clone());
    let mut h = Harness::from_session(session);
    h.session().wait_for_analyst();
    assert_eq!(analyst.requests()[0].language, "en");
    h.keys("<Space>s");
    assert!(h.screen().contains("Summary in en"));
}

#[test]
fn lang_switches_the_language_and_the_cache_keeps_every_language() {
    let fx = feature_branch();
    let analyst = language_naming_analyst();
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();

    h.keys(":lang en<CR>");
    h.session().wait_for_analyst();
    h.keys("<Space>s");
    assert!(h.screen().contains("Summary in en"), "{}", h.screen());
    h.keys("<Esc>");

    h.keys(":lang ru<CR>");
    h.keys("<Space>s");
    assert!(h.screen().contains("Summary in ru"), "{}", h.screen());
    h.keys("<Esc>");
    h.session().wait_for_analyst();
    assert_eq!(analyst.requests().len(), 2, "ru came from the cache");
}

#[test]
fn lang_without_an_argument_names_the_current_language() {
    let fx = feature_branch();
    let analyst = language_naming_analyst();
    let mut h = review_with(&fx, &analyst);
    h.keys(":lang<CR>");
    let screen = h.screen();
    assert!(screen.lines().last().unwrap().contains("ru"), "{screen}");
}

#[test]
fn space_s_opens_the_summary_overlay() {
    let fx = feature_branch();
    let analyst = FakeAnalyst::answering("Adds modules a and b");
    let mut h = review_with(&fx, &analyst);
    h.session().wait_for_analyst();
    h.keys("<Space>s");
    let screen = h.screen();
    assert!(screen.contains("Summary"), "{screen}");
    assert!(screen.contains("Adds modules a and b"), "{screen}");
    h.keys("<Esc>");
    assert!(!h.screen().contains("Adds modules a and b"));
}

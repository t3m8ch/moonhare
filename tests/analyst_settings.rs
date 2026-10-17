//! `:effort` and `:model`: the Analyst's settings change during the session.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use moonhare::analyst::{
    Analyst, AnalystError, AnalystSettings, Summary, SummaryRequest, EFFORT_LEVELS,
};
use moonhare::claude::ClaudeAnalyst;
use moonhare::session::Session;
use support::{feature_branch, Fixture, Harness};

struct Recording {
    requests: Mutex<Vec<SummaryRequest>>,
}

impl Analyst for Recording {
    fn summarize(&self, request: &SummaryRequest) -> Result<Summary, AnalystError> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(Summary::default())
    }
}

fn review(fx: &Fixture) -> (Harness, Arc<Recording>) {
    let analyst = Arc::new(Recording {
        requests: Mutex::new(Vec::new()),
    });
    let mut session = Session::open(fx.path(), 120, 30).unwrap();
    session.set_analyst(analyst.clone());
    let mut h = Harness::from_session(session);
    h.session().wait_for_analyst();
    (h, analyst)
}

fn last_settings(analyst: &Recording) -> AnalystSettings {
    analyst
        .requests
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .settings
        .clone()
}

fn last_line(h: &mut Harness) -> String {
    h.screen().lines().rev().nth(1).unwrap().to_string()
}

#[test]
fn the_next_request_carries_the_new_effort_and_model() {
    let fx = feature_branch();
    let (mut h, analyst) = review(&fx);
    assert_eq!(last_settings(&analyst), AnalystSettings::default());

    h.keys(":effort high<CR>:model opus<CR>:analyze<CR>");
    h.session().wait_for_analyst();
    let settings = last_settings(&analyst);
    assert_eq!(settings.effort.as_deref(), Some("high"));
    assert_eq!(settings.model.as_deref(), Some("opus"));

    h.keys(":effort default<CR>:model default<CR>:analyze<CR>");
    h.session().wait_for_analyst();
    assert_eq!(last_settings(&analyst), AnalystSettings::default());
}

#[test]
fn a_bad_effort_is_refused_with_the_valid_levels_and_changes_nothing() {
    let fx = feature_branch();
    let (mut h, analyst) = review(&fx);
    h.keys(":effort medium<CR>:effort loud<CR>");
    let line = last_line(&mut h);
    for level in EFFORT_LEVELS {
        assert!(line.contains(level), "{line}");
    }
    assert!(line.contains("loud"), "{line}");
    h.keys(":analyze<CR>");
    h.session().wait_for_analyst();
    assert_eq!(last_settings(&analyst).effort.as_deref(), Some("medium"));
}

#[test]
fn without_an_argument_the_commands_show_the_current_value() {
    let fx = feature_branch();
    let (mut h, _) = review(&fx);
    h.keys(":effort<CR>");
    assert!(last_line(&mut h).contains("default"));
    h.keys(":effort xhigh<CR>:effort<CR>");
    assert!(last_line(&mut h).contains("xhigh"));
    h.keys(":model<CR>");
    assert!(last_line(&mut h).contains("default"));
    h.keys(":model claude-opus-4-1<CR>:model<CR>");
    assert!(last_line(&mut h).contains("claude-opus-4-1"));
}

#[test]
fn a_model_that_looks_like_an_option_is_refused() {
    let fx = feature_branch();
    let (mut h, _) = review(&fx);
    h.keys(":model --dangerously<CR>:model<CR>");
    assert!(last_line(&mut h).contains("default"));
}

#[test]
fn the_status_line_shows_the_values_once_set() {
    let fx = feature_branch();
    let (mut h, _) = review(&fx);
    h.session()
        .handle(ratatui::crossterm::event::Event::Resize(200, 30));
    assert!(!h.screen().contains("effort:"));
    h.keys(":effort max<CR>:model sonnet<CR>j");
    let screen = h.screen();
    assert!(screen.contains("effort:max"), "{screen}");
    assert!(screen.contains("model:sonnet"), "{screen}");
    h.keys(":effort default<CR>j");
    assert!(!h.screen().contains("effort:"));
}

#[test]
fn startup_values_are_validated_and_applied() {
    let fx = feature_branch();
    let (mut h, analyst) = review(&fx);
    assert!(h.session().set_effort("nope").is_err());
    h.session().set_effort("low").unwrap();
    h.session().set_model("haiku").unwrap();
    h.keys(":analyze<CR>");
    h.session().wait_for_analyst();
    let settings = last_settings(&analyst);
    assert_eq!(settings.effort.as_deref(), Some("low"));
    assert_eq!(settings.model.as_deref(), Some("haiku"));
}

#[test]
fn help_mentions_the_commands() {
    let help = moonhare::hints::help_lines().join("\n");
    assert!(help.contains(":effort") && help.contains(":model"));
}

// The adapter: a fake `claude` records its arguments.

fn fake_claude() -> (tempfile::TempDir, ClaudeAnalyst) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("claude");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}/args'\ncat > /dev/null\necho '{{\"is_error\":false,\"result\":\"\",\"structured_output\":{{\"summary\":\"ok\"}}}}'\n",
        dir.path().display()
    );
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let analyst = ClaudeAnalyst::with_program(path);
    (dir, analyst)
}

fn request(fx: &Fixture, settings: AnalystSettings) -> SummaryRequest {
    SummaryRequest {
        repo: fx.path().to_path_buf(),
        base: fx.git(&["rev-parse", "main"]).trim().to_string(),
        head: fx.git(&["rev-parse", "feature"]).trim().to_string(),
        language: "en".into(),
        pull_request: None,
        settings,
    }
}

fn args(dir: &tempfile::TempDir) -> Vec<String> {
    fs::read_to_string(dir.path().join("args"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

fn value_after(args: &[String], flag: &str) -> Option<String> {
    let at = args.iter().position(|a| a == flag)?;
    args.get(at + 1).cloned()
}

#[test]
fn claude_gets_the_flags_only_when_set_and_keeps_its_restrictions() {
    let fx = feature_branch();
    let (dir, analyst) = fake_claude();

    analyst
        .summarize(&request(&fx, AnalystSettings::default()))
        .unwrap();
    let plain = args(&dir);
    assert!(!plain.iter().any(|a| a == "--effort" || a == "--model"));

    let settings = AnalystSettings {
        effort: Some("xhigh".into()),
        model: Some("opus".into()),
    };
    analyst.summarize(&request(&fx, settings)).unwrap();
    let set = args(&dir);
    assert_eq!(value_after(&set, "--effort").as_deref(), Some("xhigh"));
    assert_eq!(value_after(&set, "--model").as_deref(), Some("opus"));
    for flag in [
        "--tools",
        "--permission-mode",
        "--setting-sources",
        "--strict-mcp-config",
        "--add-dir",
    ] {
        assert!(set.iter().any(|a| a == flag), "{flag} missing: {set:?}");
    }
    assert_eq!(
        value_after(&set, "--tools").as_deref(),
        Some("Read,Grep,Glob")
    );
    assert_eq!(
        value_after(&set, "--permission-mode").as_deref(),
        Some("dontAsk")
    );
}

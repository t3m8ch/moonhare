//! The Analyst's answers survive between runs: a second session on the same
//! commits asks the fake `claude` nothing. The calls are counted by the fake.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use moonhare::claude::ClaudeAnalyst;
use moonhare::session::Session;
use support::{feature_branch, Fixture, Harness};
use tempfile::TempDir;

/// A `claude` that appends a line to `calls` on every run and answers every
/// kind of request from the `reply` file.
struct FakeClaude {
    dir: TempDir,
}

impl FakeClaude {
    fn new() -> Self {
        let fake = FakeClaude {
            dir: tempfile::tempdir().unwrap(),
        };
        let d = fake.dir.path();
        let script = format!(
            "#!/bin/sh\nd='{}'\necho call >> \"$d/calls\"\ncat > /dev/null\ncat \"$d/reply\"\n",
            d.display()
        );
        let path = d.join("claude");
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        fake.says("first");
        fake
    }

    /// From now on every request is answered with `word` in every field.
    fn says(&self, word: &str) {
        let output = serde_json::json!({
            "summary": format!("summary {word}"),
            "groups": [{
                "name": format!("group {word}"), "description": "d",
                "mechanical": false, "files": ["src/a.rs", "src/b.rs", "README.md"]
            }],
            "mermaid": "graph LR\n    A --> B",
            "explanation": format!("explanation {word}"),
        });
        let reply = serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false,
            "result": "", "structured_output": output
        });
        fs::write(self.dir.path().join("reply"), reply.to_string()).unwrap();
    }

    fn calls(&self) -> usize {
        fs::read_to_string(self.dir.path().join("calls"))
            .map(|c| c.lines().count())
            .unwrap_or(0)
    }

    fn analyst(&self) -> Arc<ClaudeAnalyst> {
        Arc::new(ClaudeAnalyst::with_program(self.dir.path().join("claude")))
    }

    /// A new session, as after restarting moonhare.
    fn session(&self, fx: &Fixture) -> Harness {
        self.session_with(fx, |_| {})
    }

    fn session_with(&self, fx: &Fixture, setup: impl FnOnce(&mut Session)) -> Harness {
        let mut session = Session::open(fx.path(), 120, 30).expect("session opens");
        setup(&mut session);
        session.set_analyst(self.analyst());
        let mut h = Harness::from_session(session);
        h.session().wait_for_analyst();
        h
    }
}

fn cache_files(fx: &Fixture) -> Vec<std::path::PathBuf> {
    let reviews = fx.path().join(".git/moonhare/reviews");
    let mut found = Vec::new();
    fn walk(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.parent().is_some_and(|p| p.ends_with("analyst")) {
                found.push(path);
            }
        }
    }
    walk(&reviews, &mut found);
    found
}

#[test]
fn a_second_run_on_the_same_commits_does_not_call_claude() {
    let fx = feature_branch();
    let fake = FakeClaude::new();

    let _first = fake.session(&fx);
    assert_eq!(fake.calls(), 2, "the Summary and the Groups");

    let mut second = fake.session(&fx);
    assert_eq!(fake.calls(), 2, "nothing was asked again");
    second.keys("<Space>s");
    let screen = second.screen();
    assert!(screen.contains("summary first"), "{screen}");
    second.keys("<Esc>");
    assert!(second.screen().contains("group first"));
}

#[test]
fn analyze_ignores_the_cache_and_overwrites_it() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let _first = fake.session(&fx);

    fake.says("second");
    let mut forced = fake.session(&fx);
    forced.keys(":analyze<CR>");
    forced.session().wait_for_analyst();
    assert_eq!(fake.calls(), 4);
    forced.keys("<Space>s");
    assert!(forced.screen().contains("summary second"));

    fake.says("third");
    let mut later = fake.session(&fx);
    assert_eq!(fake.calls(), 4, "the refreshed answer is the cached one");
    later.keys("<Space>s");
    assert!(later.screen().contains("summary second"));
}

#[test]
fn a_new_commit_is_a_new_request() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let _first = fake.session(&fx);

    fx.write("src/c.rs", "fn c() {}\n").commit("add c");
    let _second = fake.session(&fx);
    assert_eq!(fake.calls(), 4);

    let _again = fake.session(&fx);
    assert_eq!(fake.calls(), 4, "both heads stay cached");
}

#[test]
fn another_language_is_another_entry() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let _ru = fake.session(&fx);

    let _en = fake.session_with(&fx, |s| s.set_language("en"));
    assert_eq!(fake.calls(), 4);

    let _en_again = fake.session_with(&fx, |s| s.set_language("en"));
    let _ru_again = fake.session(&fx);
    assert_eq!(fake.calls(), 4);
}

#[test]
fn effort_and_model_do_not_change_the_key() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let _first = fake.session(&fx);

    let _tuned = fake.session_with(&fx, |s| {
        s.set_effort("max").unwrap();
        s.set_model("opus").unwrap();
    });
    assert_eq!(fake.calls(), 2);
}

#[test]
fn a_damaged_cache_file_is_ignored_and_rebuilt() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let _first = fake.session(&fx);
    let files = cache_files(&fx);
    assert_eq!(files.len(), 2);

    fs::write(&files[0], b"\xff\xfe not a cache").unwrap();
    fs::write(&files[1], "moonhare-analyst-cache 99\nkey\n{").unwrap();
    let mut second = fake.session(&fx);
    assert_eq!(fake.calls(), 4, "both were asked for again");
    second.keys("<Space>s");
    assert!(second.screen().contains("summary first"));

    let _third = fake.session(&fx);
    assert_eq!(fake.calls(), 4, "and the rebuilt entries are good");
}

#[test]
fn a_truncated_answer_in_a_valid_file_is_a_miss() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let _first = fake.session(&fx);
    for file in cache_files(&fx) {
        let text = fs::read_to_string(&file).unwrap();
        let cut = text.rfind('{').unwrap();
        fs::write(&file, format!("{}{{\"unexpected\":1}}", &text[..cut])).unwrap();
    }
    let _second = fake.session(&fx);
    assert_eq!(fake.calls(), 4);
}

#[test]
fn an_explanation_of_the_same_file_content_is_cached() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let mut first = fake.session(&fx);
    first.keys("<Space>X");
    first.session().wait_for_analyst();
    assert!(first.screen().contains("explanation first"));
    assert_eq!(fake.calls(), 3);

    let mut second = fake.session(&fx);
    second.keys("<Space>X");
    second.session().wait_for_analyst();
    assert_eq!(fake.calls(), 3);
    assert!(second.screen().contains("explanation first"));
}

#[test]
fn a_diagram_is_cached() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let mut first = fake.session(&fx);
    first.keys(":diagram<CR>");
    first.session().wait_for_analyst();
    assert_eq!(fake.calls(), 3);

    let mut second = fake.session(&fx);
    second.keys(":diagram<CR>");
    second.session().wait_for_analyst();
    assert_eq!(fake.calls(), 3);
    assert!(second.screen().contains("Diagram"));
}

#[test]
fn an_explanation_of_a_file_is_asked_again_once_its_content_changed() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let mut first = fake.session(&fx);
    first.keys("<Space>X");
    first.session().wait_for_analyst();
    assert_eq!(fake.calls(), 3);

    // The cursor starts on README.md; changing it changes the blob.
    fx.write("README.md", "# demo\n\nrewritten\n")
        .commit("rewrite readme");
    let mut second = fake.session(&fx);
    second.keys("<Space>X");
    second.session().wait_for_analyst();
    assert_eq!(
        fake.calls(),
        6,
        "Summary and Groups for the new head, and the file"
    );
}

#[test]
fn the_screen_says_cached_for_answers_read_from_disk() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let mut first = fake.session(&fx);
    first.keys("<Space>s");
    assert!(!first.screen().contains("cached"));
    first.keys("<Esc>");

    let mut second = fake.session(&fx);
    assert!(
        second.screen().contains("groups cached"),
        "{}",
        second.screen()
    );
    second.keys("<Space>s");
    assert!(
        second.screen().contains("Summary [ru] cached"),
        "{}",
        second.screen()
    );
    second.keys("<Esc>:analyze<CR>");
    second.session().wait_for_analyst();
    assert!(!second.screen().contains("groups cached"));
}

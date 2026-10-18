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
            "#!/bin/sh\n[ \"$1\" = --probe ] && exit 0\nd='{}'\necho call >> \"$d/calls\"\ncat > /dev/null\ncat \"$d/reply\"\n",
            d.display()
        );
        let path = d.join("claude");
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        // Another test thread may fork while the script is still open for
        // writing, which makes exec fail with ETXTBSY and loses a call. Run
        // it once to be sure it is settled.
        let probe = || std::process::Command::new(&path).arg("--probe").status();
        while probe().is_err() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
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
            "mermaid": format!("graph LR\n    A --> W{word}"),
            "explanation": format!("explanation {word}"),
        });
        let reply = serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false,
            "result": "", "structured_output": output
        });
        fs::write(self.dir.path().join("reply"), reply.to_string()).unwrap();
    }

    /// From now on every request fails.
    fn fails(&self) {
        let reply = serde_json::json!({
            "type": "result", "subtype": "error_during_execution", "is_error": true,
            "result": "the fake is down"
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
fn the_size_of_the_window_is_no_part_of_the_key_of_a_diagram() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let mut first = fake.session(&fx);
    first.keys(":diagram<CR>");
    first.session().wait_for_analyst();
    let calls = fake.calls();

    let mut second = fake.session(&fx);
    second.resize(200, 60);
    second.keys(":diagram<CR>");
    second.session().wait_for_analyst();
    assert_eq!(fake.calls(), calls);
    assert!(second.screen().contains("cached"), "{}", second.screen());
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

/// The directory of the one Review the fixture has.
fn review_dir(fx: &Fixture) -> std::path::PathBuf {
    let reviews = fx.path().join(".git/moonhare/reviews");
    let mut dirs: Vec<_> = fs::read_dir(&reviews)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert_eq!(dirs.len(), 1, "{dirs:?}");
    dirs.pop().unwrap()
}

#[test]
fn analyze_all_forgets_this_reviews_cache_and_asks_again() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let mut h = fake.session(&fx);
    h.keys("<Space>X");
    h.session().wait_for_analyst();
    h.keys("<Esc>:diagram<CR>");
    h.session().wait_for_analyst();
    h.keys("<Esc>");
    assert_eq!(fake.calls(), 4, "Summary, Groups, Explanation, Diagram");
    assert_eq!(cache_files(&fx).len(), 4);

    let review = review_dir(&fx);
    fs::write(review.join("keep-me"), "not the Analyst's").unwrap();
    let other = review.parent().unwrap().join("other-review/analyst");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("entry"), "another Review's answer").unwrap();

    fake.says("second");
    h.keys(":analyze all<CR>");
    assert!(h.screen().contains("y / n"), "{}", h.screen());
    h.keys("y");
    h.session().wait_for_analyst();
    assert_eq!(fake.calls(), 6, "the Summary and the Groups at once");
    let own: Vec<_> = cache_files(&fx)
        .into_iter()
        .filter(|f| f.starts_with(&review))
        .collect();
    assert_eq!(own.len(), 2, "only the new Summary and Groups: {own:?}");
    assert!(review.join("keep-me").exists());
    assert!(other.join("entry").exists());
    assert!(h.screen().contains("group second"), "{}", h.screen());

    h.keys("<Space>X");
    h.session().wait_for_analyst();
    assert_eq!(fake.calls(), 7, "the Explanation is asked for again");
    assert!(h.screen().contains("explanation second"), "{}", h.screen());
    h.keys("<Esc>:diagram<CR>");
    h.session().wait_for_analyst();
    assert_eq!(fake.calls(), 8, "and the Diagram");
    assert!(h.screen().contains("Wsecond"), "{}", h.screen());
}

#[test]
fn analyze_all_cancelled_deletes_nothing() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let mut h = fake.session(&fx);
    let before = cache_files(&fx);
    h.keys(":analyze all<CR>");
    assert!(h.screen().contains("y / n"), "{}", h.screen());
    h.keys("n");
    h.session().wait_for_analyst();
    assert_eq!(fake.calls(), 2);
    assert_eq!(cache_files(&fx), before);
    assert!(!h.screen().contains("y / n"));
}

#[test]
fn r_in_the_explanation_asks_once_more_and_overwrites_the_entry() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let mut first = fake.session(&fx);
    first.keys("<Space>X");
    first.session().wait_for_analyst();
    assert_eq!(fake.calls(), 3);

    let mut h = fake.session(&fx);
    h.keys("<Space>X");
    assert!(h.screen().contains("cached"), "{}", h.screen());
    fake.says("second");
    h.keys("r");
    let during = h.screen();
    assert!(during.contains("explanation first"), "{during}");
    assert!(during.contains("updating"), "{during}");
    assert!(!during.contains("cached"), "{during}");
    h.session().wait_for_analyst();
    assert_eq!(fake.calls(), 4, "exactly one new request");
    let screen = h.screen();
    assert!(screen.contains("explanation second"), "{screen}");
    assert!(!screen.contains("cached"), "{screen}");
    assert!(!screen.contains("updating"), "{screen}");

    fake.says("third");
    let mut later = fake.session(&fx);
    later.keys("<Space>X");
    later.session().wait_for_analyst();
    assert_eq!(fake.calls(), 4);
    assert!(later.screen().contains("explanation second"));
}

#[test]
fn a_failed_refresh_of_an_explanation_keeps_the_old_one() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let mut h = fake.session(&fx);
    h.keys("<Space>X");
    h.session().wait_for_analyst();
    fake.fails();
    h.keys("r");
    h.session().wait_for_analyst();
    assert_eq!(fake.calls(), 4);
    let screen = h.screen();
    assert!(screen.contains("explanation first"), "{screen}");
    assert!(screen.contains("⚠ Analyst"), "{screen}");
    assert!(screen.contains("previous explanation is kept"), "{screen}");
}

#[test]
fn r_in_the_diagram_asks_once_more_and_overwrites_the_entry() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let mut first = fake.session(&fx);
    first.keys(":diagram<CR>");
    first.session().wait_for_analyst();
    assert_eq!(fake.calls(), 3);

    let mut h = fake.session(&fx);
    h.keys(":diagram<CR>");
    assert!(h.screen().contains("cached"), "{}", h.screen());
    fake.says("second");
    h.keys("r");
    let during = h.screen();
    assert!(during.contains("Wfirst"), "{during}");
    assert!(during.contains("updating"), "{during}");
    assert!(!during.contains("cached"), "{during}");
    h.session().wait_for_analyst();
    assert_eq!(fake.calls(), 4, "exactly one new request");
    let screen = h.screen();
    assert!(screen.contains("Wsecond"), "{screen}");
    assert!(!screen.contains("cached"), "{screen}");
    assert!(!screen.contains("updating"), "{screen}");

    let mut later = fake.session(&fx);
    later.keys(":diagram<CR>");
    later.session().wait_for_analyst();
    assert_eq!(fake.calls(), 4);
    assert!(later.screen().contains("Wsecond"));
}

#[test]
fn a_failed_refresh_of_a_diagram_keeps_the_old_one() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let mut h = fake.session(&fx);
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
    fake.fails();
    h.keys("r");
    h.session().wait_for_analyst();
    assert_eq!(fake.calls(), 4);
    let screen = h.screen();
    assert!(screen.contains("Wfirst"), "{screen}");
    assert!(screen.contains("Analyst:"), "{screen}");
}

#[test]
fn help_and_overlay_hints_name_the_refresh_keys() {
    let fx = feature_branch();
    let fake = FakeClaude::new();
    let mut h = fake.session(&fx);
    h.resize(140, 160);
    h.keys("?");
    let help = h.screen();
    for (key, word) in [(":analyze [all]", "саммари"), (":analyze [all]", "кэш")] {
        let found = help.lines().any(|l| {
            let row = l.trim_start_matches(['│', ' ']);
            row.starts_with(key) && l.contains(word)
        });
        assert!(found, "no help row for {key} ({word}):\n{help}");
    }
    assert!(help.contains("r в окне: пересчитать"), "{help}");
    h.keys("<Esc><Space>X");
    h.session().wait_for_analyst();
    let last = h.screen().lines().last().unwrap_or("").to_string();
    assert!(last.contains("r: пересчитать"), "{last}");
    h.keys("<Esc>:diagram<CR>");
    h.session().wait_for_analyst();
    let last = h.screen().lines().last().unwrap_or("").to_string();
    assert!(last.contains("r: пересчитать"), "{last}");
}

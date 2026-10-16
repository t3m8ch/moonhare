//! The `claude -p` adapter explaining a hunk or a file, against a fake
//! executable standing in for `claude`.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use moonhare::analyst::{Analyst, AnalystError, ExplainRequest, ExplainTarget};
use moonhare::claude::ClaudeAnalyst;
use support::{feature_branch, Fixture};
use tempfile::TempDir;

/// A `claude` that records how it was called and replies with canned output.
struct FakeClaude {
    dir: TempDir,
}

impl FakeClaude {
    fn replying(stdout: &str) -> Self {
        let fake = FakeClaude {
            dir: tempfile::tempdir().unwrap(),
        };
        let d = fake.dir.path();
        fs::write(d.join("reply"), stdout).unwrap();
        let script = format!(
            "#!/bin/sh\nd='{}'\nprintf '%s\\n' \"$@\" > \"$d/args\"\ncat > \"$d/stdin\"\npwd > \"$d/cwd\"\ncat \"$d/reply\"\n",
            d.display()
        );
        let path = d.join("claude");
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        fake
    }

    fn analyst(&self) -> ClaudeAnalyst {
        ClaudeAnalyst::with_program(self.dir.path().join("claude"))
    }

    fn recorded(&self, name: &str) -> String {
        fs::read_to_string(self.dir.path().join(name)).unwrap_or_default()
    }
}

fn success(explanation: &str) -> String {
    serde_json::json!({
        "type": "result", "subtype": "success", "is_error": false,
        "result": "", "structured_output": { "explanation": explanation }
    })
    .to_string()
}

fn request(fx: &Fixture, target: ExplainTarget) -> ExplainRequest {
    ExplainRequest {
        repo: fx.path().to_path_buf(),
        base: fx.git(&["rev-parse", "main"]).trim().to_string(),
        head: fx.git(&["rev-parse", "feature"]).trim().to_string(),
        language: "ru".into(),
        pull_request: None,
        target,
    }
}

fn hunk() -> ExplainTarget {
    ExplainTarget::Hunk {
        path: "src/a.rs".into(),
        text: "-    1\n+    10\n".into(),
    }
}

#[test]
fn returns_the_explanation_from_the_structured_output() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&success("Константа выросла"));
    let explanation = fake.analyst().explain(&request(&fx, hunk())).unwrap();
    assert_eq!(explanation.text, "Константа выросла");
}

#[test]
fn asks_for_an_explanation_schema_and_runs_as_safely_as_the_summary() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&success("x"));
    fake.analyst().explain(&request(&fx, hunk())).unwrap();
    let args: Vec<String> = fake.recorded("args").lines().map(String::from).collect();
    let at = args.iter().position(|a| a == "--json-schema").unwrap();
    let schema: serde_json::Value = serde_json::from_str(&args[at + 1]).unwrap();
    assert_eq!(schema["required"], serde_json::json!(["explanation"]));
    let at = args.iter().position(|a| a == "--tools").unwrap();
    assert_eq!(args[at + 1], "Read,Grep,Glob");
    let at = args.iter().position(|a| a == "--setting-sources").unwrap();
    assert_eq!(args[at + 1], "user");
    assert!(
        args.contains(&"--strict-mcp-config".to_string()),
        "{args:?}"
    );
    assert!(
        !fake
            .recorded("cwd")
            .trim()
            .starts_with(fx.path().to_str().unwrap()),
        "ran inside the reviewed tree"
    );
}

#[test]
fn the_prompt_for_a_hunk_names_the_file_the_hunk_and_the_language() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&success("x"));
    let mut req = request(&fx, hunk());
    req.language = "en".into();
    fake.analyst().explain(&req).unwrap();
    let prompt = fake.recorded("stdin");
    assert!(prompt.contains("src/a.rs"), "{prompt}");
    assert!(prompt.contains("+    10"), "{prompt}");
    assert!(prompt.contains("Language: en"), "{prompt}");
}

#[test]
fn the_prompt_for_a_file_carries_the_diff_of_that_file_only() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&success("x"));
    let target = ExplainTarget::File {
        path: "src/b.rs".into(),
    };
    fake.analyst().explain(&request(&fx, target)).unwrap();
    let prompt = fake.recorded("stdin");
    assert!(prompt.contains("src/b.rs"), "{prompt}");
    assert!(prompt.contains("+    2"), "{prompt}");
    assert!(!prompt.contains("+    10"), "{prompt}");
}

#[test]
fn an_answer_without_an_explanation_is_a_failure() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&success("x").replace("explanation", "other"));
    let err = fake.analyst().explain(&request(&fx, hunk())).unwrap_err();
    assert!(matches!(err, AnalystError::Failed(_)), "{err:?}");
}

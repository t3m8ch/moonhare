//! The `claude -p` adapter asked for Groups, against a fake executable
//! standing in for `claude`.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use moonhare::analyst::{Analyst, AnalystError, Group, GroupRequest};
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
            "#!/bin/sh\nd='{}'\nprintf '%s\\n' \"$@\" > \"$d/args\"\ncat > \"$d/stdin\"\ncat \"$d/reply\"\n",
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

fn answer(structured_output: serde_json::Value) -> String {
    serde_json::json!({
        "type": "result", "subtype": "success", "is_error": false,
        "result": "", "structured_output": structured_output
    })
    .to_string()
}

fn request(fx: &Fixture) -> GroupRequest {
    GroupRequest {
        repo: fx.path().to_path_buf(),
        base: fx.git(&["rev-parse", "main"]).trim().to_string(),
        head: fx.git(&["rev-parse", "feature"]).trim().to_string(),
        language: "ru".into(),
        pull_request: None,
        files: vec!["README.md".into(), "src/a.rs".into(), "src/b.rs".into()],
    }
}

fn value_after(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .map(|i| args[i + 1].clone())
}

#[test]
fn returns_the_groups_from_the_structured_output_in_order() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&answer(serde_json::json!({
        "groups": [
            { "name": "Модули", "description": "a и b", "mechanical": false,
              "files": ["src/a.rs", "src/b.rs"] },
            { "name": "Документация", "description": "readme", "mechanical": true,
              "files": ["README.md"] }
        ]
    })));
    let grouping = fake.analyst().group(&request(&fx)).unwrap();
    assert_eq!(
        grouping.groups,
        vec![
            Group {
                name: "Модули".into(),
                description: "a и b".into(),
                mechanical: false,
                files: vec!["src/a.rs".into(), "src/b.rs".into()],
            },
            Group {
                name: "Документация".into(),
                description: "readme".into(),
                mechanical: true,
                files: vec!["README.md".into()],
            },
        ]
    );
}

#[test]
fn asks_for_groups_in_the_language_and_lists_the_files_under_the_same_safeguards() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&answer(serde_json::json!({ "groups": [] })));
    fake.analyst().group(&request(&fx)).unwrap();
    let args: Vec<String> = fake.recorded("args").lines().map(String::from).collect();
    let schema: serde_json::Value =
        serde_json::from_str(&value_after(&args, "--json-schema").expect("a schema")).unwrap();
    assert_eq!(schema["required"], serde_json::json!(["groups"]));
    assert!(
        args.contains(&"--strict-mcp-config".to_string()),
        "{args:?}"
    );
    assert_eq!(
        value_after(&args, "--tools").as_deref(),
        Some("Read,Grep,Glob")
    );
    let prompt = fake.recorded("stdin");
    for file in ["README.md", "src/a.rs", "src/b.rs"] {
        assert!(prompt.contains(file), "{prompt}");
    }
    assert!(prompt.contains("Language: ru"), "{prompt}");
}

#[test]
fn an_answer_without_groups_is_a_failure() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&answer(serde_json::json!({ "summary": "x" })));
    let error = fake.analyst().group(&request(&fx)).unwrap_err();
    assert!(matches!(error, AnalystError::Failed(_)), "{error:?}");
}

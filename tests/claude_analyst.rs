//! The `claude -p` adapter against a fake executable standing in for `claude`.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use moonhare::analyst::{Analyst, AnalystError, SummaryRequest};
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
        fs::write(d.join("code"), "0").unwrap();
        let script = format!(
            "#!/bin/sh\nd='{}'\nprintf '%s\\n' \"$@\" > \"$d/args\"\nenv > \"$d/env\"\ncat > \"$d/stdin\"\n[ -f \"$d/hang\" ] && exec sleep 30\npwd > \"$d/cwd\"\nls -A . > \"$d/cwdfiles\"\nadd=; prev=; for a in \"$@\"; do [ \"$prev\" = --add-dir ] && add=\"$a\"; prev=\"$a\"; done\necho \"$add\" > \"$d/adddir\"\ngit -C \"$add\" rev-parse HEAD > \"$d/head\" 2>/dev/null\nls -A \"$add\" > \"$d/addfiles\"\ncat \"$d/reply\"\nexit $(cat \"$d/code\")\n",
            d.display()
        );
        let path = d.join("claude");
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        fake
    }

    fn hang(&self) {
        fs::write(self.dir.path().join("hang"), "").unwrap();
    }

    fn fail_with(&self, code: i32) {
        fs::write(self.dir.path().join("code"), code.to_string()).unwrap();
    }

    fn analyst(&self) -> ClaudeAnalyst {
        ClaudeAnalyst::with_program(self.dir.path().join("claude"))
    }

    fn recorded(&self, name: &str) -> String {
        fs::read_to_string(self.dir.path().join(name)).unwrap_or_default()
    }
}

fn success(summary: &str) -> String {
    serde_json::json!({
        "type": "result", "subtype": "success", "is_error": false,
        "result": "", "structured_output": { "summary": summary }
    })
    .to_string()
}

fn request(fx: &Fixture) -> SummaryRequest {
    SummaryRequest {
        repo: fx.path().to_path_buf(),
        base: fx.git(&["rev-parse", "main"]).trim().to_string(),
        head: fx.git(&["rev-parse", "feature"]).trim().to_string(),
        language: "ru".into(),
        pull_request: None,
    }
}

#[test]
fn returns_the_summary_from_the_structured_output() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&success("Добавлены a и b"));
    let summary = fake.analyst().summarize(&request(&fx)).unwrap();
    assert_eq!(summary.text, "Добавлены a и b");
}

fn args_of(fake: &FakeClaude) -> Vec<String> {
    fake.recorded("args").lines().map(String::from).collect()
}

fn value_after(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .map(|i| args[i + 1].clone())
}

#[test]
fn runs_print_mode_with_a_json_schema_and_json_output() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&success("x"));
    fake.analyst().summarize(&request(&fx)).unwrap();
    let args = args_of(&fake);
    assert!(args.contains(&"-p".to_string()), "{args:?}");
    assert_eq!(
        value_after(&args, "--output-format").as_deref(),
        Some("json")
    );
    let schema: serde_json::Value =
        serde_json::from_str(&value_after(&args, "--json-schema").expect("a schema")).unwrap();
    assert_eq!(schema["required"], serde_json::json!(["summary"]));
    assert_eq!(schema["properties"]["summary"]["type"], "string");
}

#[test]
fn offers_read_only_tools_and_nothing_that_writes() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&success("x"));
    fake.analyst().summarize(&request(&fx)).unwrap();
    let args = args_of(&fake);
    assert_eq!(
        value_after(&args, "--tools").as_deref(),
        Some("Read,Grep,Glob")
    );
    assert_eq!(
        value_after(&args, "--allowedTools").as_deref(),
        Some("Read,Grep,Glob")
    );
    assert_eq!(
        value_after(&args, "--permission-mode").as_deref(),
        Some("dontAsk")
    );
    assert!(args.contains(&"--no-session-persistence".to_string()));
    for forbidden in ["Bash", "Edit", "Write", "bypassPermissions"] {
        assert!(!args.iter().any(|a| a.contains(forbidden)), "{forbidden}");
    }
}

#[test]
fn strips_anthropic_variables_from_the_environment() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&success("x"));
    let analyst = fake.analyst().with_environment([
        ("ANTHROPIC_API_KEY", "sk-secret"),
        ("ANTHROPIC_BASE_URL", "http://elsewhere"),
        ("PATH", "/usr/bin:/bin"),
        ("HOME", "/home/reviewer"),
    ]);
    analyst.summarize(&request(&fx)).unwrap();
    let env = fake.recorded("env");
    assert!(!env.contains("ANTHROPIC"), "{env}");
    assert!(!env.contains("sk-secret"), "{env}");
    assert!(env.contains("HOME=/home/reviewer"), "{env}");
}

#[test]
fn strips_variables_that_switch_the_provider_or_carry_cloud_credentials() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&success("x"));
    let analyst = fake.analyst().with_environment([
        ("CLAUDE_CODE_USE_BEDROCK", "1"),
        ("CLAUDE_CODE_USE_VERTEX", "1"),
        ("AWS_SECRET_ACCESS_KEY", "aws-secret"),
        ("PATH", "/usr/bin:/bin"),
        ("CLAUDE_CONFIG_DIR", "/home/reviewer/.claude-alt"),
    ]);
    analyst.summarize(&request(&fx)).unwrap();
    let env = fake.recorded("env");
    for gone in ["CLAUDE_CODE_USE", "AWS_", "aws-secret"] {
        assert!(!env.contains(gone), "{gone}: {env}");
    }
    assert!(
        env.contains("CLAUDE_CONFIG_DIR=/home/reviewer/.claude-alt"),
        "{env}"
    );
}

#[test]
fn works_in_a_temporary_worktree_on_the_head_and_removes_it() {
    let fx = feature_branch();
    fx.checkout("main");
    let before = fx.state();
    let fake = FakeClaude::replying(&success("x"));
    let req = request(&fx);
    fake.analyst().summarize(&req).unwrap();
    let worktree = fake.recorded("adddir");
    let worktree = worktree.trim();
    assert_ne!(std::path::Path::new(worktree), fx.path());
    assert_eq!(fake.recorded("head").trim(), req.head);
    assert!(
        !std::path::Path::new(worktree).exists(),
        "worktree left behind"
    );
    assert_eq!(fx.git(&["worktree", "list"]).lines().count(), 1);
    assert_eq!(fx.state(), before);
}

#[test]
fn never_runs_in_the_reviewed_tree_so_its_project_settings_do_not_apply() {
    let fx = feature_branch();
    fx.write(
        ".claude/settings.json",
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"touch /tmp/pwned"}]}]}}"#,
    )
    .write(".mcp.json", r#"{"mcpServers":{}}"#)
    .commit("add project settings");
    let fake = FakeClaude::replying(&success("x"));
    let mut req = request(&fx);
    req.head = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fake.analyst().summarize(&req).unwrap();

    // The reviewed code is there to read, but only through --add-dir ...
    assert!(fake.recorded("addfiles").contains(".claude"));
    // ... while the process itself starts in an empty directory elsewhere.
    let cwd = fake.recorded("cwd");
    let cwd = std::path::Path::new(cwd.trim());
    assert_eq!(fake.recorded("cwdfiles").trim(), "");
    assert!(!cwd.starts_with(fake.recorded("adddir").trim()));
    assert!(!cwd.starts_with(fx.path()));
    assert!(cwd.ancestors().all(|a| !a.join(".git").exists()));

    // And the settings that could carry hooks or servers are not loaded.
    let args = args_of(&fake);
    assert_eq!(
        value_after(&args, "--setting-sources").as_deref(),
        Some("user")
    );
    assert!(args.contains(&"--strict-mcp-config".to_string()));
    assert!(args.contains(&"--disable-slash-commands".to_string()));
    assert!(!args
        .iter()
        .any(|a| a == "--mcp-config" || a == "--settings"));
}

#[test]
fn the_prompt_names_the_language_the_commits_and_the_diff() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&success("x"));
    let mut req = request(&fx);
    req.language = "en".into();
    fake.analyst().summarize(&req).unwrap();
    let prompt = fake.recorded("stdin");
    assert!(prompt.contains("Language: en"), "{prompt}");
    assert!(prompt.contains("change a"), "{prompt}");
    assert!(prompt.contains("+    10"), "{prompt}");
    assert!(!prompt.contains("Pull request"), "{prompt}");
}

#[test]
fn a_missing_claude_is_reported_as_unavailable() {
    let fx = feature_branch();
    let analyst = ClaudeAnalyst::with_program("/nonexistent/claude");
    let err = analyst.summarize(&request(&fx)).unwrap_err();
    assert!(matches!(err, AnalystError::Unavailable(_)), "{err:?}");
    assert!(err.to_string().contains("claude"), "{err}");
}

#[test]
fn no_login_is_reported_as_such() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(
        r#"{"type":"result","is_error":true,"result":"Not logged in · Please run /login"}"#,
    );
    fake.fail_with(1);
    let err = fake.analyst().summarize(&request(&fx)).unwrap_err();
    assert!(matches!(err, AnalystError::NotLoggedIn(_)), "{err:?}");
    assert!(err.to_string().contains("claude /login"), "{err}");
}

#[test]
fn a_revision_that_looks_like_an_option_is_never_taken_for_one() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&success("x"));
    let planted = fx.path().join("planted");
    let mut req = request(&fx);
    req.base = format!("--output={}", planted.display());
    let _ = fake.analyst().summarize(&req);
    assert!(!planted.exists(), "git took the revision for an option");
}

#[test]
fn worktrees_left_behind_by_a_crashed_run_are_pruned() {
    let fx = feature_branch();
    let leftover = tempfile::tempdir().unwrap();
    let path = leftover.path().join("gone");
    fx.git(&[
        "worktree",
        "add",
        "--detach",
        path.to_str().unwrap(),
        "feature",
    ]);
    fs::remove_dir_all(&path).unwrap();
    assert_eq!(fx.git(&["worktree", "list"]).lines().count(), 2);
    let fake = FakeClaude::replying(&success("x"));
    fake.analyst().summarize(&request(&fx)).unwrap();
    assert_eq!(fx.git(&["worktree", "list"]).lines().count(), 1);
}

#[test]
fn a_claude_that_hangs_is_stopped_after_the_timeout() {
    let fx = feature_branch();
    fx.checkout("main");
    let fake = FakeClaude::replying(&success("x"));
    fake.hang();
    let analyst = fake
        .analyst()
        .with_timeout(std::time::Duration::from_millis(300));
    let started = std::time::Instant::now();
    let err = analyst.summarize(&request(&fx)).unwrap_err();
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert!(matches!(err, AnalystError::Failed(_)), "{err:?}");
    assert!(err.to_string().contains("timed out"), "{err}");
    assert_eq!(fx.git(&["worktree", "list"]).lines().count(), 1);
}

#[test]
fn a_failing_claude_is_reported_with_what_it_said() {
    let fx = feature_branch();
    let fake =
        FakeClaude::replying(r#"{"type":"result","is_error":true,"result":"Rate limit reached"}"#);
    fake.fail_with(1);
    let err = fake.analyst().summarize(&request(&fx)).unwrap_err();
    assert!(matches!(err, AnalystError::Failed(_)), "{err:?}");
    assert!(err.to_string().contains("Rate limit reached"), "{err}");
}

#[test]
fn an_answer_that_does_not_fit_the_schema_is_a_failure() {
    let fx = feature_branch();
    for reply in [
        "not json at all".to_string(),
        r#"{"type":"result","is_error":false,"result":"free text"}"#.to_string(),
        r#"{"type":"result","is_error":false,"structured_output":{"summary":3}}"#.to_string(),
    ] {
        let fake = FakeClaude::replying(&reply);
        let err = fake.analyst().summarize(&request(&fx)).unwrap_err();
        assert!(matches!(err, AnalystError::Failed(_)), "{reply}: {err:?}");
    }
}

#[test]
fn the_prompt_carries_the_pull_request_title_and_description() {
    let fx = feature_branch();
    let fake = FakeClaude::replying(&success("x"));
    let mut req = request(&fx);
    req.pull_request = Some(moonhare::forge::PullRequest {
        number: 7,
        title: "Rework the widgets".into(),
        body: "Because the old ones leaked".into(),
        base_branch: "main".into(),
    });
    fake.analyst().summarize(&req).unwrap();
    let prompt = fake.recorded("stdin");
    assert!(prompt.contains("Rework the widgets"), "{prompt}");
    assert!(prompt.contains("Because the old ones leaked"), "{prompt}");
}

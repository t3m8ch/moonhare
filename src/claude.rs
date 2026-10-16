//! The Analyst over `claude -p` (ADR 0002).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::analyst::{Analyst, AnalystError, Summary, SummaryRequest};

/// The shape the answer must have; `claude` validates it.
const SCHEMA: &str = r#"{"type":"object","properties":{"summary":{"type":"string"}},"required":["summary"],"additionalProperties":false}"#;

/// The only tools the Analyst gets: reading and searching files.
const READ_ONLY_TOOLS: &str = "Read,Grep,Glob";

pub struct ClaudeAnalyst {
    program: PathBuf,
    environment: Vec<(String, String)>,
}

impl ClaudeAnalyst {
    pub fn new() -> ClaudeAnalyst {
        ClaudeAnalyst::with_program("claude")
    }

    /// Use another executable than `claude` found on `PATH`.
    pub fn with_program(program: impl Into<PathBuf>) -> ClaudeAnalyst {
        ClaudeAnalyst {
            program: program.into(),
            environment: std::env::vars().collect(),
        }
    }

    /// Run the child with this environment instead of the current one.
    pub fn with_environment<K: Into<String>, V: Into<String>>(
        mut self,
        vars: impl IntoIterator<Item = (K, V)>,
    ) -> ClaudeAnalyst {
        self.environment = vars
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        self
    }
}

impl Default for ClaudeAnalyst {
    fn default() -> Self {
        Self::new()
    }
}

/// A detached worktree on the head of the Range, removed when dropped. The
/// Analyst reads files there, so the Reviewer's own working copy is never
/// touched.
struct TempWorktree {
    repo: PathBuf,
    path: PathBuf,
    _dir: tempfile::TempDir,
}

impl TempWorktree {
    fn create(repo: &Path, head: &str) -> Result<TempWorktree, AnalystError> {
        let failed = |e: &dyn std::fmt::Display| {
            AnalystError::Failed(format!("cannot create a worktree for the Analyst: {e}"))
        };
        let dir = tempfile::Builder::new()
            .prefix("moonhare-analyst-")
            .tempdir()
            .map_err(|e| failed(&e))?;
        let path = dir.path().join("head");
        let out = Command::new("git")
            .args(["worktree", "add", "--detach", "--quiet"])
            .arg(&path)
            .arg(head)
            .current_dir(repo)
            .output()
            .map_err(|e| failed(&e))?;
        if !out.status.success() {
            return Err(failed(&String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok(TempWorktree {
            repo: repo.to_path_buf(),
            path,
            _dir: dir,
        })
    }
}

impl Drop for TempWorktree {
    fn drop(&mut self) {
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&self.path)
            .current_dir(&self.repo)
            .output();
    }
}

/// The diff goes into the prompt up to this many bytes; the Analyst reads the
/// rest from the worktree.
const DIFF_LIMIT: usize = 100_000;

/// Output of a git command in the clone, empty if it fails.
fn git_text(repo: &Path, args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn prompt(request: &SummaryRequest) -> String {
    let range = format!("{}..{}", request.base, request.head);
    let commits = git_text(&request.repo, &["log", "--format=%h %s", &range]);
    let mut diff = git_text(&request.repo, &["diff", &request.base, &request.head]);
    if diff.len() > DIFF_LIMIT {
        let mut cut = DIFF_LIMIT;
        while !diff.is_char_boundary(cut) {
            cut -= 1;
        }
        diff.truncate(cut);
        diff.push_str("\n[diff truncated: read the files in the working directory]\n");
    }
    let pull_request = request.pull_request.as_ref().map_or(String::new(), |pr| {
        format!(
            "Pull request #{}: {}\n\n{}\n\n",
            pr.number, pr.title, pr.body
        )
    });
    format!(
        "You are the Analyst of a code review tool. Summarize what the change \
         below does and why, for a human who is about to review it. Your working \
         directory holds the files at the head of the change; you may read and \
         search them, nothing else.\n\n\
         Language: {language}\n\n\
         {pull_request}\
         Commits:\n{commits}\n\
         Diff:\n{diff}",
        language = request.language,
    )
}

const NOT_LOGGED_IN: &str = "claude is not logged in: run `claude /login`, then :analyze";

/// What `claude -p --output-format json` printed, as a summary or an error.
fn interpret(stdout: &[u8], stderr: &[u8], success: bool) -> Result<Summary, AnalystError> {
    let reply: Option<serde_json::Value> = serde_json::from_slice(stdout).ok();
    let said = reply
        .as_ref()
        .and_then(|r| r["result"].as_str())
        .map(str::to_string)
        .unwrap_or_else(|| String::from_utf8_lossy(stderr).trim().to_string());
    let errored = !success || reply.as_ref().is_some_and(|r| r["is_error"] == true);
    if errored {
        let lower = said.to_lowercase();
        return Err(
            if lower.contains("not logged in") || lower.contains("/login") {
                AnalystError::NotLoggedIn(NOT_LOGGED_IN.into())
            } else {
                AnalystError::Failed(format!("claude failed: {said}"))
            },
        );
    }
    reply
        .as_ref()
        .and_then(|r| r["structured_output"]["summary"].as_str())
        .map(|text| Summary { text: text.into() })
        .ok_or_else(|| AnalystError::Failed("claude gave an answer without a summary".into()))
}

impl Analyst for ClaudeAnalyst {
    fn summarize(&self, request: &SummaryRequest) -> Result<Summary, AnalystError> {
        let worktree = TempWorktree::create(&request.repo, &request.head)?;
        let mut child = Command::new(&self.program)
            .args(["-p", "--output-format", "json", "--json-schema", SCHEMA])
            .args([
                "--tools",
                READ_ONLY_TOOLS,
                "--allowedTools",
                READ_ONLY_TOOLS,
            ])
            .args(["--permission-mode", "dontAsk", "--no-session-persistence"])
            .env_clear()
            .envs(
                self.environment
                    .iter()
                    .filter(|(name, _)| !name.starts_with("ANTHROPIC_"))
                    .map(|(name, value)| (name, value)),
            )
            .current_dir(&worktree.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    AnalystError::Unavailable(format!(
                        "claude is not installed ({}): the Analyst is unavailable",
                        self.program.display()
                    ))
                } else {
                    AnalystError::Failed(format!("cannot run claude: {e}"))
                }
            })?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(prompt(request).as_bytes());
        }
        let out = child
            .wait_with_output()
            .map_err(|e| AnalystError::Failed(format!("cannot run claude: {e}")))?;
        interpret(&out.stdout, &out.stderr, out.status.success())
    }
}

//! The Analyst over `claude -p` (ADR 0002).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::analyst::{Analyst, AnalystError, Summary, SummaryRequest};

/// The shape the answer must have; `claude` validates it.
const SCHEMA: &str = r#"{"type":"object","properties":{"summary":{"type":"string"}},"required":["summary"],"additionalProperties":false}"#;

/// The only tools the Analyst gets: reading and searching files.
const READ_ONLY_TOOLS: &str = "Read,Grep,Glob";

pub struct ClaudeAnalyst {
    program: PathBuf,
    environment: Vec<(String, String)>,
    timeout: Duration,
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
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Give up on `claude` after this long.
    pub fn with_timeout(mut self, timeout: Duration) -> ClaudeAnalyst {
        self.timeout = timeout;
        self
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
        // Forget worktrees whose directories a crashed run left deleted.
        let _ = Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(repo)
            .output();
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

fn prompt(request: &SummaryRequest, tree: &Path) -> String {
    let range = format!("{}..{}", request.base, request.head);
    let commits = git_text(&request.repo, &["log", "--format=%h %s", &range, "--"]);
    let mut diff = git_text(&request.repo, &["diff", &request.base, &request.head, "--"]);
    if diff.len() > DIFF_LIMIT {
        let mut cut = DIFF_LIMIT;
        while !diff.is_char_boundary(cut) {
            cut -= 1;
        }
        diff.truncate(cut);
        diff.push_str("\n[diff truncated: read the files in the tree]\n");
    }
    let pull_request = request.pull_request.as_ref().map_or(String::new(), |pr| {
        format!(
            "Pull request #{}: {}\n\n{}\n\n",
            pr.number, pr.title, pr.body
        )
    });
    format!(
        "You are the Analyst of a code review tool. Summarize what the change \
         below does and why, for a human who is about to review it. The files at the head of the change are in \
         {tree}; you may read and search them, nothing else.\n\n\
         Language: {language}\n\n\
         {pull_request}\
         Commits:\n{commits}\n\
         Diff:\n{diff}",
        language = request.language,
        tree = tree.display(),
    )
}

/// Variables that would switch `claude` away from the Reviewer's own login
/// (an API key, Bedrock, Vertex) or hand it cloud credentials.
fn is_provider_variable(name: &str) -> bool {
    [
        "ANTHROPIC_",
        "CLAUDE_CODE_USE_",
        "AWS_",
        "CLOUD_ML_",
        "VERTEX_",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
        || name == "GOOGLE_APPLICATION_CREDENTIALS"
}

/// How long `claude` may think before the Analyst gives up.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Feed `input` to the child, collect its output, and kill it when it takes
/// longer than `timeout`.
fn run_with_timeout(
    mut child: Child,
    input: Vec<u8>,
    timeout: Duration,
) -> Result<Output, AnalystError> {
    fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    }
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    if let Some(mut stdin) = child.stdin.take() {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AnalystError::Failed(format!(
                    "claude timed out after {} s",
                    timeout.as_secs().max(1)
                )));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(AnalystError::Failed(format!("cannot run claude: {e}"))),
        }
    };
    let collect = |h: Option<std::thread::JoinHandle<Vec<u8>>>| {
        h.and_then(|h| h.join().ok()).unwrap_or_default()
    };
    Ok(Output {
        status,
        stdout: collect(stdout),
        stderr: collect(stderr),
    })
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
        .map(|text| Summary {
            text: text.into(),
            ..Summary::default()
        })
        .ok_or_else(|| AnalystError::Failed("claude gave an answer without a summary".into()))
}

impl Analyst for ClaudeAnalyst {
    fn summarize(&self, request: &SummaryRequest) -> Result<Summary, AnalystError> {
        // `--` cannot end the options in front of a revision, so refuse
        // anything that git could take for one.
        if [&request.base, &request.head]
            .iter()
            .any(|rev| rev.starts_with('-'))
        {
            return Err(AnalystError::Failed(format!(
                "not a revision: {}..{}",
                request.base, request.head
            )));
        }
        let worktree = TempWorktree::create(&request.repo, &request.head)?;
        let sandbox = tempfile::Builder::new()
            .prefix("moonhare-cwd-")
            .tempdir()
            .map_err(|e| AnalystError::Failed(format!("cannot create a directory: {e}")))?;
        let child = Command::new(&self.program)
            .args(["-p", "--output-format", "json", "--json-schema", SCHEMA])
            .args([
                "--tools",
                READ_ONLY_TOOLS,
                "--allowedTools",
                READ_ONLY_TOOLS,
            ])
            .args(["--permission-mode", "dontAsk", "--no-session-persistence"])
            // The reviewed tree is somebody else's code: nothing in it may
            // configure this process. Settings come from the user only, MCP
            // servers and skills are not loaded, and the process does not
            // start inside the tree (it is only readable through --add-dir).
            .args(["--setting-sources", "user", "--strict-mcp-config"])
            .arg("--disable-slash-commands")
            .arg("--add-dir")
            .arg(&worktree.path)
            .env_clear()
            .envs(
                self.environment
                    .iter()
                    .filter(|(name, _)| !is_provider_variable(name))
                    .map(|(name, value)| (name, value)),
            )
            .current_dir(sandbox.path())
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
        let out = run_with_timeout(
            child,
            prompt(request, &worktree.path).into_bytes(),
            self.timeout,
        )?;
        interpret(&out.stdout, &out.stderr, out.status.success())
    }
}

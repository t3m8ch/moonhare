//! The Analyst over `claude -p` (ADR 0002).

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::analyst::{
    Analyst, AnalystError, AnalystSettings, Diagram, DiagramRequest, DiagramSubject,
    ExplainRequest, ExplainTarget, Explanation, Group, GroupRequest, Grouping, Summary,
    SummaryRequest,
};
use crate::diagram::Room;

/// The shape the answer must have; `claude` validates it.
const SCHEMA: &str = r#"{"type":"object","properties":{"summary":{"type":"string"},"diagram":{"type":"string","description":"Mermaid text, only when the interaction of components changed"}},"required":["summary"],"additionalProperties":false}"#;

/// The shape of the answer to a request for a diagram.
const DIAGRAM_SCHEMA: &str = r#"{"type":"object","properties":{"mermaid":{"type":"string"}},"required":["mermaid"],"additionalProperties":false}"#;

/// Asks for the whole structured answer instead of one field of it.
const WHOLE_ANSWER: &str = "";

/// The shape of the Groups; `claude` validates it.
const GROUPS_SCHEMA: &str = r#"{"type":"object","properties":{"groups":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"description":{"type":"string"},"mechanical":{"type":"boolean"},"files":{"type":"array","items":{"type":"string"}}},"required":["name","description","mechanical","files"],"additionalProperties":false}}},"required":["groups"],"additionalProperties":false}"#;

/// The shape of an explanation.
const EXPLAIN_SCHEMA: &str = r#"{"type":"object","properties":{"explanation":{"type":"string"}},"required":["explanation"],"additionalProperties":false}"#;

/// The only tools the Analyst gets: reading and searching files.
const READ_ONLY_TOOLS: &str = "Read,Grep,Glob";

pub struct ClaudeAnalyst {
    program: PathBuf,
    environment: Vec<(String, String)>,
    timeout: Duration,
    /// The repositories whose stale worktrees are already pruned.
    pruned: Mutex<HashSet<PathBuf>>,
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
            pruned: Default::default(),
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

/// Serializes the git commands that touch `.git/worktrees`. Each of them
/// reads every entry there and fails when a concurrent `add`, `remove` or
/// `prune` is creating or deleting one, and `prune` deletes an entry that an
/// `add` has not finished yet. A process-wide mutex covers the requests of one
/// moonhare, a lock file under moonhare's directory in `.git` covers several
/// moonhares on the same repository.
struct WorktreeLock {
    // Fields drop in order: the file lock goes before the mutex.
    _file: Option<File>,
    _guard: MutexGuard<'static, ()>,
}

impl WorktreeLock {
    fn acquire(repo: &Path) -> WorktreeLock {
        static IN_PROCESS: Mutex<()> = Mutex::new(());
        let guard = IN_PROCESS.lock().unwrap_or_else(PoisonError::into_inner);
        let common = git_text(
            repo,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        );
        let common = common.trim();
        // Without a lock file (no git directory, a read-only repository) the
        // mutex still protects this process's own requests.
        let file = if common.is_empty() {
            None
        } else {
            let dir = Path::new(common).join("moonhare");
            std::fs::create_dir_all(&dir)
                .and_then(|()| {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .truncate(false)
                        .write(true)
                        .open(dir.join("worktrees.lock"))
                })
                .and_then(|file| file.lock().map(|()| file))
                .ok()
        };
        WorktreeLock {
            _file: file,
            _guard: guard,
        }
    }
}

/// How many times a worktree command is tried while a lock of git itself,
/// taken by a git outside moonhare, is in the way.
const WORKTREE_ATTEMPTS: u32 = 5;

/// `git worktree <args>` in `repo` under the [`WorktreeLock`], tried again
/// while it fails on a lock of git itself.
fn git_worktree(repo: &Path, args: &[&OsStr]) -> std::io::Result<Output> {
    let _lock = WorktreeLock::acquire(repo);
    let mut attempt = 1;
    loop {
        let out = Command::new("git")
            .arg("worktree")
            .args(args)
            .current_dir(repo)
            .output()?;
        let locked = String::from_utf8_lossy(&out.stderr).contains(".lock");
        if out.status.success() || !locked || attempt == WORKTREE_ATTEMPTS {
            return Ok(out);
        }
        std::thread::sleep(Duration::from_millis(50 * u64::from(attempt)));
        attempt += 1;
    }
}

/// Forget the worktrees of earlier runs whose directories are gone, once
/// per repository in this process, before its first request.
fn prune_once(repo: &Path, pruned: &Mutex<HashSet<PathBuf>>) {
    let mut pruned = pruned.lock().unwrap_or_else(PoisonError::into_inner);
    if pruned.insert(repo.to_path_buf()) {
        let _ = git_worktree(repo, &["prune".as_ref()]);
    }
}

/// A detached worktree on the head of the Range, removed and unregistered
/// when dropped, also after an error, a timeout or a panic. The Analyst reads
/// files there, so the Reviewer's own working copy is never touched.
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
        // git names the worktree after the last component of its path, so
        // the unique name of the directory makes a unique worktree.
        let path = dir.path().to_path_buf();
        let out = git_worktree(
            repo,
            &[
                "add".as_ref(),
                "--detach".as_ref(),
                "--quiet".as_ref(),
                path.as_os_str(),
                head.as_ref(),
            ],
        )
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
        let _ = git_worktree(
            &self.repo,
            &["remove".as_ref(), "--force".as_ref(), self.path.as_os_str()],
        );
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

/// `diff` cut to `DIFF_LIMIT` bytes, with a note when it was cut.
fn capped(mut diff: String) -> String {
    if diff.len() > DIFF_LIMIT {
        let mut cut = DIFF_LIMIT;
        while !diff.is_char_boundary(cut) {
            cut -= 1;
        }
        diff.truncate(cut);
        diff.push_str("\n[diff truncated: read the files in the tree]\n");
    }
    diff
}

fn prompt(request: &SummaryRequest, tree: &Path) -> String {
    let range = format!("{}..{}", request.base, request.head);
    let commits = git_text(&request.repo, &["log", "--format=%h %s", &range, "--"]);
    let diff = capped(git_text(
        &request.repo,
        &["diff", &request.base, &request.head, "--"],
    ));
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
         If the change alters how components interact, also give `diagram`: a \
         Mermaid sequence diagram or flowchart of that interaction. {rules} \
         Leave `diagram` out otherwise.\n\n\
         Language: {language}\n\n\
         {pull_request}\
         Commits:\n{commits}\n\
         Diff:\n{diff}",
        language = request.language,
        tree = tree.display(),
        rules = diagram_rules(request.room),
    )
}

/// What keeps a diagram readable in the terminal window it is drawn in:
/// boxes side by side cost about 20 columns each.
fn diagram_rules(room: Room) -> String {
    let side_by_side = (room.width / 20).clamp(2, 6);
    format!(
        "It is drawn with box-drawing characters in a terminal window of {} columns \
         and {} rows, so keep it that small: at most {side_by_side} participants or \
         nodes side by side, short labels (a few words, under 20 characters), a \
         flowchart top-down (`flowchart TD`) unless the point is a left-to-right \
         pipeline.",
        room.width, room.height
    )
}

fn diagram_prompt(request: &DiagramRequest, tree: &Path) -> String {
    let (paths, what) = match &request.subject {
        DiagramSubject::File { path } => (vec![path.as_str()], format!("the file {path}")),
        DiagramSubject::Fragment { path, line } => (
            vec![path.as_str()],
            format!("the fragment of {path} around line {line} of its new version"),
        ),
        DiagramSubject::Group { name, files } => (
            files.iter().map(String::as_str).collect(),
            format!(
                "the group of files \"{name}\" ({}) taken together",
                files.join(", ")
            ),
        ),
    };
    let mut args = vec!["diff", &request.base, &request.head, "--"];
    args.extend(paths);
    let diff = capped(git_text(&request.repo, &args));
    format!(
        "You are the Analyst of a code review tool. Draw {what} for a human who \
         is about to review it: a Mermaid sequence diagram if the point is an \
         exchange between components over time, otherwise a flowchart. \
         {rules} Answer with Mermaid text only, in `mermaid`. The files at the head of the \
         change are in {tree}; you may read and search them, nothing else.\n\n\
         Language: {language}\n\n\
         Diff:\n{diff}",
        language = request.language,
        tree = tree.display(),
        rules = diagram_rules(request.room),
    )
}

fn groups_prompt(request: &GroupRequest, tree: &Path) -> String {
    let files: String = request.files.iter().map(|f| format!("- {f}\n")).collect();
    let range = format!("{}..{}", request.base, request.head);
    let commits = git_text(&request.repo, &["log", "--format=%h %s", &range, "--"]);
    let pull_request = request.pull_request.as_ref().map_or(String::new(), |pr| {
        format!(
            "Pull request #{}: {}\n\n{}\n\n",
            pr.number, pr.title, pr.body
        )
    });
    format!(
        "You are the Analyst of a code review tool. Split the files the change \
         below touches into groups that belong together logically, for a human who is \
         about to review it. Every file listed must be in exactly one group. Give each \
         group a short name and a one-line description, mark it mechanical when its changes \
         need no close reading (generated files, lockfiles, bulk renames), and list the \
         groups in the order the human should read them. The files at the head of the \
         change are in {tree}; you may read and search them, nothing else.\n\n\
         Language: {language}\n\n\
         {pull_request}\
         Files:\n{files}\n\
         Commits:\n{commits}",
        language = request.language,
        tree = tree.display(),
    )
}

fn parse_groups(answer: &serde_json::Value) -> Option<Grouping> {
    let text = |g: &serde_json::Value, field: &str| Some(g[field].as_str()?.to_string());
    let groups = answer
        .as_array()?
        .iter()
        .map(|g| {
            Some(Group {
                name: text(g, "name")?,
                description: text(g, "description")?,
                mechanical: g["mechanical"].as_bool()?,
                files: g["files"]
                    .as_array()?
                    .iter()
                    .map(|f| f.as_str().map(String::from))
                    .collect::<Option<_>>()?,
            })
        })
        .collect::<Option<_>>()?;
    Some(Grouping { groups })
}

fn explain_prompt(request: &ExplainRequest, tree: &Path) -> String {
    let (what, path, hunk) = match &request.target {
        ExplainTarget::Hunk { path, text } => ("this hunk", path, format!("Hunk:\n{text}\n")),
        ExplainTarget::File { path } => ("this file", path, String::new()),
    };
    let mut diff = git_text(
        &request.repo,
        &["diff", &request.base, &request.head, "--", path],
    );
    if diff.len() > DIFF_LIMIT {
        let mut cut = DIFF_LIMIT;
        while !diff.is_char_boundary(cut) {
            cut -= 1;
        }
        diff.truncate(cut);
        diff.push_str("\n[diff truncated: read the file in the tree]\n");
    }
    let pull_request = request.pull_request.as_ref().map_or(String::new(), |pr| {
        format!(
            "Pull request #{}: {}\n\n{}\n\n",
            pr.number, pr.title, pr.body
        )
    });
    format!(
        "You are the Analyst of a code review tool. Explain {what} of a change \
         to a human who is reviewing it: what it does, why it is probably \
         there, and anything surprising. The files at the head of the change \
         are in {tree}; you may read and search them, nothing else.\n\n\
         Language: {language}\n\n\
         {pull_request}\
         File: {path}\n\
         {hunk}\
         Diff of the file:\n{diff}",
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

/// What `claude -p --output-format json` printed: the text of the `field` of
/// the structured answer, or an error.
fn interpret(
    stdout: &[u8],
    stderr: &[u8],
    success: bool,
    field: &str,
) -> Result<serde_json::Value, AnalystError> {
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
        .map(|r| match field {
            WHOLE_ANSWER => r["structured_output"].clone(),
            field => r["structured_output"][field].clone(),
        })
        .filter(|v| !v.is_null())
        .ok_or_else(|| AnalystError::Failed(format!("claude gave an answer without a {field}")))
}

impl ClaudeAnalyst {
    /// Run `claude -p` over the Range with the read-only worktree at `head`,
    /// and return the `field` of its structured answer.
    fn ask(
        &self,
        repo: &Path,
        base: &str,
        head: &str,
        (schema, field): (&str, &str),
        settings: &AnalystSettings,
        prompt: impl FnOnce(&Path) -> String,
    ) -> Result<serde_json::Value, AnalystError> {
        // `--` cannot end the options in front of a revision, so refuse
        // anything that git could take for one.
        if [base, head].iter().any(|rev| rev.starts_with('-')) {
            return Err(AnalystError::Failed(format!(
                "not a revision: {base}..{head}"
            )));
        }
        prune_once(repo, &self.pruned);
        let worktree = TempWorktree::create(repo, head)?;
        let sandbox = tempfile::Builder::new()
            .prefix("moonhare-cwd-")
            .tempdir()
            .map_err(|e| AnalystError::Failed(format!("cannot create a directory: {e}")))?;
        let child = Command::new(&self.program)
            .args(["-p", "--output-format", "json", "--json-schema", schema])
            .args([
                "--tools",
                READ_ONLY_TOOLS,
                "--allowedTools",
                READ_ONLY_TOOLS,
            ])
            .args(["--permission-mode", "dontAsk", "--no-session-persistence"])
            .args(
                settings
                    .effort
                    .iter()
                    .flat_map(|e| ["--effort", e.as_str()]),
            )
            .args(settings.model.iter().flat_map(|m| ["--model", m.as_str()]))
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
        let out = run_with_timeout(child, prompt(&worktree.path).into_bytes(), self.timeout)?;
        interpret(&out.stdout, &out.stderr, out.status.success(), field)
    }
}

/// The answer's `field` as text.
fn as_text(answer: serde_json::Value, field: &str) -> Result<String, AnalystError> {
    answer
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| AnalystError::Failed(format!("claude gave an answer without a {field}")))
}

impl Analyst for ClaudeAnalyst {
    fn summarize(&self, request: &SummaryRequest) -> Result<Summary, AnalystError> {
        self.ask(
            &request.repo,
            &request.base,
            &request.head,
            (SCHEMA, WHOLE_ANSWER),
            &request.settings,
            |tree| prompt(request, tree),
        )
        .and_then(|answer| {
            Ok(Summary {
                text: as_text(answer["summary"].clone(), "summary")?,
                diagram: answer["diagram"].as_str().map(str::to_string),
            })
        })
    }

    fn diagram(&self, request: &DiagramRequest) -> Result<Diagram, AnalystError> {
        self.ask(
            &request.repo,
            &request.base,
            &request.head,
            (DIAGRAM_SCHEMA, "mermaid"),
            &request.settings,
            |tree| diagram_prompt(request, tree),
        )
        .and_then(|a| as_text(a, "mermaid"))
        .map(|mermaid| Diagram { mermaid })
    }

    fn explain(&self, request: &ExplainRequest) -> Result<Explanation, AnalystError> {
        self.ask(
            &request.repo,
            &request.base,
            &request.head,
            (EXPLAIN_SCHEMA, "explanation"),
            &request.settings,
            |tree| explain_prompt(request, tree),
        )
        .and_then(|a| as_text(a, "explanation"))
        .map(|text| Explanation { text })
    }

    fn group(&self, request: &GroupRequest) -> Result<Grouping, AnalystError> {
        let answer = self.ask(
            &request.repo,
            &request.base,
            &request.head,
            (GROUPS_SCHEMA, "groups"),
            &request.settings,
            |tree| groups_prompt(request, tree),
        )?;
        parse_groups(&answer)
            .ok_or_else(|| AnalystError::Failed("claude gave an answer without groups".into()))
    }
}

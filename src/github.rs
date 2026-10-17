//! GitHub as a [`Forge`], through the `gh` command-line client.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::forge::{Forge, ForgeError, PullRequest};

pub struct GitHub {
    program: PathBuf,
    /// How long `gh pr view` may take before the Forge counts as unreachable.
    timeout: Duration,
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

impl GitHub {
    pub fn new() -> GitHub {
        GitHub::with_program("gh")
    }

    /// Use another executable than `gh` found on `PATH`.
    pub fn with_program(program: impl Into<PathBuf>) -> GitHub {
        GitHub {
            program: program.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Give up on `gh pr view` after `timeout`.
    pub fn with_timeout(mut self, timeout: Duration) -> GitHub {
        self.timeout = timeout;
        self
    }
}

const PR_FIELDS: &str = "number,title,body,baseRefName";

impl GitHub {
    fn spawn_error(&self, e: &std::io::Error) -> ForgeError {
        if e.kind() == std::io::ErrorKind::NotFound {
            ForgeError::Unavailable(format!(
                "gh is not installed ({}): pull request details are unavailable",
                self.program.display()
            ))
        } else {
            ForgeError::Failed(format!("cannot run gh: {e}"))
        }
    }

    fn view(&self, selector: &str) -> Result<PullRequest, ForgeError> {
        let mut command = Command::new(&self.program);
        command.args(["pr", "view", "--json", PR_FIELDS, "--", selector]);
        // A `gh` that hangs must not keep the pull request awaited for ever.
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(command.output());
        });
        let out = match receiver.recv_timeout(self.timeout) {
            Ok(out) => out.map_err(|e| self.spawn_error(&e))?,
            Err(_) => {
                return Err(ForgeError::Network(format!(
                    "gh did not answer in {}s, check the network",
                    self.timeout.as_secs()
                )))
            }
        };
        if !out.status.success() {
            return Err(classify(&String::from_utf8_lossy(&out.stderr)));
        }
        parse_pull_request(&out.stdout)
    }
}

/// Turn what `gh` said on failure into an error the reviewer can act on.
fn classify(stderr: &str) -> ForgeError {
    let stderr = stderr.trim();
    if stderr.contains("no pull requests found")
        || stderr.contains("Could not resolve to a PullRequest")
    {
        ForgeError::NotFound(stderr.to_string())
    } else if [
        "error connecting to",
        "dial tcp",
        "Could not resolve host",
        "timeout",
    ]
    .iter()
    .any(|m| stderr.contains(m))
    {
        ForgeError::Network(format!(
            "cannot reach GitHub, check the network: {}",
            stderr.lines().next().unwrap_or_default()
        ))
    } else {
        ForgeError::Failed(format!("gh failed: {stderr}"))
    }
}

fn parse_pull_request(json: &[u8]) -> Result<PullRequest, ForgeError> {
    let bad =
        |e: &dyn std::fmt::Display| ForgeError::Failed(format!("unexpected answer from gh: {e}"));
    let v: serde_json::Value = serde_json::from_slice(json).map_err(|e| bad(&e))?;
    let text = |key: &str| v[key].as_str().map(str::to_string);
    Ok(PullRequest {
        number: v["number"].as_u64().ok_or_else(|| bad(&"no number"))?,
        title: text("title").ok_or_else(|| bad(&"no title"))?,
        body: text("body").unwrap_or_default(),
        base_branch: text("baseRefName").ok_or_else(|| bad(&"no baseRefName"))?,
    })
}

impl Default for GitHub {
    fn default() -> Self {
        GitHub::new()
    }
}

impl Forge for GitHub {
    fn pull_request_for_branch(&self, branch: &str) -> Result<Option<PullRequest>, ForgeError> {
        match self.view(branch) {
            Ok(pr) => Ok(Some(pr)),
            Err(ForgeError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn pull_request(&self, number: u64) -> Result<PullRequest, ForgeError> {
        self.view(&number.to_string()).map_err(|e| match e {
            ForgeError::NotFound(_) => ForgeError::NotFound(format!("no pull request #{number}")),
            other => other,
        })
    }

    fn fetch_pull_request_head(&self, repo: &Path, number: u64) -> Result<String, ForgeError> {
        // GitHub advertises every pull request head as `refs/pull/N/head`.
        // Fetching it into a ref of our own creates no branch and leaves the
        // working copy alone.
        let reference = format!("refs/moonhare/pr/{number}");
        let out = Command::new("git")
            .args(["fetch", "--quiet", "--no-tags", "origin"])
            .arg(format!("+refs/pull/{number}/head:{reference}"))
            .current_dir(repo)
            .output()
            .map_err(|e| ForgeError::Failed(format!("cannot run git: {e}")))?;
        if out.status.success() {
            return Ok(reference);
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        Err(if stderr.contains("couldn't find remote ref") {
            ForgeError::NotFound(format!("no pull request #{number} on origin"))
        } else if ["Could not resolve host", "unable to access", "Connection"]
            .iter()
            .any(|m| stderr.contains(m))
        {
            ForgeError::Network(format!(
                "cannot reach origin, check the network: {}",
                stderr.lines().next().unwrap_or_default()
            ))
        } else {
            ForgeError::Failed(format!(
                "fetching the pull request failed: {}",
                stderr.trim()
            ))
        })
    }
}

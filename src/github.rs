//! GitHub as a [`Forge`], through the `gh` command-line client.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::forge::{Forge, ForgeError, PullRequest};

pub struct GitHub {
    program: PathBuf,
    /// The clone `gh` is asked about; the current directory when `None`.
    dir: Option<PathBuf>,
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
            dir: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Ask about the clone at `dir` instead of the current directory.
    pub fn in_dir(mut self, dir: impl Into<PathBuf>) -> GitHub {
        self.dir = Some(dir.into());
        self
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

    /// Run `command`; a `gh` that outlives the timeout is killed, so that a
    /// hung one neither keeps the pull request awaited nor outlives moonhare.
    fn run_with_timeout(&self, command: &mut Command) -> Result<std::process::Output, ForgeError> {
        use std::io::Read;
        use std::process::Stdio;
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| self.spawn_error(&e))?;
        fn drain(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_end(&mut bytes);
                }
                bytes
            })
        }
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let deadline = std::time::Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ForgeError::Network(format!(
                        "gh did not answer in {}s, check the network",
                        self.timeout.as_secs()
                    )));
                }
                Err(e) => return Err(ForgeError::Failed(format!("cannot run gh: {e}"))),
            }
        };
        Ok(std::process::Output {
            status,
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        })
    }

    fn view(&self, selector: &str) -> Result<PullRequest, ForgeError> {
        self.check_github_remote()?;
        let out = self.run_with_timeout(
            self.command(&self.program)
                .args(["pr", "view", "--json", PR_FIELDS, "--", selector]),
        )?;
        if !out.status.success() {
            return Err(classify(&String::from_utf8_lossy(&out.stderr)));
        }
        parse_pull_request(&out.stdout)
    }
}

impl GitHub {
    /// `program`, run in the clone asked about.
    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        if let Some(dir) = &self.dir {
            command.current_dir(dir);
        }
        command
    }

    /// Whether some remote of the clone is on GitHub. Without one there is
    /// nothing to ask, and `gh` is not even started.
    fn check_github_remote(&self) -> Result<(), ForgeError> {
        // If git itself cannot say, let `gh` find out.
        let Ok(out) = self.command("git").args(["remote", "-v"]).output() else {
            return Ok(());
        };
        if !out.status.success() {
            return Ok(());
        }
        let listing = String::from_utf8_lossy(&out.stdout);
        let urls: Vec<&str> = listing
            .lines()
            .filter_map(|line| line.split_whitespace().nth(1))
            .collect();
        if urls.is_empty() {
            return Err(ForgeError::Unavailable(
                "the repository has no git remote, so no GitHub pull request is looked up".into(),
            ));
        }
        let known = known_hosts();
        if urls
            .iter()
            .filter_map(|url| remote_host(url))
            .any(|host| is_github_host(&host, &known))
        {
            Ok(())
        } else {
            Err(ForgeError::Unavailable(
                "no git remote points to GitHub, so no pull request is looked up".into(),
            ))
        }
    }
}

/// The host of a remote URL, if it names one: `https://host/…`,
/// `ssh://user@host:port/…` or the scp-like `user@host:path`. A path on disk
/// has none.
fn remote_host(url: &str) -> Option<String> {
    let authority = match url.split_once("://") {
        Some((_, rest)) => rest.split('/').next().unwrap_or_default(),
        None => {
            let (before, _) = url.split_once(':')?;
            if before.contains('/') {
                return None;
            }
            before
        }
    };
    let host = authority.rsplit('@').next().unwrap_or_default();
    let host = host.split(':').next().unwrap_or_default();
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// GitHub itself, a host `gh` is logged into, or a name that says GitHub
/// (an SSH alias such as `github.com-work`, a GitHub Enterprise server).
/// A wrong guess only costs one run of `gh`.
fn is_github_host(host: &str, known: &[String]) -> bool {
    host.contains("github") || known.iter().any(|k| k == host)
}

/// The hosts `gh` knows: `$GH_HOST` and those in its `hosts.yml`.
fn known_hosts() -> Vec<String> {
    let mut hosts: Vec<String> = std::env::var("GH_HOST").ok().into_iter().collect();
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let config = env("GH_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| env("XDG_CONFIG_HOME").map(|d| PathBuf::from(d).join("gh")))
        .or_else(|| env("HOME").map(|d| PathBuf::from(d).join(".config/gh")));
    if let Some(text) = config.and_then(|d| std::fs::read_to_string(d.join("hosts.yml")).ok()) {
        // Every host is a key at the top level of the YAML.
        hosts.extend(
            text.lines()
                .filter(|l| !l.starts_with([' ', '\t', '#', '-']))
                .filter_map(|l| l.trim_end().strip_suffix(':'))
                .map(|h| h.trim_matches(['"', '\'']).to_ascii_lowercase()),
        );
    }
    hosts
}

/// Turn what `gh` said on failure into an error the reviewer can act on.
fn classify(stderr: &str) -> ForgeError {
    let stderr = stderr.trim();
    if stderr.contains("no pull requests found")
        || stderr.contains("Could not resolve to a PullRequest")
    {
        ForgeError::NotFound(stderr.to_string())
    } else if [
        "no git remotes found",
        "point to a known GitHub host",
        "not logged into any GitHub hosts",
        "To get started with GitHub CLI",
    ]
    .iter()
    .any(|m| stderr.contains(m))
    {
        ForgeError::Unavailable(format!(
            "gh has no GitHub to ask: {}",
            stderr.lines().next().unwrap_or_default()
        ))
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
            ForgeError::Unavailable(m) => {
                ForgeError::Unavailable(format!("cannot open pull request #{number}: {m}"))
            }
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

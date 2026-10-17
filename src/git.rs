//! Read-only access to a git repository.
//!
//! Everything here shells out to plumbing commands that only read: no
//! checkout, no index refresh, no ref updates. The reviewer's branches and
//! working copy are never touched.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::mechanical::{self, Kind};

#[derive(Debug)]
pub struct GitError(pub String);

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GitError {}

/// How a file differs between the two ends of a Range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Added,
    Modified,
    Deleted,
    /// Moved to `path` from `old_path`, possibly with edits.
    Renamed,
}

impl Status {
    pub fn letter(self) -> char {
        match self {
            Status::Added => 'A',
            Status::Modified => 'M',
            Status::Deleted => 'D',
            Status::Renamed => 'R',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub status: Status,
    /// Where the file was before, for a rename.
    pub old_path: Option<String>,
    /// Set for a Mechanical file: it is shown collapsed and listed last.
    pub mechanical: Option<Kind>,
}

impl ChangedFile {
    /// The path of the file at the base of the Range.
    pub fn base_path(&self) -> &str {
        self.old_path.as_deref().unwrap_or(&self.path)
    }
}

/// The Range under review: from the merge-base with the default branch up to
/// the head.
#[derive(Debug, Clone)]
pub struct Range {
    /// Name of the head: the current branch, or `HEAD` when detached.
    pub head_name: String,
    /// Name of the default branch the base was computed against.
    pub base_name: String,
    pub base: String,
    pub head: String,
}

/// One commit of the Range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
}

/// What the reviewer asked to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The branch checked out in the clone.
    Current,
    /// A local branch by name, measured against the default branch.
    Branch(String),
    /// An explicit `base..head`; each end is any revision.
    Range { base: String, head: String },
}

impl Target {
    /// Interpret the command-line argument: nothing, a branch, or `base..head`.
    pub fn from_arg(arg: Option<&str>) -> Target {
        match arg {
            None => Target::Current,
            Some(a) => match a.split_once("..") {
                Some((base, head)) => Target::Range {
                    base: base.to_string(),
                    head: head.to_string(),
                },
                None => Target::Branch(a.to_string()),
            },
        }
    }
}

#[derive(Clone)]
pub struct Repo {
    root: PathBuf,
}

impl Repo {
    /// Open the repository that contains `path`.
    pub fn open(path: &Path) -> Result<Repo, GitError> {
        let probe = Repo {
            root: path.to_path_buf(),
        };
        let top = probe
            .run(&["rev-parse", "--show-toplevel"])
            .map_err(|_| GitError(format!("{} is not inside a git repository", path.display())))?;
        Ok(Repo {
            root: PathBuf::from(top.trim_end()),
        })
    }

    /// The top of the working tree.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn run_bytes(&self, args: &[&str]) -> Result<Vec<u8>, GitError> {
        let out = Command::new("git")
            .arg("--no-optional-locks")
            .args(args)
            .current_dir(&self.root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .map_err(|e| GitError(format!("cannot run git: {e}")))?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(GitError(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

    /// Like [`run_bytes`](Self::run_bytes), with `input` written to git's stdin.
    pub(crate) fn run_bytes_stdin(&self, args: &[&str], input: &[u8]) -> Result<Vec<u8>, GitError> {
        use std::io::Write;
        use std::process::Stdio;
        let mut child = Command::new("git")
            .arg("--no-optional-locks")
            .args(args)
            .current_dir(&self.root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| GitError(format!("cannot run git: {e}")))?;
        // Git answers while it still reads: write on a thread of its own, or
        // both sides could wait for the other to empty a full pipe.
        let stdin = child.stdin.take();
        let out = std::thread::scope(|scope| {
            scope.spawn(move || {
                if let Some(mut stdin) = stdin {
                    let _ = stdin.write_all(input);
                }
            });
            child.wait_with_output()
        })
        .map_err(|e| GitError(format!("cannot run git: {e}")))?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(GitError(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

    pub(crate) fn run(&self, args: &[&str]) -> Result<String, GitError> {
        self.run_bytes(args)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    fn resolves(&self, rev: &str) -> bool {
        self.run(&["rev-parse", "--verify", "-q", &format!("{rev}^{{commit}}")])
            .is_ok()
    }

    /// The branch the Range is measured against: `origin/HEAD` if known,
    /// otherwise a local `main`, `master` or `trunk`.
    fn default_branch(&self) -> Result<String, GitError> {
        if let Ok(r) = self.run(&["symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD"]) {
            let r = r.trim().to_string();
            if self.resolves(&r) {
                return Ok(r);
            }
        }
        for name in ["main", "master", "trunk"] {
            if self.resolves(&format!("refs/heads/{name}")) {
                return Ok(name.to_string());
            }
        }
        Err(GitError(
            "cannot find the default branch (origin/HEAD, main, master or trunk)".into(),
        ))
    }

    pub fn commit_id(&self, rev: &str) -> Result<String, GitError> {
        if !self.resolves(rev) {
            return Err(GitError(format!("unknown revision: {rev}")));
        }
        let id = self.run(&["rev-parse", "--verify", &format!("{rev}^{{commit}}")])?;
        Ok(id.trim().to_string())
    }

    /// The Range a [`Target`] names.
    pub fn range_for(&self, target: &Target) -> Result<Range, GitError> {
        match target {
            Target::Current => self.current_range(),
            Target::Branch(name) => {
                let full = format!("refs/heads/{name}");
                if !self.resolves(&full) {
                    return Err(GitError(format!("no such branch: {name}")));
                }
                self.branch_range(name, &full)
            }
            Target::Range { base: b, head: h } => Ok(Range {
                head_name: h.clone(),
                base_name: b.clone(),
                base: self.commit_id(b)?,
                head: self.commit_id(h)?,
            }),
        }
    }

    /// The Range of the current branch: merge-base with the default branch up
    /// to HEAD.
    pub fn current_range(&self) -> Result<Range, GitError> {
        let head_name = self
            .run(&["symbolic-ref", "-q", "--short", "HEAD"])
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "HEAD".to_string());
        self.branch_range(&head_name, "HEAD")
    }

    /// The Range of a head that is not a local branch (a pull request head
    /// fetched into a ref of our own). It is measured against `base_hint`, the
    /// branch the Forge says it targets, when that exists here (as is or on
    /// `origin`), otherwise against the default branch.
    pub fn range_for_head(
        &self,
        head_name: &str,
        rev: &str,
        base_hint: Option<&str>,
    ) -> Result<Range, GitError> {
        let base_name = base_hint.and_then(|b| {
            [format!("origin/{b}"), format!("refs/heads/{b}")]
                .into_iter()
                .find(|c| self.resolves(c))
        });
        self.branch_range_against(head_name, rev, base_name)
    }

    /// Merge-base of `rev` with the default branch, up to `rev`.
    fn branch_range(&self, head_name: &str, rev: &str) -> Result<Range, GitError> {
        self.branch_range_against(head_name, rev, None)
    }

    fn branch_range_against(
        &self,
        head_name: &str,
        rev: &str,
        base_name: Option<String>,
    ) -> Result<Range, GitError> {
        let head = self.run(&["rev-parse", "--verify", &format!("{rev}^{{commit}}")])?;
        let head = head.trim().to_string();
        let head_name = head_name.to_string();
        let base_name = match base_name {
            Some(b) => b,
            None => self.default_branch()?,
        };
        let base = self.run(&["merge-base", &base_name, &head])?;
        Ok(Range {
            head_name,
            base_name,
            base: base.trim().to_string(),
            head,
        })
    }

    /// Commits of the Range, oldest first. A merge commit counts as one commit
    /// relative to its first parent.
    pub fn commits(&self, base: &str, head: &str) -> Result<Vec<Commit>, GitError> {
        let out = self.run(&[
            "rev-list",
            "--reverse",
            "--first-parent",
            "--format=%s",
            &format!("{base}..{head}"),
        ])?;
        let mut commits = Vec::new();
        let mut lines = out.lines();
        while let (Some(header), Some(subject)) = (lines.next(), lines.next()) {
            if let Some(sha) = header.strip_prefix("commit ") {
                commits.push(Commit {
                    sha: sha.to_string(),
                    subject: subject.to_string(),
                });
            }
        }
        Ok(commits)
    }

    /// How many commits lie in `base..head`.
    pub fn commit_count(&self, base: &str, head: &str) -> Result<usize, GitError> {
        self.count_commits(&["rev-list", "--count", &format!("{base}..{head}")])
    }

    /// How many merge commits lie in `base..head`.
    pub fn merge_commit_count(&self, base: &str, head: &str) -> Result<usize, GitError> {
        self.count_commits(&[
            "rev-list",
            "--merges",
            "--count",
            &format!("{base}..{head}"),
        ])
    }

    fn count_commits(&self, args: &[&str]) -> Result<usize, GitError> {
        let out = self.run(args)?;
        out.trim()
            .parse()
            .map_err(|_| GitError(format!("unexpected rev-list output: {out}")))
    }

    /// Files whose content differs between `base` and `head`.
    pub fn changed_files(&self, base: &str, head: &str) -> Result<Vec<ChangedFile>, GitError> {
        let raw = self.run_bytes(&["diff-tree", "-r", "-M", "--name-status", "-z", base, head])?;
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        let mut parts = raw.split(|&b| b == 0).filter(|p| !p.is_empty());
        let mut files = Vec::new();
        while let (Some(status), Some(path)) = (parts.next(), parts.next()) {
            let (status, path, old_path) = match status[0] {
                b'A' => (Status::Added, text(path), None),
                b'D' => (Status::Deleted, text(path), None),
                // A rename lists the old path, then the new one.
                b'R' => match parts.next() {
                    Some(new) => (Status::Renamed, text(new), Some(text(path))),
                    None => break,
                },
                _ => (Status::Modified, text(path), None),
            };
            files.push(ChangedFile {
                path,
                status,
                old_path,
                mechanical: None,
            });
        }
        mechanical::classify(self, base, head, &mut files);
        Ok(files)
    }

    /// Content of the blob `id`.
    pub fn blob(&self, id: &str) -> Result<Vec<u8>, GitError> {
        self.run_bytes(&["cat-file", "blob", id])
    }

    /// Content of `path` at `rev`.
    pub fn file_at(&self, rev: &str, path: &str) -> Result<Vec<u8>, GitError> {
        self.run_bytes(&["cat-file", "blob", &format!("{rev}:{path}")])
    }
}

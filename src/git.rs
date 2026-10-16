//! Read-only access to a git repository.
//!
//! Everything here shells out to plumbing commands that only read: no
//! checkout, no index refresh, no ref updates. The reviewer's branches and
//! working copy are never touched.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

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
}

impl Status {
    pub fn letter(self) -> char {
        match self {
            Status::Added => 'A',
            Status::Modified => 'M',
            Status::Deleted => 'D',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub status: Status,
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

    fn run_bytes(&self, args: &[&str]) -> Result<Vec<u8>, GitError> {
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

    fn run(&self, args: &[&str]) -> Result<String, GitError> {
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

    /// The Range of the current branch: merge-base with the default branch up
    /// to HEAD.
    pub fn current_range(&self) -> Result<Range, GitError> {
        let head = self.run(&["rev-parse", "--verify", "HEAD^{commit}"])?;
        let head = head.trim().to_string();
        let head_name = self
            .run(&["symbolic-ref", "-q", "--short", "HEAD"])
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "HEAD".to_string());
        let base_name = self.default_branch()?;
        let base = self.run(&["merge-base", &base_name, &head])?;
        Ok(Range {
            head_name,
            base_name,
            base: base.trim().to_string(),
            head,
        })
    }

    /// Files whose content differs between `base` and `head`.
    pub fn changed_files(&self, base: &str, head: &str) -> Result<Vec<ChangedFile>, GitError> {
        let raw = self.run_bytes(&[
            "diff-tree",
            "-r",
            "--no-renames",
            "--name-status",
            "-z",
            base,
            head,
        ])?;
        let mut parts = raw.split(|&b| b == 0).filter(|p| !p.is_empty());
        let mut files = Vec::new();
        while let (Some(status), Some(path)) = (parts.next(), parts.next()) {
            let status = match status[0] {
                b'A' => Status::Added,
                b'D' => Status::Deleted,
                _ => Status::Modified,
            };
            files.push(ChangedFile {
                path: String::from_utf8_lossy(path).into_owned(),
                status,
            });
        }
        Ok(files)
    }

    /// Content of `path` at `rev`.
    pub fn file_at(&self, rev: &str, path: &str) -> Result<Vec<u8>, GitError> {
        self.run_bytes(&["cat-file", "blob", &format!("{rev}:{path}")])
    }
}

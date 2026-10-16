//! The Review's record of what is Reviewed.
//!
//! A mark belongs to a File change and is keyed by its content (the path and
//! the blob ids it turns one version into another), not by the commit that
//! carries it (ADR 0003).

use std::collections::HashSet;

use crate::git::{GitError, Repo};

/// The all-zero object id git prints for "no such blob".
const NULL_BLOB: &str = "0000000000000000000000000000000000000000";

/// One commit's modification of one file, with the blobs on both sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub old_blob: String,
    pub new_blob: String,
}

/// How much of a file is Reviewed in a Scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Unreviewed,
    Partial,
    Reviewed,
}

/// The file changes made by one commit (or any `base..head` pair), by path.
pub fn file_changes(repo: &Repo, base: &str, head: &str) -> Result<Vec<FileChange>, GitError> {
    let raw = repo.run_bytes(&[
        "diff-tree",
        "-r",
        "-M",
        "--raw",
        "--no-abbrev",
        "-z",
        base,
        head,
    ])?;
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    let mut parts = raw.split(|&b| b == 0).filter(|p| !p.is_empty());
    let mut changes = Vec::new();
    while let (Some(meta), Some(path)) = (parts.next(), parts.next()) {
        let meta = text(meta);
        // ":<mode> <mode> <old blob> <new blob> <status>"
        let fields: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
        let (Some(old), Some(new), Some(status)) = (fields.get(2), fields.get(3), fields.get(4))
        else {
            break;
        };
        let (path, old_path) = if status.starts_with('R') || status.starts_with('C') {
            match parts.next() {
                Some(new_path) => (text(new_path), Some(text(path))),
                None => break,
            }
        } else {
            (text(path), None)
        };
        changes.push(FileChange {
            path,
            old_path,
            old_blob: old.to_string(),
            new_blob: new.to_string(),
        });
    }
    Ok(changes)
}

/// The blobs of a change worth keeping alive.
pub fn blobs(change: &FileChange) -> impl Iterator<Item = &str> {
    [change.old_blob.as_str(), change.new_blob.as_str()]
        .into_iter()
        .filter(|b| *b != NULL_BLOB)
}

/// What identifies a File change: its path and the blobs on both sides.
type Key = (String, String, String);

/// Which File changes are Reviewed.
#[derive(Debug, Default)]
pub struct Review {
    marked: HashSet<Key>,
}

impl Review {
    pub fn is_marked(&self, change: &FileChange) -> bool {
        self.marked.contains(&Self::key(change))
    }

    fn key(change: &FileChange) -> Key {
        (
            change.path.clone(),
            change.old_blob.clone(),
            change.new_blob.clone(),
        )
    }

    pub fn mark(&mut self, changes: &[FileChange]) {
        self.marked.extend(changes.iter().map(Self::key));
    }

    pub fn unmark(&mut self, changes: &[FileChange]) {
        for c in changes {
            self.marked.remove(&Self::key(c));
        }
    }

    pub fn state(&self, changes: &[FileChange]) -> Mark {
        let done = changes.iter().filter(|c| self.is_marked(c)).count();
        match done {
            0 => Mark::Unreviewed,
            n if n == changes.len() => Mark::Reviewed,
            _ => Mark::Partial,
        }
    }
}

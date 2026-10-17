//! The Review's record of what is Reviewed.
//!
//! A mark belongs to a File change and is keyed by its content (the path and
//! the blob ids it turns one version into another), not by the commit that
//! carries it (ADR 0003).

use std::collections::HashSet;
use std::path::PathBuf;

use crate::git::{GitError, Repo};

/// The all-zero object id git prints for "no such blob".
pub const NULL_BLOB: &str = "0000000000000000000000000000000000000000";

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

/// Which File changes are Reviewed, kept in a file inside the clone's `.git`
/// so that it outlives the process and is never shared with other clones.
#[derive(Debug)]
pub struct Review {
    /// Identifies the Review among the clone's: derived from its name.
    id: String,
    /// `.git/moonhare/reviews/<id>`.
    dir: PathBuf,
    marked: HashSet<Key>,
}

/// 128 bits of FNV-1a over `text`, as 32 hex digits.
fn digest(text: &str) -> String {
    let mut out = String::new();
    for seed in [0xcbf2_9ce4_8422_2325_u64, 0x8422_2325_cbf2_9ce4_u64] {
        let mut h = seed;
        for b in text.bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
        out.push_str(&format!("{h:016x}"));
    }
    out
}

fn io_error(what: &str, e: std::io::Error) -> String {
    format!("{what}: {e}")
}

impl Review {
    /// The Review named `name` in the clone of `repo`: the marks saved by an
    /// earlier run, or none.
    pub fn open(repo: &Repo, name: &str) -> Result<Review, GitError> {
        let common = repo.run(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
        let id = digest(name);
        let dir = PathBuf::from(common.trim_end())
            .join("moonhare")
            .join("reviews")
            .join(&id);
        let mut marked = HashSet::new();
        if let Ok(saved) = std::fs::read(dir.join("marks")) {
            for record in saved.split(|&b| b == 0).filter(|r| !r.is_empty()) {
                let record = String::from_utf8_lossy(record);
                let mut fields = record.splitn(3, ' ');
                if let (Some(old), Some(new), Some(path)) =
                    (fields.next(), fields.next(), fields.next())
                {
                    marked.insert((path.to_string(), old.to_string(), new.to_string()));
                }
            }
        }
        Ok(Review { id, dir, marked })
    }

    /// The directory the Review keeps its records in.
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    pub fn is_marked(&self, change: &FileChange) -> bool {
        self.marked.contains(&Self::key(change))
    }

    fn save(&self) -> Result<(), String> {
        let mut data = Vec::new();
        let mut records: Vec<_> = self.marked.iter().collect();
        records.sort();
        for (path, old, new) in records {
            data.extend(format!("{old} {new} {path}").bytes());
            data.push(0);
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| io_error("cannot save the Review", e))?;
        let tmp = self.dir.join("marks.tmp");
        std::fs::write(&tmp, data).map_err(|e| io_error("cannot save the Review", e))?;
        std::fs::rename(&tmp, self.dir.join("marks"))
            .map_err(|e| io_error("cannot save the Review", e))
    }

    /// Keep the versions behind `changes` out of reach of `git gc`.
    fn pin(&self, repo: &Repo, changes: &[FileChange]) -> Result<(), String> {
        for blob in changes.iter().flat_map(blobs) {
            // A submodule's commit is not ours to keep; it may not even exist.
            if repo.run(&["cat-file", "-e", blob]).is_err() {
                continue;
            }
            let name = format!("refs/moonhare/{}/{blob}", self.id);
            repo.run(&["update-ref", &name, blob])
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Mark `changes` Reviewed and save, pinning their blobs first.
    pub fn mark(&mut self, repo: &Repo, changes: &[FileChange]) -> Result<(), String> {
        self.pin(repo, changes)?;
        self.marked.extend(changes.iter().map(Self::key));
        self.save()
    }

    /// Take the marks of `changes` back and save. The pins stay until a reset.
    pub fn unmark(&mut self, changes: &[FileChange]) -> Result<(), String> {
        for c in changes {
            self.marked.remove(&Self::key(c));
        }
        self.save()
    }

    /// Forget every mark and release every pin of the Review.
    pub fn reset(&mut self, repo: &Repo) -> Result<(), String> {
        self.marked.clear();
        // Only the marks go: Comments live in the same directory and are
        // the Reviewer's own words, which a reset must not destroy.
        match std::fs::remove_file(self.dir.join("marks")) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error("cannot reset the Review", e)),
        }
        let prefix = format!("refs/moonhare/{}/", self.id);
        let refs = repo
            .run(&["for-each-ref", "--format=%(refname)", &prefix])
            .map_err(|e| e.to_string())?;
        for name in refs.lines() {
            repo.run(&["update-ref", "-d", name])
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn key(change: &FileChange) -> Key {
        (
            change.path.clone(),
            change.old_blob.clone(),
            change.new_blob.clone(),
        )
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

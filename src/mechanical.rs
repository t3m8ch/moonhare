//! Mechanical files: those whose changes need no close reading. They are
//! shown collapsed and listed after the ordinary files, but still count in
//! progress and are marked Reviewed by hand like any other.

use std::collections::HashSet;

use crate::git::{ChangedFile, Repo};

/// Why a file is Mechanical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Lock,
    Snapshot,
    Binary,
    Generated,
}

impl Kind {
    /// The words shown in the diff pane for a collapsed file.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Lock => "lock file",
            Kind::Snapshot => "snapshot",
            Kind::Binary => "binary",
            Kind::Generated => "generated",
        }
    }
}

/// Lock files that do not end in `.lock`, recognised by file name.
const LOCK_FILES: &[&str] = &[
    "package-lock.json",
    "npm-shrinkwrap.json",
    "pnpm-lock.yaml",
    "bun.lockb",
    "go.sum",
    "Package.resolved",
    "packages.lock.json",
    "gradle.lockfile",
];

/// Directories that hold snapshots.
const SNAPSHOT_DIRS: &[&str] = &["__snapshots__", "snapshots"];

/// Mark the Mechanical files among `files`, the changes from `base` to
/// `head`, and move them behind the ordinary ones. A file keeps its place
/// within its group.
pub fn classify(repo: &Repo, base: &str, head: &str, files: &mut [ChangedFile]) {
    // Two independent git processes: run them side by side.
    let (binary, generated) = std::thread::scope(|scope| {
        let binary = scope.spawn(|| binary_paths(repo, base, head));
        let generated = generated_paths(repo, head, files);
        (binary.join().unwrap_or_default(), generated)
    });
    for file in files.iter_mut() {
        file.mechanical = kind_of_path(&file.path)
            .or_else(|| binary.contains(&file.path).then_some(Kind::Binary))
            .or_else(|| generated.contains(&file.path).then_some(Kind::Generated));
    }
    files.sort_by_key(|f| f.mechanical.is_some());
}

/// Paths the repository's attributes at `head` mark `linguist-generated`.
fn generated_paths(repo: &Repo, head: &str, files: &[ChangedFile]) -> HashSet<String> {
    if files.is_empty() {
        return HashSet::new();
    }
    let source = format!("--source={head}");
    let mut args = vec!["check-attr", &source, "-z", "linguist-generated", "--"];
    args.extend(files.iter().map(|f| f.path.as_str()));
    let Ok(raw) = repo.run_bytes(&args) else {
        return HashSet::new();
    };
    // Triples of path, attribute and value, each ended by a NUL.
    let mut fields = raw.split(|&b| b == 0);
    let mut found = HashSet::new();
    while let (Some(path), Some(_), Some(value)) = (fields.next(), fields.next(), fields.next()) {
        if matches!(value, b"set" | b"true") {
            found.insert(String::from_utf8_lossy(path).into_owned());
        }
    }
    found
}

/// Paths git itself treats as binary in `base..head` (a NUL byte, or a `-diff`
/// attribute): `git diff --numstat` prints `-` for their line counts.
fn binary_paths(repo: &Repo, base: &str, head: &str) -> HashSet<String> {
    let args = ["diff-tree", "-r", "-M", "--numstat", "-z", base, head];
    let Ok(raw) = repo.run_bytes(&args) else {
        return HashSet::new();
    };
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    let mut parts = raw.split(|&b| b == 0).filter(|p| !p.is_empty());
    let mut found = HashSet::new();
    while let Some(head) = parts.next() {
        let head = text(head);
        let mut fields = head.splitn(3, '\t');
        let (Some(added), Some(deleted), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            break;
        };
        // A rename has an empty path here, then the old and the new path.
        let path = if path.is_empty() {
            parts.next();
            match parts.next() {
                Some(new) => text(new),
                None => break,
            }
        } else {
            path.to_string()
        };
        if added == "-" && deleted == "-" {
            found.insert(path);
        }
    }
    found
}

/// The kind decided by the path alone.
pub fn kind_of_path(path: &str) -> Option<Kind> {
    let mut parts = path.rsplit('/');
    let name = parts.next().unwrap_or(path);
    if name.ends_with(".lock") || LOCK_FILES.contains(&name) {
        return Some(Kind::Lock);
    }
    if name.ends_with(".snap")
        || name.ends_with(".snap.new")
        || parts.any(|d| SNAPSHOT_DIRS.contains(&d))
    {
        return Some(Kind::Snapshot);
    }
    None
}

//! The Review's record of what is Reviewed.
//!
//! A mark belongs to a File change and is keyed by its content (the path and
//! the blob ids it turns one version into another), not by the commit that
//! carries it (ADR 0003).

use std::collections::{HashMap, HashSet};
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
    Ok(parse_raw(&raw).into_iter().flat_map(|(_, c)| c).collect())
}

/// The file changes of many `(base, head)` pairs from a single git process,
/// in the order of `pairs`. The heads must differ from one another.
pub fn file_changes_of_pairs(
    repo: &Repo,
    pairs: &[(&str, &str)],
) -> Result<Vec<Vec<FileChange>>, GitError> {
    if pairs.is_empty() {
        return Ok(Vec::new());
    }
    let input: String = pairs.iter().map(|(b, h)| format!("{h} {b}\n")).collect();
    let raw = repo.run_bytes_stdin(
        &[
            "diff-tree",
            "-r",
            "-M",
            "--raw",
            "--no-abbrev",
            "-z",
            "--stdin",
        ],
        input.as_bytes(),
    )?;
    // Each pair with changes is announced by the id of its head (a stdin line is `<commit> <parent>`).
    let mut by_head: HashMap<String, Vec<FileChange>> = parse_raw(&raw).into_iter().collect();
    Ok(pairs
        .iter()
        .map(|(_, head)| by_head.remove(*head).unwrap_or_default())
        .collect())
}

/// Parse `diff-tree --raw -z` output: runs of changes, each run led by the
/// id git prints before it (empty when git printed none).
fn parse_raw(raw: &[u8]) -> Vec<(String, Vec<FileChange>)> {
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    let mut parts = raw.split(|&b| b == 0).filter(|p| !p.is_empty());
    let mut runs: Vec<(String, Vec<FileChange>)> = vec![(String::new(), Vec::new())];
    while let Some(part) = parts.next() {
        if part[0] != b':' {
            runs.push((text(part), Vec::new()));
            continue;
        }
        let Some(path) = parts.next() else { break };
        let meta = text(part);
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
        if let Some((_, changes)) = runs.last_mut() {
            changes.push(FileChange {
                path,
                old_path,
                old_blob: old.to_string(),
                new_blob: new.to_string(),
            });
        }
    }
    runs
}

/// The blobs of a change worth keeping alive.
pub fn blobs(change: &FileChange) -> impl Iterator<Item = &str> {
    [change.old_blob.as_str(), change.new_blob.as_str()]
        .into_iter()
        .filter(|b| *b != NULL_BLOB)
}

/// What identifies a File change: its path and the blobs on both sides.
type Key = (String, String, String);

/// A mark seen as an edge of the file's history graph: the version before,
/// the version after, and when it was made (later is larger).
type Edge = (String, String, u64);

/// What a walk along marked edges found.
struct Walk {
    /// Versions of the file the Reviewer has reviewed up to, starting from
    /// the base of the Scope.
    reached: HashSet<String>,
    /// The marks followed, as (path, old, new).
    edges: Vec<(String, Edge)>,
}

/// Which File changes are Reviewed, kept in a file inside the clone's `.git`
/// so that it outlives the process and is never shared with other clones.
#[derive(Debug)]
pub struct Review {
    /// Identifies the Review among the clone's: derived from its name.
    id: String,
    /// `.git/moonhare/reviews/<id>`.
    dir: PathBuf,
    /// The marks and when each was made (a counter, so later is larger).
    marked: HashMap<Key, u64>,
    next: u64,
    /// The same marks by path, to follow them from version to version.
    by_path: HashMap<String, Vec<Edge>>,
}

/// 128 bits of FNV-1a over `text`, as 32 hex digits.
pub(crate) fn digest(text: &str) -> String {
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
        let mut review = Review {
            id,
            dir,
            marked: HashMap::new(),
            next: 0,
            by_path: HashMap::new(),
        };
        if let Ok(saved) = std::fs::read(review.dir.join("marks")) {
            for record in saved.split(|&b| b == 0).filter(|r| !r.is_empty()) {
                let record = String::from_utf8_lossy(record);
                let mut fields = record.splitn(3, ' ');
                if let (Some(old), Some(new), Some(path)) =
                    (fields.next(), fields.next(), fields.next())
                {
                    review.insert((path.to_string(), old.to_string(), new.to_string()));
                }
            }
        }
        Ok(review)
    }

    /// The directory the Review keeps its records in.
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    pub fn is_marked(&self, change: &FileChange) -> bool {
        self.marked.contains_key(&Self::key(change))
    }

    /// Record a mark as the latest one.
    fn insert(&mut self, key: Key) {
        self.remove(&key);
        self.next += 1;
        let (path, old, new) = key.clone();
        self.by_path
            .entry(path)
            .or_default()
            .push((old, new, self.next));
        self.marked.insert(key, self.next);
    }

    fn remove(&mut self, key: &Key) {
        if self.marked.remove(key).is_some() {
            if let Some(edges) = self.by_path.get_mut(&key.0) {
                edges.retain(|(old, new, _)| *old != key.1 || *new != key.2);
            }
        }
    }

    fn save(&self) -> Result<(), String> {
        let mut data = Vec::new();
        // In the order the marks were made, so that it survives a restart.
        let mut records: Vec<_> = self.marked.iter().collect();
        records.sort_by_key(|(_, made)| **made);
        for ((path, old, new), _) in records {
            data.extend(format!("{old} {new} {path}").bytes());
            data.push(0);
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| io_error("cannot save the Review", e))?;
        // Two running instances must not write the same temporary file.
        let tmp = self.dir.join(format!("marks.{}.tmp", std::process::id()));
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
        // Oldest first, so the newest change is the latest mark.
        for change in changes.iter().rev() {
            self.insert(Self::key(change));
        }
        self.save()
    }

    /// Take the marks of `changes` back and save. The pins stay until a reset.
    /// If the file still counts as Reviewed (by marks of an earlier shape of
    /// the history), those marks go too, or the mark could not be taken back.
    pub fn unmark(&mut self, changes: &[FileChange]) -> Result<(), String> {
        for c in changes {
            self.remove(&Self::key(c));
        }
        while let Some(walk) = self.walk(changes).filter(|w| self.reaches_head(changes, w)) {
            if walk.edges.is_empty() {
                break;
            }
            for (path, (old, new, _)) in walk.edges {
                self.remove(&(path, old, new));
            }
        }
        self.save()
    }

    /// Forget every mark and release every pin of the Review.
    pub fn reset(&mut self, repo: &Repo) -> Result<(), String> {
        self.marked.clear();
        self.by_path.clear();
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

    /// Follow the marks from the version at the base of `changes` (newest
    /// first). `None` when there are no changes.
    fn walk(&self, changes: &[FileChange]) -> Option<Walk> {
        let base = &changes.last()?.old_blob;
        // A renamed file was marked under each of its names.
        let paths: HashSet<&str> = changes
            .iter()
            .flat_map(|c| [Some(c.path.as_str()), c.old_path.as_deref()])
            .flatten()
            .collect();
        // The base counts as reached only through marks, never by itself: a
        // file whose content did not change (a pure rename, a mode change, an
        // edit that was undone) is not Reviewed until it is marked.
        let mut reached: HashSet<String> = HashSet::new();
        let mut edges = Vec::new();
        let mut open = vec![base.clone()];
        let mut expanded: HashSet<String> = HashSet::new();
        while let Some(version) = open.pop() {
            if !expanded.insert(version.clone()) {
                continue;
            }
            for path in &paths {
                for edge in self.by_path.get(*path).into_iter().flatten() {
                    if edge.0 == version {
                        edges.push((path.to_string(), edge.clone()));
                        reached.insert(edge.1.clone());
                        open.push(edge.1.clone());
                    }
                }
            }
        }
        Some(Walk { reached, edges })
    }

    /// Whether the marks lead to the newest version. A change that leaves
    /// the blob as it was (a rename, a mode change) is a step the walk can
    /// take through any mark of the same version, so each such change has to
    /// be marked itself.
    fn reaches_head(&self, changes: &[FileChange], walk: &Walk) -> bool {
        changes
            .first()
            .is_some_and(|newest| walk.reached.contains(&newest.new_blob))
            && changes
                .iter()
                .filter(|c| c.old_blob == c.new_blob)
                .all(|c| self.is_marked(c))
    }

    /// `changes` (newest first) are Reviewed when the marks lead from the
    /// version before the oldest to the version after the newest. That holds
    /// when every change is marked, and also when the history was rewritten
    /// (squashed, split, rebased) but the Reviewer has been through the same
    /// versions.
    pub fn state(&self, changes: &[FileChange]) -> Mark {
        let Some(walk) = self.walk(changes) else {
            return Mark::Unreviewed;
        };
        if self.reaches_head(changes, &walk) {
            Mark::Reviewed
        } else if !walk.edges.is_empty() || changes.iter().any(|c| self.is_marked(c)) {
            Mark::Partial
        } else {
            Mark::Unreviewed
        }
    }

    /// The version of the file the Reviewer last reviewed up to, in the
    /// scope of `changes` (newest first): the target of the latest mark on
    /// the way from the base. `None` when the Reviewer has not got past it.
    pub fn seen_blob(&self, changes: &[FileChange]) -> Option<String> {
        let walk = self.walk(changes)?;
        walk.edges
            .into_iter()
            .max_by_key(|(_, (_, _, made))| *made)
            .map(|(_, (_, new, _))| new)
    }
}

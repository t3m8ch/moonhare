//! The Analyst's answers kept on disk beside the rest of the Review, so that
//! a second run on the same commits spends no tokens (ADR 0006).
//!
//! One file per answer under `<review dir>/analyst/`, named by a digest of
//! its key. A file is a header line, the key, and the answer as JSON. Anything
//! that does not read back is a miss, never an error.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use serde_json::{json, Value};

use crate::analyst::{Diagram, Explanation, Group, Grouping, Summary};
use crate::review::digest;

/// Bump when a prompt or the shape of an answer changes: older entries stop
/// matching and are evicted in time.
pub const FORMAT_VERSION: u32 = 1;

const HEADER: &str = "moonhare-analyst-cache";

/// Entries kept; the least recently written go first.
const MAX_ENTRIES: usize = 200;

/// Larger files are not ours (or are damaged) and are not read.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// Makes the name of a temporary file unique within the process.
static WRITES: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone)]
pub struct AnalystCache {
    dir: PathBuf,
}

impl AnalystCache {
    /// The cache of the Review that keeps its records in `review_dir`.
    pub fn new(review_dir: &Path) -> AnalystCache {
        AnalystCache {
            dir: review_dir.join("analyst"),
        }
    }

    /// The key text of an answer of `kind`: `parts` are what it depends on.
    /// Effort and model are deliberately not among them.
    pub fn key(kind: &str, parts: &[&str]) -> String {
        let mut key = format!("{kind}\u{1}v{FORMAT_VERSION}");
        for part in parts {
            key.push('\u{1}');
            // A part holding the separator cannot shift its neighbours.
            key.push_str(&part.replace('\u{1}', "\u{2}").replace('\n', "\u{3}"));
        }
        key
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(digest(key))
    }

    fn load(&self, key: &str) -> Option<Value> {
        let path = self.path(key);
        if std::fs::metadata(&path).ok()?.len() > MAX_FILE_BYTES {
            return None;
        }
        let text = std::fs::read_to_string(path).ok()?;
        let mut parts = text.splitn(3, '\n');
        let header = parts.next()?;
        let stored_key = parts.next()?;
        let body = parts.next()?;
        if header != format!("{HEADER} {FORMAT_VERSION}") || stored_key != key {
            return None;
        }
        serde_json::from_str(body).ok()
    }

    /// Save `answer`. Failures are ignored: a cache that cannot be written is
    /// a cache that misses.
    fn store(&self, key: &str, answer: &Value) {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let path = self.path(key);
        let tmp = path.with_extension(format!(
            "tmp{}-{}",
            std::process::id(),
            WRITES.fetch_add(1, Ordering::Relaxed)
        ));
        let text = format!("{HEADER} {FORMAT_VERSION}\n{key}\n{answer}");
        if std::fs::write(&tmp, text).is_err() || std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return;
        }
        self.evict();
    }

    fn evict(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(SystemTime, PathBuf)> = entries
            .flatten()
            .filter(|e| e.path().extension().is_none())
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        if files.len() <= MAX_ENTRIES {
            return;
        }
        files.sort();
        for (_, path) in &files[..files.len() - MAX_ENTRIES] {
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn load_summary(&self, key: &str) -> Option<Summary> {
        let v = self.load(key)?;
        Some(Summary {
            text: v.get("text")?.as_str()?.to_string(),
            diagram: match v.get("diagram")? {
                Value::Null => None,
                d => Some(d.as_str()?.to_string()),
            },
        })
    }

    pub fn store_summary(&self, key: &str, summary: &Summary) {
        self.store(
            key,
            &json!({ "text": summary.text, "diagram": summary.diagram }),
        );
    }

    pub fn load_grouping(&self, key: &str) -> Option<Grouping> {
        let v = self.load(key)?;
        let mut groups = Vec::new();
        for g in v.get("groups")?.as_array()? {
            groups.push(Group {
                name: g.get("name")?.as_str()?.to_string(),
                description: g.get("description")?.as_str()?.to_string(),
                mechanical: g.get("mechanical")?.as_bool()?,
                files: g
                    .get("files")?
                    .as_array()?
                    .iter()
                    .map(|f| f.as_str().map(str::to_string))
                    .collect::<Option<_>>()?,
            });
        }
        Some(Grouping { groups })
    }

    pub fn store_grouping(&self, key: &str, grouping: &Grouping) {
        let groups: Vec<Value> = grouping
            .groups
            .iter()
            .map(|g| {
                json!({
                    "name": g.name, "description": g.description,
                    "mechanical": g.mechanical, "files": g.files,
                })
            })
            .collect();
        self.store(key, &json!({ "groups": groups }));
    }

    pub fn load_explanation(&self, key: &str) -> Option<Explanation> {
        let v = self.load(key)?;
        Some(Explanation {
            text: v.get("text")?.as_str()?.to_string(),
        })
    }

    pub fn store_explanation(&self, key: &str, explanation: &Explanation) {
        self.store(key, &json!({ "text": explanation.text }));
    }

    pub fn load_diagram(&self, key: &str) -> Option<Diagram> {
        let v = self.load(key)?;
        Some(Diagram {
            mermaid: v.get("mermaid")?.as_str()?.to_string(),
        })
    }

    pub fn store_diagram(&self, key: &str, diagram: &Diagram) {
        self.store(key, &json!({ "mermaid": diagram.mermaid }));
    }
}

//! Comments: the Reviewer's remarks on lines, files or the whole Range.
//!
//! A line Comment is bound to a side (old or new) and keeps a snapshot of the
//! code it was written on, so it can be found again when lines move.

use std::path::{Path, PathBuf};

pub use crate::diff::Side;

/// What a Comment is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Lines `start..=end` (0-based) of one side of a file, with the code
    /// they held when the Comment was written.
    Lines {
        path: String,
        side: Side,
        start: usize,
        end: usize,
        snapshot: Vec<String>,
    },
    File {
        path: String,
    },
    Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Open,
    Sent,
    Closed,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::Open => "open",
            State::Sent => "sent",
            State::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub id: u64,
    pub target: Target,
    pub text: String,
    pub state: State,
}

/// Where the code a line Comment was written on is in `lines` now: the stored
/// place when the snapshot still sits there, else the nearest place that holds
/// it, else nowhere.
pub fn locate(lines: &[String], start: usize, snapshot: &[String]) -> Option<usize> {
    let len = snapshot.len();
    if len == 0 || len > lines.len() {
        return None;
    }
    let at = |i: usize| lines[i..i + len] == *snapshot;
    if start + len <= lines.len() && at(start) {
        return Some(start);
    }
    (0..=lines.len() - len)
        .filter(|&i| at(i))
        .min_by_key(|&i| i.abs_diff(start))
}

/// All Comments of a Review, in the order they were written, kept in the
/// Review's directory inside the clone's `.git`.
#[derive(Debug, Default)]
pub struct Comments {
    items: Vec<Comment>,
    file: Option<PathBuf>,
}

/// Separates the fields of a saved Comment; records are separated by a NUL.
const FIELD: char = '\x1f';

impl Comments {
    /// The Comments saved in `dir` by an earlier run, or none.
    pub fn open(dir: &Path) -> Comments {
        let file = dir.join("comments");
        let items = std::fs::read_to_string(&file)
            .map(|saved| {
                saved
                    .split('\0')
                    .filter_map(Comment::decode)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Comments {
            items,
            file: Some(file),
        }
    }

    /// Add an open Comment and save.
    pub fn add(&mut self, target: Target, text: String) -> Result<u64, String> {
        let id = self.items.iter().map(|c| c.id).max().map_or(1, |m| m + 1);
        self.items.push(Comment {
            id,
            target,
            text,
            state: State::Open,
        });
        self.save().map(|()| id)
    }

    /// Forget every Comment without touching the disk (the Review's reset
    /// removes its directory).
    pub fn clear(&mut self) {
        self.items.clear();
    }

    fn save(&self) -> Result<(), String> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        let data: Vec<String> = self.items.iter().map(Comment::encode).collect();
        let io = |e: std::io::Error| format!("cannot save the Comments: {e}");
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let tmp = file.with_extension("tmp");
        std::fs::write(&tmp, data.join("\0")).map_err(io)?;
        std::fs::rename(&tmp, file).map_err(io)
    }

    /// Put the Comments with these ids into `state`.
    pub fn set_state(&mut self, ids: &[u64], state: State) -> Result<(), String> {
        for c in self.items.iter_mut().filter(|c| ids.contains(&c.id)) {
            c.state = state;
        }
        self.save()
    }

    pub fn all(&self) -> &[Comment] {
        &self.items
    }
}

impl Comment {
    fn encode(&self) -> String {
        let (kind, path, side, start, end, snapshot) = match &self.target {
            Target::Lines {
                path,
                side,
                start,
                end,
                snapshot,
            } => (
                "L",
                path.as_str(),
                if *side == Side::Old { "o" } else { "n" },
                *start,
                *end,
                snapshot.join("\n"),
            ),
            Target::File { path } => ("F", path.as_str(), "", 0, 0, String::new()),
            Target::Range => ("R", "", "", 0, 0, String::new()),
        };
        let clean = |t: &str| t.replace(['\0', FIELD], " ");
        [
            self.id.to_string(),
            self.state.label().to_string(),
            kind.to_string(),
            clean(path),
            side.to_string(),
            start.to_string(),
            end.to_string(),
            clean(&self.text),
            clean(&snapshot),
        ]
        .join(&FIELD.to_string())
    }

    fn decode(record: &str) -> Option<Comment> {
        let f: Vec<&str> = record.split(FIELD).collect();
        let [id, state, kind, path, side, start, end, text, snapshot] = f[..] else {
            return None;
        };
        let state = match state {
            "open" => State::Open,
            "sent" => State::Sent,
            "closed" => State::Closed,
            _ => return None,
        };
        let target = match kind {
            "L" => Target::Lines {
                path: path.to_string(),
                side: if side == "o" { Side::Old } else { Side::New },
                start: start.parse().ok()?,
                end: end.parse().ok()?,
                snapshot: snapshot.split('\n').map(str::to_string).collect(),
            },
            "F" => Target::File {
                path: path.to_string(),
            },
            "R" => Target::Range,
            _ => return None,
        };
        Some(Comment {
            id: id.parse().ok()?,
            target,
            text: text.to_string(),
            state,
        })
    }
}

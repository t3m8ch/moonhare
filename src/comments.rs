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
        anchor: Anchor,
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

/// Lines of surroundings kept on each side of a snapshot.
const CONTEXT: usize = 2;

/// What a line Comment remembers of its code, to find it again after the
/// lines moved: the code itself, its neighbours, and whether the code was the
/// only place in the file that looked like that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub snapshot: Vec<String>,
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub unique: bool,
}

fn occurrences(lines: &[String], snapshot: &[String]) -> Vec<usize> {
    let len = snapshot.len();
    if len == 0 || len > lines.len() {
        return vec![];
    }
    (0..=lines.len() - len)
        .filter(|&i| lines[i..i + len] == *snapshot)
        .collect()
}

impl Anchor {
    /// The anchor of lines `start..=end` of `lines`.
    pub fn capture(lines: &[String], start: usize, end: usize) -> Anchor {
        let snapshot = lines[start..=end].to_vec();
        Anchor {
            before: lines[start.saturating_sub(CONTEXT)..start].to_vec(),
            after: lines[end + 1..(end + 1 + CONTEXT).min(lines.len())].to_vec(),
            unique: occurrences(lines, &snapshot).len() == 1,
            snapshot,
        }
    }

    fn surroundings_match(&self, lines: &[String], at: usize) -> bool {
        let end = at + self.snapshot.len();
        let before = &lines[at.saturating_sub(self.before.len())..at];
        let after = &lines[end..(end + self.after.len()).min(lines.len())];
        before == self.before && after == self.after
    }
}

/// Where the code of a line Comment written at `start` is in `lines` now.
/// Code that was one of a kind is found wherever it went. Code that looked
/// like other lines (`}`, a blank) is found only where its neighbours match
/// too, and a doubtful match counts as no match: such a Comment is hidden
/// rather than hung under the wrong line.
pub fn locate(lines: &[String], start: usize, anchor: &Anchor) -> Option<usize> {
    let found = occurrences(lines, &anchor.snapshot);
    let in_context: Vec<usize> = found
        .iter()
        .copied()
        .filter(|&i| anchor.surroundings_match(lines, i))
        .collect();
    match in_context[..] {
        [one] => return Some(one),
        [_, ..] if in_context.contains(&start) => return Some(start),
        [_, ..] => return None,
        [] => {}
    }
    match found[..] {
        [one] if anchor.unique => Some(one),
        _ => None,
    }
}

/// All Comments of a Review, in the order they were written, kept in the
/// Review's directory inside the clone's `.git`.
#[derive(Debug, Default)]
pub struct Comments {
    items: Vec<Comment>,
    /// Records this version cannot read (written by another version, or
    /// damaged): kept as they are and written back with the rest.
    unread: Vec<String>,
    /// Changes with every added or deleted Comment, for caches.
    revision: u64,
    file: Option<PathBuf>,
}

/// Separates the fields of a saved Comment; records are separated by a NUL.
const FIELD: char = '\x1f';

impl Comments {
    /// The Comments saved in `dir` by an earlier run, or none.
    pub fn open(dir: &Path) -> Comments {
        let file = dir.join("comments");
        let saved = std::fs::read_to_string(&file).unwrap_or_default();
        let (mut items, mut unread) = (Vec::new(), Vec::new());
        for record in saved.split('\0').filter(|r| !r.is_empty()) {
            match Comment::decode(record) {
                Some(c) => items.push(c),
                None => unread.push(record.to_string()),
            }
        }
        Comments {
            items,
            unread,
            revision: 0,
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
        self.revision += 1;
        self.save().map(|()| id)
    }

    /// Delete every Comment, on disk first: if saving fails the Comments
    /// stay in memory too, so the two never disagree.
    pub fn clear(&mut self) -> Result<(), String> {
        self.revision += 1;
        let kept = std::mem::take(&mut self.items);
        match self.save() {
            Ok(()) => Ok(()),
            Err(e) => {
                self.items = kept;
                Err(e)
            }
        }
    }

    fn save(&self) -> Result<(), String> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        let mut data: Vec<String> = self.items.iter().map(Comment::encode).collect();
        data.extend(self.unread.iter().cloned());
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

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn all(&self) -> &[Comment] {
        &self.items
    }
}

impl Comment {
    fn encode(&self) -> String {
        let none = || [String::new(), String::new(), String::new(), String::new()];
        let (kind, path, side, start, end, anchor) = match &self.target {
            Target::Lines {
                path,
                side,
                start,
                end,
                anchor,
            } => (
                "L",
                path.as_str(),
                if *side == Side::Old { "o" } else { "n" },
                *start,
                *end,
                [
                    anchor.snapshot.join("\n"),
                    anchor.before.join("\n"),
                    anchor.after.join("\n"),
                    if anchor.unique { "u" } else { "" }.to_string(),
                ],
            ),
            Target::File { path } => ("F", path.as_str(), "", 0, 0, none()),
            Target::Range => ("R", "", "", 0, 0, none()),
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
            clean(&anchor[0]),
            clean(&anchor[1]),
            clean(&anchor[2]),
            anchor[3].clone(),
        ]
        .join(&FIELD.to_string())
    }

    fn decode(record: &str) -> Option<Comment> {
        let f: Vec<&str> = record.split(FIELD).collect();
        let [id, state, kind, path, side, start, end, text, snapshot, before, after, unique] =
            f[..]
        else {
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
                anchor: {
                    let lines = |t: &str| -> Vec<String> {
                        if t.is_empty() {
                            vec![]
                        } else {
                            t.split('\n').map(str::to_string).collect()
                        }
                    };
                    Anchor {
                        snapshot: snapshot.split('\n').map(str::to_string).collect(),
                        before: lines(before),
                        after: lines(after),
                        unique: unique == "u",
                    }
                },
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

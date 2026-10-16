//! Comments: the Reviewer's remarks on lines, files or the whole Range.
//!
//! A line Comment is bound to a side (old or new) and keeps a snapshot of the
//! code it was written on, so it can be found again when lines move.

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

/// All Comments of a Review, in the order they were written.
#[derive(Debug, Default)]
pub struct Comments {
    items: Vec<Comment>,
}

impl Comments {
    pub fn add(&mut self, target: Target, text: String) -> u64 {
        let id = self.items.iter().map(|c| c.id).max().map_or(1, |m| m + 1);
        self.items.push(Comment {
            id,
            target,
            text,
            state: State::Open,
        });
        id
    }

    pub fn all(&self) -> &[Comment] {
        &self.items
    }
}

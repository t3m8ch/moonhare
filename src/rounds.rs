//! Rounds: the Prompts the Reviewer handed over, and how they cut the Range's
//! commits into Rounds.
//!
//! A Prompt remembers the head commit and the number of commits the Range had
//! when it was copied; commits after that point are the Author agent's answer
//! and belong to the next Round.

use std::path::{Path, PathBuf};

use crate::git::Commit;

/// One Prompt that was copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub number: u32,
    /// The head of the Range when it was copied.
    pub head: String,
    /// How many commits the Range had then.
    pub commits: usize,
}

/// All Prompts of a Review, kept in the Review's directory.
#[derive(Debug, Default)]
pub struct Prompts {
    items: Vec<Sent>,
    file: Option<PathBuf>,
}

const FIELD: char = '\x1f';

impl Prompts {
    /// The Prompts saved in `dir` by an earlier run, or none.
    pub fn open(dir: &Path) -> Prompts {
        let file = dir.join("prompts");
        let saved = std::fs::read_to_string(&file).unwrap_or_default();
        let items = saved
            .lines()
            .filter_map(|line| {
                let mut f = line.split(FIELD);
                Some(Sent {
                    number: f.next()?.parse().ok()?,
                    head: f.next()?.to_string(),
                    commits: f.next()?.parse().ok()?,
                })
            })
            .collect();
        Prompts {
            items,
            file: Some(file),
        }
    }

    /// The number the next Prompt gets.
    pub fn next_number(&self) -> u32 {
        self.items.iter().map(|p| p.number).max().unwrap_or(0) + 1
    }

    /// Remember that a Prompt was copied with the Range at `head`, and save.
    /// On a failed save nothing is remembered.
    pub fn record(&mut self, head: &str, commits: usize) -> Result<u32, String> {
        let number = self.next_number();
        self.items.push(Sent {
            number,
            head: head.to_string(),
            commits,
        });
        if let Err(e) = self.save() {
            self.items.pop();
            return Err(e);
        }
        Ok(number)
    }

    fn save(&self) -> Result<(), String> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        let data: String = self
            .items
            .iter()
            .map(|p| format!("{}{FIELD}{}{FIELD}{}\n", p.number, p.head, p.commits))
            .collect();
        let io = |e: std::io::Error| format!("cannot save the Prompts: {e}");
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let tmp = file.with_extension("tmp");
        std::fs::write(&tmp, data).map_err(io)?;
        std::fs::rename(&tmp, file).map_err(io)
    }

    pub fn all(&self) -> &[Sent] {
        &self.items
    }
}

/// One Round of the commit list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Round {
    pub number: usize,
    /// Index of its first commit.
    pub start: usize,
    /// The Prompts it came after, oldest first.
    pub after: Vec<u32>,
}

impl Round {
    /// "Round 2, after Prompt #1" (or "after Prompts #1, #2").
    pub fn title(&self) -> String {
        match self.after[..] {
            [] => format!("Round {}", self.number),
            [one] => format!("Round {}, after Prompt #{one}", self.number),
            _ => {
                // Short enough for the narrow commit list.
                let all: Vec<String> = self.after.iter().map(|n| format!("#{n}")).collect();
                format!("Round {}, after {}", self.number, all.join(", "))
            }
        }
    }
}

/// The Rounds of `commits`. The last one is the current Round, even when no
/// commit has answered the latest Prompt yet.
pub fn rounds(prompts: &[Sent], commits: &[Commit]) -> Vec<Round> {
    let mut rounds = vec![Round {
        number: 1,
        start: 0,
        after: vec![],
    }];
    let mut cut = 0;
    for p in prompts {
        let at = commits
            .iter()
            .position(|c| c.sha == p.head)
            .map_or(p.commits, |i| i + 1)
            .min(commits.len())
            .max(cut);
        // A Prompt copied before any new commit joins the Round it would
        // otherwise leave empty.
        if at > cut {
            rounds.push(Round {
                number: rounds.len() + 1,
                start: at,
                after: vec![],
            });
            cut = at;
        }
        if let Some(round) = rounds.last_mut() {
            round.after.push(p.number);
        }
    }
    rounds
}

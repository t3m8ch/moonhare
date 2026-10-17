//! Rounds in the session: the commit list with Round headers and the current
//! Round in the status line.

use super::*;

/// The `position` of a Round header row, which no cursor can stand on.
pub(super) const HEADER: usize = usize::MAX;

/// One row of the scrolling part of the commit list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ListRow {
    Header(String),
    Commit(usize),
}

impl Session {
    /// The Rounds of the Range's commits.
    fn rounds(&self) -> Vec<crate::rounds::Round> {
        crate::rounds::rounds(self.prompts.all(), &self.commits)
    }

    /// The commits, each Round under a header once a Prompt exists. A Round
    /// without commits has no header of its own.
    pub(super) fn list_rows(&self) -> Vec<ListRow> {
        if self.prompts.all().is_empty() {
            return (0..self.commits.len()).map(ListRow::Commit).collect();
        }
        let rounds = self.rounds();
        let mut rows = Vec::new();
        for (k, round) in rounds.iter().enumerate() {
            let end = rounds.get(k + 1).map_or(self.commits.len(), |r| r.start);
            if round.start >= end {
                continue;
            }
            rows.push(ListRow::Header(round.title()));
            rows.extend((round.start..end).map(ListRow::Commit));
        }
        rows
    }

    /// The number of the current Round: the one the next commits join.
    pub(super) fn current_round(&self) -> usize {
        self.rounds().last().map_or(1, |r| r.number)
    }
}

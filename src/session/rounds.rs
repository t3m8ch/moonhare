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
    /// A folded Round: "Round 1, 60 commits" standing for commits `start..end`.
    Folded {
        title: String,
        start: usize,
        end: usize,
    },
}

impl ListRow {
    /// Whether the row shows commit `index`, on its own or folded.
    pub(super) fn holds(&self, index: usize) -> bool {
        match self {
            ListRow::Commit(i) => *i == index,
            ListRow::Folded { start, end, .. } => (*start..*end).contains(&index),
            ListRow::Header(_) => false,
        }
    }
}

/// The commits `start..end` of one Round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Span {
    pub number: usize,
    pub title: String,
    pub start: usize,
    pub end: usize,
}

impl Session {
    /// The Rounds of the Range's commits.
    fn rounds(&self) -> Vec<crate::rounds::Round> {
        crate::rounds::rounds(self.prompts.all(), &self.commits)
    }

    /// The Rounds that have commits, with the commits they hold. Empty until
    /// a Prompt exists.
    pub(super) fn spans(&self) -> Vec<Span> {
        if self.prompts.all().is_empty() {
            return Vec::new();
        }
        let rounds = self.rounds();
        let mut spans = Vec::new();
        for (k, round) in rounds.iter().enumerate() {
            let end = rounds.get(k + 1).map_or(self.commits.len(), |r| r.start);
            if round.start < end {
                spans.push(Span {
                    number: round.number,
                    title: round.title(),
                    start: round.start,
                    end,
                });
            }
        }
        spans
    }

    /// The commits, each Round under a header once a Prompt exists. A Round
    /// without commits has no header of its own.
    pub(super) fn list_rows(&self) -> Vec<ListRow> {
        let spans = self.spans();
        if spans.is_empty() {
            return (0..self.commits.len()).map(ListRow::Commit).collect();
        }
        let mut rows = Vec::with_capacity(self.commits.len() + spans.len());
        for span in spans {
            if self.list.is_folded(span.number) {
                let n = span.end - span.start;
                rows.push(ListRow::Folded {
                    title: format!(
                        "Round {}, {n} commit{}",
                        span.number,
                        if n == 1 { "" } else { "s" }
                    ),
                    start: span.start,
                    end: span.end,
                });
                continue;
            }
            rows.push(ListRow::Header(span.title));
            rows.extend((span.start..span.end).map(ListRow::Commit));
        }
        rows
    }

    /// The number of the current Round: the one the next commits join.
    pub(super) fn current_round(&self) -> usize {
        self.rounds().last().map_or(1, |r| r.number)
    }
}

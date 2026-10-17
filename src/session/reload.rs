//! New commits in the branch: noticed in the background, announced in the
//! status line, applied only on `:reload` / `R` so the screen under the
//! cursor is never rebuilt on its own.

use std::time::Duration;

use super::*;

/// How often the branch is looked at again.
pub const POLL: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(super) struct Reload {
    /// What the Reviewer opened; `None` for a fixed head such as a pull
    /// request, which cannot gain commits by itself.
    pub(super) target: Option<Target>,
    since_poll: Duration,
    /// The Range as the branch has it now, when that differs from ours, and
    /// how many commits in it are ones we have not shown.
    available: Option<Available>,
    /// With `Target::Current`: the clone moved to this other branch, which is
    /// not the Range under review and is never swapped in silently.
    switched: Option<String>,
}

struct Available {
    range: Range,
    /// Commits in the new Range that we have not shown.
    fresh: usize,
    /// Commits we show that the branch no longer has.
    gone: usize,
}

impl Session {
    /// Time passing: once in a while look whether the branch moved.
    /// Returns whether the note in the status line changed.
    pub(super) fn poll_reload(&mut self, elapsed: Duration) -> bool {
        self.reload.since_poll += elapsed;
        if self.reload.since_poll < POLL {
            return false;
        }
        self.reload.since_poll = Duration::ZERO;
        let before = self.reload_status("");
        self.look_for_new_commits();
        before != self.reload_status("")
    }

    /// How long until the next look at the branch.
    pub fn next_poll_in(&self) -> Duration {
        POLL.saturating_sub(self.reload.since_poll)
    }

    fn look_for_new_commits(&mut self) {
        let Some(target) = &self.reload.target else {
            return;
        };
        // A branch that cannot be read right now is looked at again later.
        let Ok(range) = self.repo.range_for(target) else {
            return;
        };
        if matches!(target, Target::Current) && range.head_name != self.range.head_name {
            self.reload.available = None;
            self.reload.switched = Some(range.head_name);
            return;
        }
        self.reload.switched = None;
        self.reload.available = if range.head == self.range.head && range.base == self.range.base {
            None
        } else {
            let count = |of: &[Commit], among: &[Commit]| {
                of.iter()
                    .filter(|c| !among.iter().any(|o| o.sha == c.sha))
                    .count()
            };
            let now = self
                .repo
                .commits(&range.base, &range.head)
                .unwrap_or_default();
            Some(Available {
                fresh: count(&now, &self.commits),
                gone: count(&self.commits, &now),
                range,
            })
        };
    }

    /// `:reload` / `R`: show the branch as it is now. The Scope stays when
    /// its commits are still there, the selected file stays when it is.
    pub(super) fn reload(&mut self) {
        self.look_for_new_commits();
        if let Some(name) = &self.reload.switched {
            self.message = Some(format!(
                "The clone is on {name} now, not on {}: reopen moonhare to review it",
                self.range.head_name
            ));
            return;
        }
        let Some(Available { range, .. }) = self.reload.available.take() else {
            self.message = Some("Nothing to reload".into());
            return;
        };
        let loaded = self
            .repo
            .commits(&range.base, &range.head)
            .and_then(|commits| {
                let files = self.repo.changed_files(&range.base, &range.head)?;
                let changes = changes_per_commit(&self.repo, &range, &commits)?;
                Ok((commits, files, changes))
            });
        let (commits, files, commit_changes) = match loaded {
            Ok(loaded) => loaded,
            Err(e) => {
                self.message = Some(format!("Cannot reload: {e}"));
                self.reload.available = Some(Available {
                    range,
                    fresh: 0,
                    gone: 0,
                });
                return;
            }
        };
        let same = |lo: usize, hi: usize| {
            (lo..=hi).all(|i| {
                matches!((self.commits.get(i), commits.get(i)), (Some(a), Some(b)) if a.sha == b.sha)
            })
        };
        let scope = match self.scope {
            Scope::Commits { lo, hi } if same(lo, hi) => self.scope,
            _ => Scope::All,
        };
        self.commit_cursor = match scope {
            Scope::All => 0,
            Scope::Commits { lo, .. } => lo + 1,
        };
        self.commit_anchor = None;
        self.range = range;
        self.commits = commits;
        self.range_files = files;
        self.commit_changes = commit_changes;
        self.refresh_head();
        self.apply_scope(scope);
        self.message = Some("Reloaded".into());
    }

    /// The status line with the note about new commits in front.
    pub(super) fn reload_status(&self, rest: &str) -> String {
        if let Some(name) = &self.reload.switched {
            return format!(" ⚠ clone switched to {name}: reopen moonhare{rest}");
        }
        match &self.reload.available {
            Some(Available { fresh, gone, .. }) if *gone > 0 => {
                format!(" branch rewritten: +{fresh}/-{gone} commits (:reload){rest}")
            }
            Some(Available { fresh: 1, .. }) => format!(" +1 new commit (:reload){rest}"),
            Some(Available { fresh, .. }) => format!(" +{fresh} new commits (:reload){rest}"),
            None => rest.to_string(),
        }
    }
}

/// The File changes each commit of the Range makes, oldest commit first.
pub(super) fn changes_per_commit(
    repo: &Repo,
    range: &Range,
    commits: &[Commit],
) -> Result<Vec<Vec<FileChange>>, GitError> {
    let pairs: Vec<(&str, &str)> = commits
        .iter()
        .enumerate()
        .map(|(i, commit)| {
            let parent = match i.checked_sub(1) {
                Some(prev) => &commits[prev].sha,
                None => &range.base,
            };
            (parent.as_str(), commit.sha.as_str())
        })
        .collect();
    // One git process for all the commits.
    review::file_changes_of_pairs(repo, &pairs)
}

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
    available: Option<(Range, usize)>,
}

impl Session {
    /// Time passing: once in a while look whether the branch moved.
    pub(super) fn poll_reload(&mut self, elapsed: Duration) {
        self.reload.since_poll += elapsed;
        if self.reload.since_poll >= POLL {
            self.reload.since_poll = Duration::ZERO;
            self.look_for_new_commits();
        }
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
        self.reload.available = if range.head == self.range.head && range.base == self.range.base {
            None
        } else {
            let shown: std::collections::HashSet<&str> =
                self.commits.iter().map(|c| c.sha.as_str()).collect();
            let fresh = self
                .repo
                .commits(&range.base, &range.head)
                .map(|commits| {
                    commits
                        .iter()
                        .filter(|c| !shown.contains(c.sha.as_str()))
                        .count()
                })
                .unwrap_or(0);
            Some((range, fresh))
        };
    }

    /// `:reload` / `R`: show the branch as it is now. The Scope stays when
    /// its commits are still there, the selected file stays when it is.
    pub(super) fn reload(&mut self) {
        self.look_for_new_commits();
        let Some((range, _)) = self.reload.available.take() else {
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
                self.reload.available = Some((range, 0));
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
        match &self.reload.available {
            Some((_, 1)) => format!(" +1 new commit (:reload){rest}"),
            Some((_, n)) => format!(" +{n} new commits (:reload){rest}"),
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
    let mut all = Vec::new();
    for (i, commit) in commits.iter().enumerate() {
        let parent = match i.checked_sub(1) {
            Some(prev) => &commits[prev].sha,
            None => &range.base,
        };
        all.push(review::file_changes(repo, parent, &commit.sha)?);
    }
    Ok(all)
}

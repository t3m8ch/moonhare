//! Reviewed marks in the session: marking the selected file in the current
//! Scope, the state shown beside each file, progress and the jumps between
//! unreviewed files.

use super::*;

const BAR_WIDTH: usize = 8;
/// Rows at the bottom of the file box that show progress.
pub(super) const PROGRESS_ROWS: u16 = 2;

impl Session {
    /// The File changes of file `index` (of the displayed list) that the
    /// current Scope contains, found by following the file back through
    /// renames from the newest commit to the oldest.
    pub(super) fn scope_changes(&self, index: usize) -> Vec<FileChange> {
        let Some(file) = self.files.get(index) else {
            return vec![];
        };
        self.changes_of(&file.path, self.scope)
    }

    pub(super) fn changes_of(&self, path: &str, scope: Scope) -> Vec<FileChange> {
        let (lo, hi) = match scope {
            Scope::All => (0, self.commits.len().saturating_sub(1)),
            Scope::Commits { lo, hi } => (lo, hi),
        };
        let mut name = path.to_string();
        let mut found = Vec::new();
        for commit in (lo..=hi).rev() {
            let Some(changes) = self.commit_changes.get(commit) else {
                continue;
            };
            if let Some(change) = changes.iter().find(|c| c.path == name) {
                if let Some(old) = &change.old_path {
                    name = old.clone();
                }
                found.push(change.clone());
            }
        }
        found
    }

    pub(super) fn mark_of(&self, index: usize) -> Mark {
        self.review.state(&self.scope_changes(index))
    }

    /// `m`: mark the selected file Reviewed in the Scope, or take the mark
    /// back when it already is.
    pub(super) fn toggle_mark(&mut self) {
        if self.files.is_empty() {
            return;
        }
        let changes = self.scope_changes(self.selected);
        let saved = if self.review.state(&changes) == Mark::Reviewed {
            self.review.unmark(&changes)
        } else {
            self.review.mark(&self.repo, &changes)
        };
        self.report(saved);
        self.refilter_if_filtering();
    }

    /// `:reset`: forget every mark of this Review and release its pins.
    pub(super) fn reset_review(&mut self) {
        let reset = self.review.reset(&self.repo);
        match reset {
            Ok(()) => self.message = Some("Review reset".into()),
            Err(e) => self.message = Some(e),
        }
    }

    /// Show a failure to save the Review; the marks stay in memory.
    fn report(&mut self, saved: Result<(), String>) {
        if let Err(e) = saved {
            self.message = Some(e);
        }
    }

    /// `M`: mark the selected file Reviewed (never the other way round) and
    /// move to the next unreviewed one.
    pub(super) fn mark_and_advance(&mut self) {
        if self.files.is_empty() {
            return;
        }
        let changes = self.scope_changes(self.selected);
        let saved = self.review.mark(&self.repo, &changes);
        self.report(saved);
        if self.unreviewed_only {
            self.advance_in_queue();
        } else {
            self.jump_unreviewed(true);
        }
    }

    /// `]u` / `[u`: the nearest file after / before the selected one that is
    /// not fully Reviewed in the Scope.
    pub(super) fn jump_unreviewed(&mut self, forward: bool) {
        let target = if forward {
            (self.selected + 1..self.files.len()).find(|&i| self.mark_of(i) != Mark::Reviewed)
        } else {
            (0..self.selected)
                .rev()
                .find(|&i| self.mark_of(i) != Mark::Reviewed)
        };
        match target {
            Some(i) => self.select(i),
            None if forward => self.message = Some("No more unreviewed files".into()),
            None => self.message = Some("No previous unreviewed file".into()),
        }
    }

    /// Reviewed files out of all files, in the Scope and in the whole Range.
    fn progress(&self) -> ((usize, usize), (usize, usize)) {
        let count = |files: &[ChangedFile], scope: Scope| {
            let done = files
                .iter()
                .filter(|f| self.review.state(&self.changes_of(&f.path, scope)) == Mark::Reviewed)
                .count();
            (done, files.len())
        };
        (
            count(&self.scope_files, self.scope),
            count(&self.range_files, Scope::All),
        )
    }

    /// The two bottom rows inside the file box: `reviewed 2/5 [███░░░░░]` for
    /// the Scope and `whole Range 4/9` for the Range.
    pub(super) fn render_progress(&self, buf: &mut Buffer, area: Rect) {
        let (scope, range) = self.progress();
        let filled = (scope.0 * BAR_WIDTH).checked_div(scope.1).unwrap_or(0);
        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(BAR_WIDTH - filled));
        let width = area.width.saturating_sub(2) as usize;
        let rows = [
            format!(" reviewed {}/{} [{bar}]", scope.0, scope.1),
            format!(" whole Range {}/{}", range.0, range.1),
        ];
        let first = area.y + area.height - 1 - PROGRESS_ROWS;
        for (i, row) in rows.iter().enumerate() {
            let style = Style::new().fg(if i == 0 { FG } else { DIM });
            buf.set_stringn(area.x + 1, first + i as u16, pad(row, width), width, style);
        }
    }
}

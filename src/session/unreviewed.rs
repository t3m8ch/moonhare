//! The "only unreviewed" filter: the queue of files still to read.

use super::*;

impl Session {
    /// The files of the Scope that the filter lets through: all of them, or
    /// only those not fully Reviewed.
    pub(super) fn visible_files(&self) -> Vec<ChangedFile> {
        let files = self
            .scope_files
            .iter()
            .filter(|f| {
                !self.unreviewed_only
                    || self.review.state(&self.changes_of(&f.path, self.scope)) != Mark::Reviewed
            })
            .cloned()
            .map(|mut f| {
                // Against a version the Reviewer saw, a new file is a change.
                let seen = self.last_seen(&f.path);
                if f.status == Status::Added
                    && seen.is_some_and(|c| c.new_blob != review::NULL_BLOB)
                {
                    f.status = Status::Modified;
                }
                f
            })
            .collect();
        self.in_group_order(files)
    }

    /// `Space u`: switch the filter, keeping the selected file when it stays.
    pub(super) fn toggle_unreviewed(&mut self) {
        self.unreviewed_only = !self.unreviewed_only;
        self.refilter();
    }

    /// After a mark changed: with the filter on, the file may have left.
    pub(super) fn refilter_if_filtering(&mut self) {
        if self.unreviewed_only {
            self.refilter();
        }
    }

    /// `M` with the filter on: the marked file has left the queue, so the next
    /// one already stands under the cursor, unless it was the last.
    pub(super) fn advance_in_queue(&mut self) {
        let was = self.selected;
        self.refilter();
        if self.selected < was || self.files.is_empty() {
            self.message = Some("No more unreviewed files".into());
        }
    }

    /// Rebuild the file list. The selection stays on its file, or when that
    /// one left the list, on whatever took its place.
    pub(super) fn refilter(&mut self) {
        let previous = self.files.get(self.selected).map(|f| f.path.clone());
        self.files = self.visible_files();
        self.selected = previous
            .and_then(|p| self.files.iter().position(|f| f.path == p))
            .unwrap_or_else(|| self.selected.min(self.files.len().saturating_sub(1)));
        self.load_selected();
    }

    /// With the filter on: the last File change of the longest run of marked
    /// ones from the start of the Scope, whose result is what the Reviewer
    /// has seen of `path`. `None` when that run is empty.
    fn last_seen(&self, path: &str) -> Option<FileChange> {
        if !self.unreviewed_only {
            return None;
        }
        let changes = self.changes_of(path, self.scope);
        let run = changes
            .iter()
            .rev()
            .take_while(|c| self.review.is_marked(c))
            .count();
        // `changes` is newest first, so the run's last change sits at len - run.
        changes
            .get(changes.len().checked_sub(run).filter(|_| run > 0)?)
            .cloned()
    }

    /// The content from which only the delta of `path` is shown, or `None`
    /// to show the whole Scope's diff.
    pub(super) fn delta_baseline(&self, path: &str) -> Option<Result<Option<Vec<u8>>, String>> {
        let seen = self.last_seen(path)?;
        Some(if seen.new_blob == review::NULL_BLOB {
            Ok(None)
        } else {
            self.repo
                .blob(&seen.new_blob)
                .map(Some)
                .map_err(|e| e.to_string())
        })
    }
}

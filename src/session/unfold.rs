//! Revealing the unchanged lines a fold of the diff hides, and folding them
//! back: `Enter`, the `z` keys of the diff pane and a click on a fold.
//!
//! What is revealed is remembered per diff (file and the versions compared),
//! as the old and new line numbers of the lines, so it survives other files,
//! resizes and a switch between side-by-side and inline. Another Scope or new
//! commits compare other versions and start folded.

use std::collections::{HashMap, HashSet};

use crate::fold::{fold, BIG_FOLD, CHUNK};

use super::*;

/// A line of the diff by its old and new line numbers: the same in both
/// layouts.
type LineNumbers = (Option<usize>, Option<usize>);

#[derive(Default)]
pub(super) struct Folds {
    /// One flag per entry of the built layout: whether the line changed.
    pub changed: Vec<bool>,
    /// The revealed lines of each diff.
    revealed: HashMap<diffs::Key, HashSet<LineNumbers>>,
    /// Whether the diff cursor last moved up: a big fold opens from below.
    came_up: bool,
}

/// The diff cursor and what its index means.
#[derive(Clone, Copy)]
pub(super) struct CursorSpot {
    file: usize,
    items: usize,
    cursor: usize,
}

impl Session {
    /// Fold the built layout again with the lines revealed in it. The last
    /// line of a Comment is never folded away, so the Comment stays in view.
    pub(super) fn refold(&mut self) {
        let set = self
            .diff_key(self.selected)
            .and_then(|k| self.folds.revealed.get(&k));
        let commented: HashSet<(Side, usize)> = self
            .comments
            .all()
            .iter()
            .filter_map(|c| self.place_of(c))
            .map(|(side, _, end)| (side, end))
            .collect();
        let revealed: Vec<bool> = (0..self.folds.changed.len())
            .map(|e| {
                let (old, new) = self.entry_lines(e);
                !self.folds.changed[e]
                    && (set.is_some_and(|s| s.contains(&(old, new)))
                        || old.is_some_and(|l| commented.contains(&(Side::Old, l)))
                        || new.is_some_and(|l| commented.contains(&(Side::New, l))))
            })
            .collect();
        self.view = fold(&self.folds.changed, &revealed);
    }

    /// Where the diff cursor is, to tell later which way it moved.
    pub(super) fn cursor_spot(&self) -> CursorSpot {
        CursorSpot {
            file: self.selected,
            items: self.view.items.len(),
            cursor: self.vp.cursor,
        }
    }

    /// Note which way the diff cursor went from `before`. A cursor moved by
    /// another file or by folding does not count: the Reviewer did not walk.
    pub(super) fn note_cursor_move(&mut self, before: CursorSpot) {
        let now = self.cursor_spot();
        if now.file == before.file && now.items == before.items && now.cursor != before.cursor {
            self.folds.came_up = now.cursor < before.cursor;
        }
    }

    /// Whether the diff cursor stands on a fold.
    pub(super) fn on_fold(&self) -> bool {
        matches!(self.view.items.get(self.vp.cursor), Some(Item::Fold { .. }))
    }

    /// `zo`, `zO`, `za`, `zc`, `zR` and `zM` in the diff pane.
    pub(super) fn diff_fold_command(&mut self, command: char) {
        let Some(&item) = self.view.items.get(self.vp.cursor) else {
            return;
        };
        match (command, item) {
            ('o' | 'a', Item::Fold { from, len }) => self.open_fold(from, len, false),
            ('O', Item::Fold { from, len }) => self.open_fold(from, len, true),
            ('c' | 'a', Item::Line(e)) => self.close_fold(e),
            ('o' | 'O' | 'c', _) => self.message = Some("No folded lines here".into()),
            ('R', item) => {
                let all: Vec<usize> = (0..self.folds.changed.len())
                    .filter(|&e| !self.folds.changed[e])
                    .collect();
                self.reveal(&all, true);
                self.refold_on(item.first());
            }
            ('M', item) => {
                if let Some(key) = self.diff_key(self.selected) {
                    self.folds.revealed.remove(&key);
                }
                self.refold_on(item.first());
            }
            _ => {}
        }
    }

    /// A click on view item `item`: a fold opens.
    pub(super) fn click_fold(&mut self, item: usize) {
        if let Some(&Item::Fold { from, len }) = self.view.items.get(item) {
            self.vp.cursor = item;
            self.open_fold(from, len, false);
        }
    }

    /// Reveal the fold of `len` lines from entry `from`: all of it when
    /// `whole` or small, else a chunk on the side the cursor came from. The
    /// cursor stays on what is left of the fold, or goes to its first line.
    fn open_fold(&mut self, from: usize, len: usize, whole: bool) {
        let end = from + len;
        let (shown, rest) = if whole || len <= BIG_FOLD {
            (from..end, from)
        } else {
            // A fold at the top of the file only has a change below it, one
            // at the bottom only above it.
            let from_below = if from == 0 {
                true
            } else if end == self.folds.changed.len() {
                false
            } else {
                self.folds.came_up
            };
            if from_below {
                (end - CHUNK..end, from)
            } else {
                (from..from + CHUNK, from + CHUNK)
            }
        };
        self.reveal(&shown.collect::<Vec<_>>(), true);
        self.refold_on(rest);
    }

    /// `zc` on entry `e`: fold the unchanged stretch around it back.
    fn close_fold(&mut self, e: usize) {
        let changed = &self.folds.changed;
        if changed[e] {
            self.message = Some("No revealed lines here".into());
            return;
        }
        let start = (0..e).rev().find(|&k| changed[k]).map_or(0, |k| k + 1);
        let end = (e..changed.len())
            .find(|&k| changed[k])
            .unwrap_or(changed.len());
        if !self.reveal(&(start..end).collect::<Vec<_>>(), false) {
            self.message = Some("No revealed lines here".into());
            return;
        }
        self.refold_on(e);
    }

    /// Mark `entries` revealed (or not) in the diff on screen; whether that
    /// changed anything.
    fn reveal(&mut self, entries: &[usize], shown: bool) -> bool {
        let Some(key) = self.diff_key(self.selected) else {
            return false;
        };
        let lines: Vec<LineNumbers> = entries.iter().map(|&e| self.entry_lines(e)).collect();
        let set = self.folds.revealed.entry(key).or_default();
        let mut any = false;
        for l in lines {
            any |= if shown { set.insert(l) } else { set.remove(&l) };
        }
        any
    }

    /// Fold again and put the cursor on the item that shows entry `e`.
    fn refold_on(&mut self, e: usize) {
        // View indexes mean other lines now.
        self.visual = None;
        self.drop_mouse_selection();
        self.refold();
        if let Some(i) = self.view.items.iter().position(|item| item.covers(e)) {
            let (total, height) = (self.line_count(), self.viewport_height());
            self.vp.jump_to(i, total, height);
        }
        self.clamp_view();
    }
}

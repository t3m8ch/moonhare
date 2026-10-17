//! Long commit lists: steps through the list by its height, and the part of
//! the list that is on screen.

use std::cell::Cell;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use rounds::{ListRow, Span};

/// What the long commit list remembers between keys and frames.
#[derive(Default)]
pub(super) struct State {
    /// The list row at the top of the scrolling part when last drawn.
    top: Cell<usize>,
    /// The last `/` search, which `n` and `N` repeat.
    query: Option<String>,
    /// The numbers of the folded Rounds.
    folded: std::collections::BTreeSet<usize>,
}

impl State {
    pub(super) fn is_folded(&self, round: usize) -> bool {
        self.folded.contains(&round)
    }
}

/// The part of the commit list that is on screen.
pub(super) struct Shown {
    /// The Round the first row belongs to, pinned under the "Весь Range" row
    /// once the list is scrolled.
    pub pinned: Option<Span>,
    pub rows: Vec<ListRow>,
}

impl Session {
    /// Keys of the commit list that only matter for long Ranges; returns
    /// whether the key was used.
    pub(super) fn list_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.pending == Some('z') {
            self.pending = None;
            if let KeyCode::Char(c) = key.code {
                self.fold_command(c);
            }
            return true;
        }
        if self.pending.is_some() {
            return false;
        }
        match key.code {
            KeyCode::Char('z') if !ctrl => self.pending = Some('z'),
            KeyCode::Char('/') if !ctrl => self.mode = Mode::Search(String::new()),
            KeyCode::Char('n') if !ctrl => self.jump_to_match(true),
            KeyCode::Char('N') if !ctrl => self.jump_to_match(false),
            KeyCode::Char('d') if ctrl => self.step_list(self.list_step() as isize),
            KeyCode::Char('u') if ctrl => self.step_list(-(self.list_step() as isize)),
            _ => return false,
        }
        true
    }

    /// Keys while a `/` search is being typed.
    pub(super) fn search_key(&mut self, key: KeyEvent) {
        let Mode::Search(buf) = &mut self.mode else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Backspace => {
                if buf.pop().is_none() {
                    self.mode = Mode::Normal;
                }
            }
            KeyCode::Enter => {
                let query = std::mem::take(buf);
                self.mode = Mode::Normal;
                if !query.is_empty() {
                    self.list.query = Some(query);
                }
                self.jump_to_match(true);
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => buf.push(c),
            _ => {}
        }
    }

    /// Whether commit `index` answers the search `query` (lower case): by its
    /// number, by the start of its sha, or by its subject. A query that is a
    /// number of the Range names that commit, not the shas that start with
    /// those digits.
    fn commit_matches(&self, index: usize, query: &str) -> bool {
        let commit = &self.commits[index];
        let number = query
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=self.commits.len()).contains(n));
        match number {
            Some(n) if n == index + 1 => return true,
            Some(_) => {}
            None if commit.sha.starts_with(query) => return true,
            None => {}
        }
        commit.subject.to_lowercase().contains(query)
    }

    /// Move the cursor to the next (or previous) commit that answers the last
    /// search, wrapping around the ends of the list.
    pub(super) fn jump_to_match(&mut self, forward: bool) {
        let Some(query) = self.list.query.clone() else {
            self.message = Some("No previous search".into());
            return;
        };
        let lower = query.to_lowercase();
        let hits: Vec<usize> = (0..self.commits.len())
            .filter(|&i| self.commit_matches(i, &lower))
            .collect();
        let cursor = self.commit_cursor;
        let target = if forward {
            hits.iter().position(|&i| i + 1 > cursor).or(Some(0))
        } else {
            hits.iter()
                .rposition(|&i| i + 1 < cursor)
                .or(hits.len().checked_sub(1))
        };
        let Some(k) = target.filter(|_| !hits.is_empty()) else {
            self.message = Some(format!("No match: {query}"));
            return;
        };
        self.commit_cursor = hits[k] + 1;
        self.reveal_commit(hits[k]);
        self.message = Some(format!("/{query} [{}/{}]", k + 1, hits.len()));
    }

    /// How many lines the scrolling part of the list has under the "Весь
    /// Range" row and its Comments, inside a box of `inner_height` lines.
    pub(super) fn list_capacity(&self, inner_width: u16, inner_height: u16) -> usize {
        let frame = self.range_frame_in(inner_width, inner_height).len();
        (inner_height as usize)
            .saturating_sub(1)
            .saturating_sub(frame)
    }

    /// The rows that fit in `capacity` lines, scrolled just far enough to
    /// keep the cursor in view. A scrolled list gives one line to the header
    /// of the Round it is scrolled into.
    pub(super) fn shown_rows(&self, capacity: usize) -> Shown {
        let rows = self.list_rows();
        let at = (self.commit_cursor > 0)
            .then(|| rows.iter().position(|r| r.holds(self.commit_cursor - 1)))
            .flatten();
        let mut top = match at {
            Some(_) => self.list.top.get().min(rows.len().saturating_sub(1)),
            None => 0,
        };
        let fit = |capacity: usize, top: usize| match at {
            // Keep the header of the Round just above in view.
            Some(at) if at < top && at > 0 && matches!(rows[at - 1], ListRow::Header(_)) => at - 1,
            Some(at) if at < top => at,
            Some(at) if at >= top + capacity => at + 1 - capacity,
            _ => top,
        };
        top = fit(capacity, top);
        let mut pinned = None;
        if top > 0 && capacity > 1 {
            if let Some(ListRow::Commit(_)) = rows.get(top) {
                top = fit(capacity - 1, top);
                if let Some(ListRow::Commit(c)) = rows.get(top) {
                    pinned = self
                        .spans()
                        .into_iter()
                        .find(|s| (s.start..s.end).contains(c));
                }
            }
        }
        self.list.top.set(top);
        let take = capacity - usize::from(pinned.is_some());
        Shown {
            pinned,
            rows: rows.into_iter().skip(top).take(take).collect(),
        }
    }

    /// A click on screen row `screen_row` inside the commit list box: it
    /// focuses the list and moves the cursor to what was clicked. Round
    /// headers are not commits, so clicking one only focuses; the pinned
    /// header goes to the first commit of its Round.
    pub(super) fn click_commits(&mut self, screen_row: u16) {
        let area = self.commits_area();
        let row = (screen_row - area.y) as usize;
        let frame = self.range_frame_in(area.width - 2, area.height - 2).len();
        let capacity = self.list_capacity(area.width - 2, area.height - 2);
        // Row 0 is the border, row 1 the pinned "Весь Range" row, the Range
        // Comments hang under it.
        if row < 1 || row > capacity + 1 + frame {
            return;
        }
        self.focus = Focus::Commits;
        if row <= 1 + frame {
            self.commit_cursor = 0;
            return;
        }
        let mut k = row - 2 - frame;
        let shown = self.shown_rows(capacity);
        if let Some(span) = &shown.pinned {
            if k == 0 {
                self.commit_cursor = span.start + 1;
                return;
            }
            k -= 1;
        }
        match shown.rows.get(k) {
            Some(ListRow::Commit(i)) => self.commit_cursor = i + 1,
            Some(ListRow::Folded { start, .. }) => {
                // Clicking a folded Round opens it.
                self.commit_cursor = start + 1;
                self.fold_command('o');
            }
            _ => {}
        }
    }

    /// `za`, `zo` and `zc` on the Round the cursor stands in.
    fn fold_command(&mut self, command: char) {
        let spans = self.spans();
        if spans.is_empty() {
            self.message = Some("No Rounds yet: they start with the first Prompt".into());
            return;
        }
        let Some(span) = (self.commit_cursor > 0)
            .then(|| {
                spans
                    .iter()
                    .find(|s| (s.start..s.end).contains(&(self.commit_cursor - 1)))
            })
            .flatten()
        else {
            self.message = Some("Put the cursor on a commit to fold its Round".into());
            return;
        };
        let fold = match command {
            'a' => !self.list.is_folded(span.number),
            'c' => true,
            'o' => false,
            _ => return,
        };
        if fold {
            self.list.folded.insert(span.number);
            // The cursor stands on the folded row, at the Round's first commit.
            self.commit_cursor = span.start + 1;
        } else {
            self.list.folded.remove(&span.number);
        }
    }

    /// Unfold the Round that holds commit `index`, so the cursor can show it.
    pub(super) fn reveal_commit(&mut self, index: usize) {
        for span in self.spans() {
            if (span.start..span.end).contains(&index) {
                self.list.folded.remove(&span.number);
            }
        }
    }

    /// Half of the rows the commit list shows, at least one.
    fn list_step(&self) -> usize {
        let area = if self.picker {
            self.picker_area()
        } else {
            self.commits_area()
        };
        let inner = if self.picker { 3 } else { 2 };
        let rows = self.list_capacity(
            area.width.saturating_sub(2),
            area.height.saturating_sub(inner),
        );
        (rows / 2).max(1)
    }

    /// The cursor positions the list can stand on, in order: the "Весь Range"
    /// row, then every commit that has a row of its own.
    pub(super) fn stops(&self) -> Vec<usize> {
        let mut stops = vec![0];
        for row in self.list_rows() {
            match row {
                ListRow::Commit(i) => stops.push(i + 1),
                ListRow::Folded { start, .. } => stops.push(start + 1),
                ListRow::Header(_) => {}
            }
        }
        stops
    }

    /// Move the commit-list cursor `delta` stops, staying inside the list.
    pub(super) fn step_list(&mut self, delta: isize) {
        let stops = self.stops();
        // A cursor hidden in a folded Round stands on the Round's row.
        let at = stops
            .partition_point(|&s| s <= self.commit_cursor)
            .saturating_sub(1);
        // A selection holds commits only: the Range row is out of reach.
        let floor = usize::from(self.commit_anchor.is_some());
        let to = at
            .saturating_add_signed(delta)
            .clamp(floor, stops.len() - 1);
        self.commit_cursor = stops[to];
    }
}

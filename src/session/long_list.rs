//! Long commit lists: steps through the list by its height.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;

impl Session {
    /// Keys of the commit list that only matter for long Ranges; returns
    /// whether the key was used.
    pub(super) fn list_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.pending.is_some() {
            return false;
        }
        match key.code {
            KeyCode::Char('d') if ctrl => self.step_list(self.list_step() as isize),
            KeyCode::Char('u') if ctrl => self.step_list(-(self.list_step() as isize)),
            _ => return false,
        }
        true
    }

    /// Half of the rows the commit list shows, at least one.
    fn list_step(&self) -> usize {
        let area = self.commits_area();
        let frame = self.range_frame_in(area.width, area.height).len();
        let rows = (area.height.saturating_sub(3) as usize).saturating_sub(frame);
        (rows / 2).max(1)
    }

    /// The cursor positions the list can stand on, in order: the "Весь Range"
    /// row, then every commit that has a row of its own.
    pub(super) fn stops(&self) -> Vec<usize> {
        let mut stops = vec![0];
        for row in self.list_rows() {
            if let rounds::ListRow::Commit(i) = row {
                stops.push(i + 1);
            }
        }
        stops
    }

    /// Move the commit-list cursor `delta` stops, staying inside the list.
    pub(super) fn step_list(&mut self, delta: isize) {
        let stops = self.stops();
        let at = stops.binary_search(&self.commit_cursor).unwrap_or(0);
        // A selection holds commits only: the Range row is out of reach.
        let floor = usize::from(self.commit_anchor.is_some());
        let to = at
            .saturating_add_signed(delta)
            .clamp(floor, stops.len() - 1);
        self.commit_cursor = stops[to];
    }
}

//! Mechanical files in the session: the diff is collapsed to a note until the
//! Reviewer expands it with `Enter`.

use super::*;

impl Session {
    /// Whether the selected file is a Mechanical one still collapsed.
    pub(super) fn collapsed(&self) -> bool {
        !self.mechanical_open
            && self
                .files
                .get(self.selected)
                .is_some_and(|f| f.mechanical.is_some())
    }

    /// `Enter`: show the diff of the selected collapsed Mechanical file.
    pub(super) fn expand_mechanical(&mut self) {
        if self.collapsed() {
            self.mechanical_open = true;
            self.dirty = true;
        }
    }

    pub(super) fn render_collapsed(
        &self,
        buf: &mut Buffer,
        file: &ChangedFile,
        area: Rect,
        top: u16,
    ) {
        let Some(kind) = file.mechanical else { return };
        let text = format!(
            "⋯ mechanical ({}): collapsed, Enter to expand",
            kind.label()
        );
        buf.set_stringn(
            area.x + 1,
            top,
            &text,
            (area.width as usize).saturating_sub(1),
            Style::new().fg(self.theme().dim),
        );
    }
}

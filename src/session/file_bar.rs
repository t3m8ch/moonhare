//! The file bar above the diff: path, state and position in the file list.
//! It is the same with and without the sidebar, so the status line below
//! never repeats it.

use super::*;

impl Session {
    /// The text of the file bar for the selected file.
    pub(super) fn file_bar(&self, file: &ChangedFile, diff: &FileDiff) -> String {
        let mut bar = special::header(file, diff);
        let state = match file.status {
            Status::Added => Some("added"),
            Status::Modified => Some("modified"),
            Status::Deleted => Some("deleted"),
            // The header already says "(renamed)".
            Status::Renamed => None,
        };
        if let Some(state) = state {
            bar.push_str(&format!("· {state} "));
        }
        bar.push_str(&format!(
            "· file {}/{} ",
            self.selected + 1,
            self.files.len()
        ));
        bar
    }
}

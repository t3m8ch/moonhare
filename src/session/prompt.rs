//! Prompts in the session: copying Comments to the clipboard.

use super::*;
use crate::comments::{State, Target};
use crate::hints::SPACE_P;
use crate::prompt::{build, Preamble};

/// Where a Prompt goes.
enum Delivery {
    Clipboard,
    File(std::path::PathBuf),
}

impl Session {
    /// The `Space p` keys; returns whether the key was one of them.
    pub(super) fn prompt_key(&mut self, key: KeyEvent) -> bool {
        if let Some((text, offset)) = &mut self.prompt_view {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.prompt_view = None,
                KeyCode::Char('j') | KeyCode::Down => {
                    *offset = (*offset + 1).min(text.lines().count().saturating_sub(1))
                }
                KeyCode::Char('k') | KeyCode::Up => *offset = offset.saturating_sub(1),
                _ => {}
            }
            return true;
        }
        match (self.pending, key.code) {
            (Some(' '), KeyCode::Char('p')) => self.pending = Some(SPACE_P),
            (Some(SPACE_P), code) => {
                self.pending = None;
                self.count = None;
                match code {
                    KeyCode::Char('p') => self.copy_prompt(),
                    KeyCode::Char('f') => self.copy_file_prompt(),
                    KeyCode::Char('c') => self.copy_cursor_prompt(),
                    KeyCode::Char('s') => self.show_prompt(),
                    _ => {}
                }
            }
            _ => return false,
        }
        true
    }

    /// `Y`: copy every open Comment as a Prompt.
    pub(super) fn copy_prompt(&mut self) {
        let ids = self.open_comment_ids();
        self.copy_comments(&ids, "No open Comments to copy");
    }

    /// `Space p f`: copy the open Comments on the displayed file.
    fn copy_file_prompt(&mut self) {
        let Some(file) = self.files.get(self.selected) else {
            self.message = Some("No file open".into());
            return;
        };
        let ids: Vec<u64> = (self.comments.all().iter())
            .filter(|c| c.state == State::Open)
            .filter(|c| match &c.target {
                Target::Lines { path, .. } | Target::File { path } => *path == file.path,
                Target::Range => false,
            })
            .map(|c| c.id)
            .collect();
        self.copy_comments(&ids, "No open Comments on this file");
    }

    /// `Space p c`: copy the Comment under the cursor (one that is closed is
    /// dealt with and stays out).
    fn copy_cursor_prompt(&mut self) {
        let ids: Vec<u64> = (self.comments.all().iter())
            .filter(|c| c.state != State::Closed)
            .map(|c| c.id)
            .filter(|id| self.comments_at_cursor().contains(id))
            .collect();
        self.copy_comments(&ids, "No Comment here");
    }

    /// `Space p s`: show the Prompt of the open Comments in an overlay.
    /// Looking is not sending: no Comment changes state.
    pub(super) fn show_prompt(&mut self) {
        let ids = self.open_comment_ids();
        if ids.is_empty() {
            self.message = Some("No open Comments to show".into());
            return;
        }
        self.prompt_view = Some((self.prompt_text(&ids), 0));
    }

    pub(super) fn render_prompt_view(&self, buf: &mut Buffer) {
        let Some((text, offset)) = &self.prompt_view else {
            return;
        };
        let area = Rect::new(
            1,
            0,
            self.size.0.saturating_sub(2),
            self.size.1.saturating_sub(BOTTOM_ROWS),
        );
        for y in area.y..area.y + area.height {
            buf.set_string(
                area.x,
                y,
                " ".repeat(area.width as usize),
                Style::new().fg(FG),
            );
        }
        draw_box(buf, area, " Prompt (j/k scroll, Esc close) ");
        let inner = area.width.saturating_sub(4) as usize;
        let rows = area.height.saturating_sub(2) as usize;
        for (i, line) in super::comments::wrap(text, inner)
            .iter()
            .skip(*offset)
            .take(rows)
            .enumerate()
        {
            buf.set_stringn(
                area.x + 2,
                area.y + 1 + i as u16,
                line,
                inner,
                Style::new().fg(FG),
            );
        }
    }

    fn open_comment_ids(&self) -> Vec<u64> {
        (self.comments.all().iter())
            .filter(|c| c.state == State::Open)
            .map(|c| c.id)
            .collect()
    }

    /// Copy the Comments with these ids as a Prompt and mark them sent;
    /// `none` is what to tell the Reviewer when there is nothing to copy.
    fn copy_comments(&mut self, ids: &[u64], none: &str) {
        self.deliver(ids, none, Delivery::Clipboard);
    }

    /// `:prompt save [path]`: write the Prompt of the open Comments to a file
    /// instead of the clipboard. It hands the Prompt over just the same.
    fn save_prompt(&mut self, path: &str) {
        let ids = self.open_comment_ids();
        let path = match path {
            "" => (self.review.dir()).join(format!("prompt-{}.md", self.prompts.next_number())),
            given => std::path::PathBuf::from(given),
        };
        self.deliver(&ids, "No open Comments to save", Delivery::File(path));
    }

    /// Hand the Prompt of these Comments over and mark them sent.
    fn deliver(&mut self, ids: &[u64], none: &str, how: Delivery) {
        if ids.is_empty() {
            self.message = Some(none.to_string());
            return;
        }
        let text = self.prompt_text(ids);
        if let Delivery::File(path) = &how {
            if let Err(e) = std::fs::write(path, &text) {
                self.message = Some(format!("Cannot save the Prompt: {e}"));
                return;
            }
        }
        let sequence = match &how {
            Delivery::Clipboard => match crate::clipboard::osc52(&text) {
                Some(sequence) => Some(sequence),
                None => {
                    self.message = Some(format!(
                        "Prompt is too big for the clipboard ({} KB, at most {}): use :prompt save",
                        text.len() / 1000,
                        crate::clipboard::MAX_TEXT / 1000
                    ));
                    return;
                }
            },
            Delivery::File(_) => None,
        };
        // Copying closes the Round: remember where the Range stood.
        let number = match self.prompts.record(&self.range.head, self.commits.len()) {
            Ok(number) => number,
            Err(e) => {
                self.message = Some(e);
                return;
            }
        };
        let done = match how {
            Delivery::Clipboard => {
                self.terminal_out.extend(sequence);
                format!("Prompt #{number} copied: {} Comments", ids.len())
            }
            Delivery::File(path) => format!(
                "Prompt #{number} saved ({} Comments): {}",
                ids.len(),
                path.display()
            ),
        };
        match self.comments.set_state(ids, State::Sent) {
            Ok(()) => self.message = Some(done),
            Err(e) => self.message = Some(e),
        }
    }

    /// `:prompt show` and `:prompt save`; returns whether it was one.
    pub(super) fn prompt_command(&mut self, command: &str) -> bool {
        let Some(rest) = command.strip_prefix("prompt") else {
            return false;
        };
        match rest.trim() {
            "show" => self.show_prompt(),
            "save" => self.save_prompt(""),
            arg if arg.strip_prefix("save").is_some_and(|a| a.starts_with(' ')) => {
                self.save_prompt(arg["save".len()..].trim())
            }
            _ => self.message = Some("Usage: :prompt show | :prompt save [path]".into()),
        }
        true
    }

    fn prompt_text(&self, ids: &[u64]) -> String {
        let comments: Vec<_> = (self.comments.all().iter())
            .filter(|c| ids.contains(&c.id))
            .collect();
        let text = build(
            &Preamble {
                number: self.prompts.next_number(),
                branch: &self.range.head_name,
                base: &self.range.base,
                head: &self.range.head,
                commits: self.commits.len(),
            },
            &comments,
        );
        crate::clipboard::sanitize(&text)
    }
}

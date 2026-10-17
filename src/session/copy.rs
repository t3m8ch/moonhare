//! Copying code from the diff: what text a selection gives, and the one way
//! text reaches the clipboard (sanitized, size-limited OSC 52).

use super::*;

/// How a copied line stands in the change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mark {
    Context,
    Removed,
    Added,
}

/// One line of a selection: its code and where it comes from.
#[derive(Debug, Clone)]
pub(super) struct CopyLine {
    pub(super) mark: Mark,
    pub(super) side: Side,
    pub(super) line: usize,
    pub(super) text: String,
}

/// The text of the lines. A mix of removed and added lines is a diff, each
/// line with its `+`, `-` or space prefix; anything else is the plain code,
/// because a single side's code reads as it is.
pub(super) fn format(lines: &[CopyLine]) -> String {
    let mixed = lines.iter().any(|l| l.mark == Mark::Removed)
        && lines.iter().any(|l| l.mark == Mark::Added);
    lines
        .iter()
        .map(|l| {
            let prefix = match (mixed, l.mark) {
                (false, _) => "",
                (true, Mark::Context) => " ",
                (true, Mark::Removed) => "-",
                (true, Mark::Added) => "+",
            };
            format!("{prefix}{}", l.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The part of `text` inside the cell columns `from..to`; a tab fills
/// [`TAB_WIDTH`] cells and is kept when any of them is inside.
pub(super) fn cut(text: &str, from: usize, to: usize) -> String {
    let mut cell = 0;
    let mut out = String::new();
    for c in text.chars() {
        let width = if c == '\t' { TAB_WIDTH } else { 1 };
        if cell < to && cell + width > from {
            out.push(c);
        }
        cell += width;
    }
    out
}

impl Session {
    /// Put `text` on the Reviewer's clipboard, or say why it cannot be.
    /// `what` names it in the message.
    pub(super) fn copy_to_clipboard(&mut self, text: &str, what: &str) {
        let text = crate::clipboard::sanitize(text);
        match crate::clipboard::osc52_limited(&text) {
            Some(sequence) => {
                self.terminal_out.push(sequence);
                self.message = Some(format!("Copied {what}"));
            }
            None => {
                self.message = Some(format!(
                    "Too big for the clipboard ({} KB, at most {}): not copied",
                    text.len() / 1000,
                    crate::clipboard::MAX_TEXT / 1000
                ));
            }
        }
    }
}

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
pub(super) fn format(lines: &[CopyLine], notes: &[Note]) -> String {
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
            let mut out = format!("{prefix}{}", l.text);
            // Comments on the line follow it, quoted.
            for note in notes
                .iter()
                .filter(|n| (n.side, n.line) == (l.side, l.line))
            {
                for text_line in note.text.lines() {
                    out.push_str(&format!("\n> {text_line}"));
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A Comment that goes along with the code it is on.
pub(super) struct Note {
    pub(super) side: Side,
    /// The last line the Comment is on.
    pub(super) line: usize,
    pub(super) text: String,
}

/// The cell that follows a wide character in a row of cells.
pub(super) const CONTINUATION: char = '\0';

/// Cells a character takes: a tab [`TAB_WIDTH`], a wide one (CJK, emoji) two,
/// a combining mark none.
pub(super) fn cell_width(c: char) -> usize {
    if c == '\t' {
        TAB_WIDTH
    } else {
        unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)
    }
}

/// Cells a text takes.
pub(super) fn text_width(text: &str) -> usize {
    text.chars().map(cell_width).sum()
}

/// The part of `text` inside the cell columns `from..to`; a tab fills
/// [`TAB_WIDTH`] cells and is kept when any of them is inside.
pub(super) fn cut(text: &str, from: usize, to: usize) -> String {
    let mut cell = 0;
    let mut out = String::new();
    for c in text.chars() {
        let width = cell_width(c);
        if cell < to && cell + width > from {
            out.push(c);
        }
        cell += width;
    }
    out
}

/// The OSC 52 sequence for `text` (sanitized), or `None` when it is too
/// long for one. The one place text is turned into a clipboard sequence.
pub(super) fn osc52_for(text: &str) -> Option<String> {
    crate::clipboard::osc52_limited(&crate::clipboard::sanitize(text))
}

impl Session {
    /// `y` and `Space y`: copy the lines of the VISUAL selection or of the
    /// mouse selection, with the Comments on them when asked to.
    pub(super) fn yank(&mut self, with_comments: bool) {
        let lines = if let Some(selection) = self.mouse.selection {
            self.selected_lines(selection)
        } else if let Some(anchor) = self.visual.take() {
            let (lo, hi) = (anchor.min(self.vp.cursor), anchor.max(self.vp.cursor));
            (lo..=hi).flat_map(|i| self.whole_lines(i)).collect()
        } else {
            self.message = Some("Nothing selected: v or the mouse selects code".into());
            return;
        };
        let notes = if with_comments {
            self.notes_on(&lines)
        } else {
            Vec::new()
        };
        let what = if with_comments {
            "the code with Comments"
        } else {
            "the code"
        };
        self.copy_to_clipboard(&format(&lines, &notes), what);
    }

    /// The Comments of the displayed file that end on one of `lines`.
    fn notes_on(&self, lines: &[CopyLine]) -> Vec<Note> {
        let mut notes = Vec::new();
        for c in self.comments.all() {
            let Some((side, _, end)) = self.place_of(c) else {
                continue;
            };
            if lines.iter().any(|l| (l.side, l.line) == (side, end)) {
                notes.push(Note {
                    side,
                    line: end,
                    text: c.text.clone(),
                });
            }
        }
        notes
    }

    /// Put `text` on the Reviewer's clipboard, or say why it cannot be.
    /// `what` names it in the message.
    pub(super) fn copy_to_clipboard(&mut self, text: &str, what: &str) {
        let text = crate::clipboard::sanitize(text);
        if text.is_empty() {
            self.message = Some("Nothing to copy".into());
            return;
        }
        match crate::clipboard::osc52_limited(&text) {
            Some(sequence) => {
                self.terminal_out.push(sequence);
                self.message = Some(format!("Copied {what}"));
            }
            None => {
                self.message = Some(format!(
                    "Too large for the clipboard ({} KB, at most {}): not copied",
                    text.len() / 1000,
                    crate::clipboard::MAX_TEXT / 1000
                ));
            }
        }
    }
}

/// The cell columns `from..to` of the word at cell column `col` of `text`:
/// a run of letters, digits and `_`, of blanks, or one other character.
pub(super) fn word_span(text: &str, col: usize) -> Option<(usize, usize)> {
    let mut cells = Vec::new();
    let mut cell = 0;
    for c in text.chars() {
        let width = cell_width(c);
        cells.push((c, cell, cell + width));
        cell += width;
    }
    let at = cells
        .iter()
        .position(|&(_, from, to)| from <= col && col < to)?;
    let class = |c: char| match c {
        c if c.is_alphanumeric() || c == '_' => 0,
        c if c.is_whitespace() => 1,
        _ => 2,
    };
    let kind = class(cells[at].0);
    if kind == 2 {
        return Some((cells[at].1, cells[at].2));
    }
    let mut first = at;
    while first > 0 && class(cells[first - 1].0) == kind {
        first -= 1;
    }
    let mut last = at;
    while last + 1 < cells.len() && class(cells[last + 1].0) == kind {
        last += 1;
    }
    Some((cells[first].1, cells[last].2))
}

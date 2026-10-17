//! Normal-mode motions inside the diff pane: counts, the cursor, Vim scrolls,
//! hunk jumps and horizontal scrolling.

use ratatui::crossterm::event::KeyCode;

use super::*;

impl Session {
    pub(super) fn scroll_view(&mut self, delta: isize) {
        self.vp
            .scroll_view(delta, self.line_count(), self.viewport_height());
    }

    /// Handle a Normal-mode key that moves around; returns whether it did.
    /// Digits build a count that the next motion consumes.
    pub(super) fn nav_key(&mut self, code: KeyCode, ctrl: bool) -> bool {
        let (total, height) = (self.line_count(), self.viewport_height());

        if let Some(prefix) = self.pending.take() {
            let count = self.count.take();
            match (prefix, code) {
                ('g', KeyCode::Char('g')) => self.go_to_line(count),
                (']', KeyCode::Char('f')) => {
                    let last = self.files.len().saturating_sub(1);
                    self.select(self.selected.saturating_add(count.unwrap_or(1)).min(last));
                }
                ('[', KeyCode::Char('f')) => {
                    self.select(self.selected.saturating_sub(count.unwrap_or(1)));
                }
                (']', KeyCode::Char('g')) => self.jump_group(true, count.unwrap_or(1)),
                ('[', KeyCode::Char('g')) => self.jump_group(false, count.unwrap_or(1)),
                (']', KeyCode::Char('u')) => self.jump_unreviewed(true),
                ('[', KeyCode::Char('u')) => self.jump_unreviewed(false),
                (']', KeyCode::Char('h')) => self.jump_hunk(true, count.unwrap_or(1)),
                ('[', KeyCode::Char('h')) => self.jump_hunk(false, count.unwrap_or(1)),
                _ => {}
            }
            return true;
        }

        if let KeyCode::Char(c @ '0'..='9') = code {
            // A lone `0` is a command; after other digits it extends the count.
            if !ctrl && (c != '0' || self.count.is_some()) {
                let digit = c as usize - '0' as usize;
                self.count = Some(
                    self.count
                        .unwrap_or(0)
                        .saturating_mul(10)
                        .saturating_add(digit),
                );
                return true;
            }
        }

        let count = self.count.take();
        let n = count.unwrap_or(1);
        let lines = n.min(isize::MAX as usize) as isize;
        match code {
            KeyCode::Char('d') if ctrl => self.vp.half_page(true, total, height),
            KeyCode::Char('u') if ctrl => self.vp.half_page(false, total, height),
            KeyCode::Char('f') if ctrl => self.vp.page(true, total, height),
            KeyCode::Char('b') if ctrl => self.vp.page(false, total, height),
            KeyCode::Char('e') if ctrl => self.vp.scroll_view(lines, total, height),
            KeyCode::Char('y') if ctrl => self.vp.scroll_view(-lines, total, height),
            KeyCode::Char('j') | KeyCode::Down if !ctrl => {
                self.vp.move_cursor(lines, total, height)
            }
            KeyCode::Char('k') | KeyCode::Up if !ctrl => self.vp.move_cursor(-lines, total, height),
            KeyCode::Char('G') => match count {
                Some(_) => self.go_to_line(count),
                None => self.vp.jump_to(total.saturating_sub(1), total, height),
            },
            KeyCode::Char('l') if !ctrl => {
                self.hscroll = self
                    .hscroll
                    .saturating_add(n.saturating_mul(HSCROLL_STEP))
                    .min(self.max_hscroll());
            }
            KeyCode::Char('h') if !ctrl => {
                self.hscroll = self.hscroll.saturating_sub(n.saturating_mul(HSCROLL_STEP));
            }
            KeyCode::Char('0') if !ctrl => self.hscroll = 0,
            KeyCode::Char(c @ ('g' | ']' | '[')) if !ctrl => {
                self.pending = Some(c);
                self.count = count;
            }
            _ => return ctrl,
        }
        true
    }

    /// `gg` / `Ngg` / `NG`: the top, or the line numbered `target` in the
    /// file (new side where there is one).
    fn go_to_line(&mut self, target: Option<usize>) {
        let (total, height) = (self.line_count(), self.viewport_height());
        let index = match target {
            None => 0,
            Some(n) => self
                .view
                .items
                .iter()
                .position(|&item| {
                    let last = match item {
                        Item::Line(e) => e,
                        Item::Fold { from, len } => from + len - 1,
                    };
                    self.entry_lines(last).1.is_some_and(|l| l + 1 >= n)
                })
                .unwrap_or(total.saturating_sub(1)),
        };
        self.vp.jump_to(index, total, height);
    }

    /// `]h` / `[h`: move to the start of the `n`th next or previous hunk.
    fn jump_hunk(&mut self, forward: bool, n: usize) {
        let cursor = self.vp.cursor;
        let target = if forward {
            let mut after = self.view.hunks.iter().filter(|&&h| h > cursor);
            after.clone().nth(n - 1).or_else(|| after.next_back())
        } else {
            let mut before = self.view.hunks.iter().rev().filter(|&&h| h < cursor);
            before.clone().nth(n - 1).or_else(|| before.next_back())
        };
        match target {
            Some(&hunk) => {
                let (total, height) = (self.line_count(), self.viewport_height());
                self.vp.jump_to(hunk, total, height);
            }
            None if forward => self.message = Some("No more hunks".into()),
            None => self.message = Some("No previous hunk".into()),
        }
    }

    /// The largest horizontal offset: the longest line ends at the pane edge.
    fn max_hscroll(&self) -> usize {
        let Some(Ok(l)) = &self.loaded else { return 0 };
        let digits = digits(l.diff.old_lines.len().max(l.diff.new_lines.len()));
        let width = self.diff_area().width as usize;
        let code_width = match self.layout() {
            DiffLayout::SideBySide => (width.saturating_sub(1) / 2).saturating_sub(digits + 1),
            DiffLayout::Inline => width.saturating_sub(2 * digits + 5),
        };
        l.max_width.saturating_sub(code_width)
    }
}

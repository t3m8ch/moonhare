//! Cursor and scrolling of a line-based pane, with Vim semantics.
//!
//! A [`Viewport`] holds the first visible line (`top`) and the `cursor` line.
//! Every operation takes the total number of lines and the pane height, so
//! the state stays valid when either changes; [`Viewport::clamp`] repairs it.

/// Lines of overlap kept by `Ctrl-f` / `Ctrl-b`, as in Vim.
const PAGE_OVERLAP: usize = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Viewport {
    pub top: usize,
    pub cursor: usize,
}

impl Viewport {
    fn max_top(total: usize, height: usize) -> usize {
        total.saturating_sub(height)
    }

    /// Bring `top` and `cursor` back into range and the cursor into view.
    pub fn clamp(&mut self, total: usize, height: usize) {
        self.cursor = self.cursor.min(total.saturating_sub(1));
        self.top = self.top.min(Self::max_top(total, height));
        self.reveal(height);
    }

    /// Scroll the minimum needed to show the cursor.
    pub fn reveal(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        if self.cursor < self.top {
            self.top = self.cursor;
        } else if self.cursor >= self.top + height {
            self.top = self.cursor + 1 - height;
        }
    }

    /// Put the cursor on `line`; scroll only when it is out of view, then
    /// centre it so a far jump lands in the middle of the pane.
    pub fn jump_to(&mut self, line: usize, total: usize, height: usize) {
        self.cursor = line.min(total.saturating_sub(1));
        if self.cursor < self.top || self.cursor >= self.top + height {
            self.top = self.cursor.saturating_sub(height / 2);
        }
        self.clamp(total, height);
    }

    /// `j` / `k`: move the cursor, the view follows at the edge.
    pub fn move_cursor(&mut self, delta: isize, total: usize, height: usize) {
        self.cursor = self.cursor.saturating_add_signed(delta);
        self.clamp(total, height);
    }

    /// `Ctrl-e` / `Ctrl-y`, the wheel: scroll the view; the cursor stays on
    /// its line until it leaves the view, then it is pushed along.
    pub fn scroll_view(&mut self, delta: isize, total: usize, height: usize) {
        self.top = self
            .top
            .saturating_add_signed(delta)
            .min(Self::max_top(total, height));
        self.cursor = self.cursor.max(self.top);
        if height > 0 {
            self.cursor = self.cursor.min(self.top + height - 1);
        }
        self.clamp(total, height);
    }

    /// `Ctrl-d` / `Ctrl-u`: scroll and move the cursor by the same number of
    /// lines, half a page by default.
    pub fn half_page(&mut self, down: bool, total: usize, height: usize) {
        let amount = (height / 2).max(1);
        let delta = if down {
            amount as isize
        } else {
            -(amount as isize)
        };
        self.top = self
            .top
            .saturating_add_signed(delta)
            .min(Self::max_top(total, height));
        self.cursor = self.cursor.saturating_add_signed(delta);
        self.clamp(total, height);
    }

    /// `Ctrl-f` / `Ctrl-b`: a page at a time, keeping two lines of overlap.
    pub fn page(&mut self, down: bool, total: usize, height: usize) {
        let step = height.saturating_sub(PAGE_OVERLAP).max(1);
        let max_top = Self::max_top(total, height);
        if down {
            if self.top >= max_top {
                self.cursor = total.saturating_sub(1);
            } else {
                self.top = (self.top + step).min(max_top);
                self.cursor = self.top;
            }
        } else if self.top == 0 {
            self.cursor = 0;
        } else {
            self.top = self.top.saturating_sub(step);
            self.cursor = (self.top + height).saturating_sub(1);
        }
        self.clamp(total, height);
    }
}

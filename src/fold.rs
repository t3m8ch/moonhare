//! Folding of long unchanged stretches and the hunks left between them.
//!
//! The input is one `changed` flag per displayed line (a row in side-by-side,
//! a line in inline). The output is the list of [`Item`]s to display: kept
//! lines and [`Item::Fold`]s standing in for hidden unchanged lines. Lines
//! the Reviewer revealed stay shown, cutting a fold into what is left of it.

/// Unchanged lines kept around a change.
pub const CONTEXT: usize = 3;
/// A stretch is folded only when at least this many lines would be hidden.
pub const MIN_FOLD: usize = 5;
/// A fold longer than this is revealed a chunk at a time.
pub const BIG_FOLD: usize = 40;
/// The lines one step reveals of a big fold.
pub const CHUNK: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// The line at this index of the input.
    Line(usize),
    /// `len` unchanged lines starting at input index `from`.
    Fold { from: usize, len: usize },
}

impl Item {
    /// The first input index the item stands for.
    pub fn first(self) -> usize {
        match self {
            Item::Line(e) | Item::Fold { from: e, .. } => e,
        }
    }

    /// Whether the item shows input index `e`, or hides it.
    pub fn covers(self, e: usize) -> bool {
        match self {
            Item::Line(l) => l == e,
            Item::Fold { from, len } => (from..from + len).contains(&e),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Folded {
    pub items: Vec<Item>,
    /// Indexes into `items` where a hunk (a run of changed lines) starts.
    pub hunks: Vec<usize>,
}

/// Fold `changed` lines' unchanged stretches, keeping the lines `revealed`
/// marks (one flag per line, like `changed`) on screen.
pub fn fold(changed: &[bool], revealed: &[bool]) -> Folded {
    let n = changed.len();
    let mut items = Vec::new();
    let mut i = 0;
    while i < n {
        if changed[i] {
            items.push(Item::Line(i));
            i += 1;
            continue;
        }
        let start = i;
        while i < n && !changed[i] {
            i += 1;
        }
        let end = i;
        let lead = if start == 0 { 0 } else { CONTEXT };
        let trail = if end == n { 0 } else { CONTEXT };
        let hidden = (end - start).saturating_sub(lead + trail);
        if hidden >= MIN_FOLD {
            items.extend((start..start + lead).map(Item::Line));
            let mut k = start + lead;
            while k < end - trail {
                if revealed.get(k) == Some(&true) {
                    items.push(Item::Line(k));
                    k += 1;
                    continue;
                }
                let from = k;
                while k < end - trail && revealed.get(k) != Some(&true) {
                    k += 1;
                }
                items.push(Item::Fold {
                    from,
                    len: k - from,
                });
            }
            items.extend((end - trail..end).map(Item::Line));
        } else {
            items.extend((start..end).map(Item::Line));
        }
    }

    let is_changed = |item: &Item| matches!(item, Item::Line(l) if changed[*l]);
    let hunks = (0..items.len())
        .filter(|&k| is_changed(&items[k]) && (k == 0 || !is_changed(&items[k - 1])))
        .collect();
    Folded { items, hunks }
}

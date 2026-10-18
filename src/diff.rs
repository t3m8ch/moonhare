//! Diff engine facade over the vendored difftastic (ADR 0001).
//!
//! [`diff_file`] takes two versions of a file and returns owned data: aligned
//! rows for side-by-side and inline views, changed fragments, and highlighting
//! for every token from the same parse. Nothing here borrows from the engine.

pub use difftastic::{Fallback as FallbackReason, Highlight, Limits, Token};

/// The limits the session diffs with. difftastic's own graph limit (3M nodes)
/// is reached only after several seconds on a few KB of heavily edited code
/// (the search slows down as it grows); past this smaller one the line-based
/// fallback comes within about a second.
pub fn session_limits() -> Limits {
    Limits {
        graph_limit: 1_000_000,
        ..Limits::default()
    }
}

/// Which version of the file a token or fragment belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Old,
    New,
}

/// How the file changed as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Modified,
    Created,
    Deleted,
    Binary,
}

/// One displayed row: 0-based indexes into `old_lines` and `new_lines`.
/// A side is `None` where the other side has a line with no counterpart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub old: Option<usize>,
    pub new: Option<usize>,
}

/// A changed piece of text on one line (columns are byte offsets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    pub line: u32,
    pub start_col: u32,
    pub end_col: u32,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct FileDiff {
    pub kind: FileKind,
    /// Detected language name, if any.
    pub language: Option<String>,
    /// Set when the diff is line-based instead of structural, and why.
    pub fallback: Option<FallbackReason>,
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
    pub rows: Vec<Row>,
    old_tokens: Vec<Token>,
    new_tokens: Vec<Token>,
}

impl FileDiff {
    /// Every token of the side, changed or not, for highlighting.
    pub fn tokens(&self, side: Side) -> &[Token] {
        match side {
            Side::Old => &self.old_tokens,
            Side::New => &self.new_tokens,
        }
    }

    /// The changed pieces of text on the side.
    pub fn changed_fragments(&self, side: Side) -> impl Iterator<Item = Fragment> + '_ {
        let lines = match side {
            Side::Old => &self.old_lines,
            Side::New => &self.new_lines,
        };
        self.tokens(side)
            .iter()
            .filter(|t| t.changed)
            .map(move |t| {
                let line = &lines[t.line as usize];
                let (s, e) = (t.start_col as usize, (t.end_col as usize).min(line.len()));
                Fragment {
                    line: t.line,
                    start_col: t.start_col,
                    end_col: t.end_col,
                    text: line.get(s..e).unwrap_or_default().to_owned(),
                }
            })
    }
}

fn split_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_owned())
        .collect()
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|&b| b == 0) || std::str::from_utf8(bytes).is_err()
}

/// Diff two versions of the file at `path`. `None` means the file does not
/// exist on that side (created or deleted). `path` selects the language.
pub fn diff_file(path: &str, old: Option<&[u8]>, new: Option<&[u8]>, limits: Limits) -> FileDiff {
    let kind = match (old, new) {
        (None, _) => FileKind::Created,
        (_, None) => FileKind::Deleted,
        _ => FileKind::Modified,
    };

    let binary = old.is_some_and(is_binary) || new.is_some_and(is_binary);
    if binary {
        return FileDiff {
            kind: FileKind::Binary,
            language: None,
            fallback: None,
            old_lines: vec![],
            new_lines: vec![],
            rows: vec![],
            old_tokens: vec![],
            new_tokens: vec![],
        };
    }

    // Both checked as valid UTF-8 above.
    let old_src = old
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();
    let new_src = new
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();

    let result = difftastic::diff(path, &old_src, &new_src, limits);
    let old_lines = split_lines(&old_src);
    let new_lines = split_lines(&new_src);

    let rows = result
        .aligned
        .iter()
        .map(|&(o, n)| Row {
            old: o.map(|i| i as usize).filter(|&i| i < old_lines.len()),
            new: n.map(|i| i as usize).filter(|&i| i < new_lines.len()),
        })
        .filter(|r| r.old.is_some() || r.new.is_some())
        .collect();

    let in_bounds = |lines: &[String], tokens: Vec<Token>| -> Vec<Token> {
        tokens
            .into_iter()
            .filter(|t| (t.line as usize) < lines.len())
            .collect()
    };

    FileDiff {
        kind,
        language: result.language,
        fallback: result.fallback,
        rows,
        old_tokens: in_bounds(&old_lines, result.lhs_tokens),
        new_tokens: in_bounds(&new_lines, result.rhs_tokens),
        old_lines,
        new_lines,
    }
}

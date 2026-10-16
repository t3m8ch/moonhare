//! In-process entry point to the vendored difftastic engine.
//!
//! This file is not part of upstream difftastic (which ships only a binary).
//! It adapts `diff_file_content` from upstream `src/main.rs` into a function
//! that takes two strings and returns owned, public data. See ADR 0001.

#![allow(dead_code)]
#![allow(renamed_and_removed_lints)]
#![allow(clippy::all)]
#![allow(unknown_lints)]

#[macro_use]
extern crate log;

mod conflicts;
mod constants;
mod diff;
mod display;
mod exit_codes;
mod files;
mod hash;
mod line_parser;
mod lines;
mod options;
mod parse;
mod summary;
mod version;
mod words;

use std::path::Path;

use typed_arena::Arena;

use crate::diff::changes::ChangeMap;
use crate::diff::shortest_path::{mark_syntax, ExceededGraphLimit};
use crate::diff::sliders::fix_all_sliders;
use crate::diff::unchanged;
use crate::display::context::all_matched_lines_filled;
use crate::lines::split_on_newlines;
use crate::parse::guess_language::{guess, language_name};
use crate::parse::syntax::{
    self, init_next_prev, AtomKind, MatchKind, MatchedPos, StringKind, TokenKind,
};
use crate::parse::tree_sitter_parser as tsp;

/// Resource limits for one diff.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Largest accepted size, in bytes, of either side.
    pub byte_limit: usize,
    /// Largest number of nodes the structural diff may explore.
    pub graph_limit: usize,
    /// Number of tolerated parse errors before falling back.
    pub parse_error_limit: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            byte_limit: 1_000_000,
            graph_limit: 3_000_000,
            parse_error_limit: 0,
        }
    }
}

/// Why the engine produced a line-based diff instead of a structural one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fallback {
    UnknownLanguage,
    ByteLimit { bytes: usize },
    GraphLimit,
    ParseErrors { language: String, errors: usize },
}

/// Syntax category of a token, for highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Highlight {
    Delimiter,
    Normal,
    String,
    Text,
    Type,
    Keyword,
    Comment,
    Error,
    Ignorable,
}

/// A token on one side. Lines are 0-based, columns are byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub line: u32,
    pub start_col: u32,
    pub end_col: u32,
    pub highlight: Highlight,
    /// True when the token is part of a change (novel on this side).
    pub changed: bool,
}

#[derive(Debug, Clone)]
pub struct EngineResult {
    /// Name of the detected language, if any.
    pub language: Option<String>,
    pub fallback: Option<Fallback>,
    /// Pairs of 0-based line numbers shown on the same row.
    pub aligned: Vec<(Option<u32>, Option<u32>)>,
    pub lhs_tokens: Vec<Token>,
    pub rhs_tokens: Vec<Token>,
}

fn highlight_of(kind: TokenKind) -> Highlight {
    match kind {
        TokenKind::Delimiter => Highlight::Delimiter,
        TokenKind::Atom(AtomKind::Normal) => Highlight::Normal,
        TokenKind::Atom(AtomKind::String(StringKind::StringLiteral)) => Highlight::String,
        TokenKind::Atom(AtomKind::String(StringKind::Text)) => Highlight::Text,
        TokenKind::Atom(AtomKind::Type) => Highlight::Type,
        TokenKind::Atom(AtomKind::Keyword) => Highlight::Keyword,
        TokenKind::Atom(AtomKind::Comment) => Highlight::Comment,
        TokenKind::Atom(AtomKind::TreeSitterError) => Highlight::Error,
        TokenKind::Atom(AtomKind::CanIgnore) => Highlight::Ignorable,
    }
}

fn token_of(mp: &MatchedPos) -> Token {
    let (MatchKind::UnchangedToken { highlight, .. }
    | MatchKind::Novel { highlight }
    | MatchKind::UnchangedPartOfNovelItem { highlight, .. }
    | MatchKind::NovelWord { highlight }
    | MatchKind::Ignored { highlight }) = mp.kind;
    Token {
        line: mp.pos.line.0,
        start_col: mp.pos.start_col,
        end_col: mp.pos.end_col,
        highlight: highlight_of(highlight),
        changed: mp.kind.is_novel() && !matches!(mp.kind, MatchKind::UnchangedPartOfNovelItem { .. }),
    }
}

/// Diff two versions of a file. `path` is used only to detect the language.
pub fn diff(path: &str, lhs_src: &str, rhs_src: &str, limits: Limits) -> EngineResult {
    let options = options::DiffOptions {
        graph_limit: limits.graph_limit,
        byte_limit: limits.byte_limit,
        parse_error_limit: limits.parse_error_limit,
        ..Default::default()
    };

    let guess_src = if rhs_src.is_empty() { lhs_src } else { rhs_src };
    let language = guess(Path::new(path), guess_src, &[]);

    let line_based = || line_parser::change_positions(lhs_src, rhs_src);

    let (fallback, lhs_mps, rhs_mps) = match language {
        None => {
            let (l, r) = line_based();
            (Some(Fallback::UnknownLanguage), l, r)
        }
        Some(lang) => {
            let config = tsp::from_language(lang);
            let arena = Arena::new();
            match tsp::to_tree_with_limit(&options, config, lhs_src, rhs_src) {
                Err(tsp::ExceededByteLimit(bytes)) => {
                    let (l, r) = line_based();
                    (Some(Fallback::ByteLimit { bytes }), l, r)
                }
                Ok((lhs_tree, rhs_tree)) => {
                    match tsp::to_syntax_with_limit(
                        lhs_src, rhs_src, &lhs_tree, &rhs_tree, &arena, config, &options,
                    ) {
                        Err(tsp::ExceededParseErrorLimit { error_count, .. }) => {
                            let (l, r) = line_based();
                            (
                                Some(Fallback::ParseErrors {
                                    language: language_name(lang).to_owned(),
                                    errors: error_count,
                                }),
                                l,
                                r,
                            )
                        }
                        Ok((lhs, rhs)) => {
                            let mut change_map = ChangeMap::default();
                            let possibly_changed =
                                unchanged::mark_unchanged(&lhs, &rhs, &mut change_map);
                            let mut exceeded = false;
                            for (lhs_nodes, rhs_nodes) in possibly_changed {
                                init_next_prev(&lhs_nodes);
                                init_next_prev(&rhs_nodes);
                                if let Err(ExceededGraphLimit {}) = mark_syntax(
                                    lhs_nodes.first().copied(),
                                    rhs_nodes.first().copied(),
                                    &mut change_map,
                                    options.graph_limit,
                                ) {
                                    exceeded = true;
                                    break;
                                }
                            }
                            if exceeded {
                                let (l, r) = line_based();
                                (Some(Fallback::GraphLimit), l, r)
                            } else {
                                fix_all_sliders(lang, &lhs, &mut change_map);
                                fix_all_sliders(lang, &rhs, &mut change_map);
                                (
                                    None,
                                    syntax::change_positions(&lhs, &change_map),
                                    syntax::change_positions(&rhs, &change_map),
                                )
                            }
                        }
                    }
                }
            }
        }
    };

    let mut lhs_lines: Vec<&str> = split_on_newlines(lhs_src).collect();
    let mut rhs_lines: Vec<&str> = split_on_newlines(rhs_src).collect();
    if lhs_lines.last() == Some(&"") && lhs_lines.len() > 1 {
        lhs_lines.pop();
    }
    if rhs_lines.last() == Some(&"") && rhs_lines.len() > 1 {
        rhs_lines.pop();
    }
    let aligned = all_matched_lines_filled(&lhs_mps, &rhs_mps, &lhs_lines, &rhs_lines)
        .into_iter()
        .map(|(l, r)| (l.map(|n| n.0), r.map(|n| n.0)))
        .collect();

    EngineResult {
        language: language.map(|l| language_name(l).to_owned()),
        fallback,
        aligned,
        lhs_tokens: lhs_mps.iter().map(token_of).collect(),
        rhs_tokens: rhs_mps.iter().map(token_of).collect(),
    }
}

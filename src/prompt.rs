//! The Prompt: Comments assembled into the text handed to the Author agent.

use crate::comments::{Comment, Target};
use crate::diff::Side;

/// What the preamble says about the Prompt and the Range it is about.
pub struct Preamble<'a> {
    pub number: u32,
    pub branch: &'a str,
    pub base: &'a str,
    pub head: &'a str,
    pub commits: usize,
}

fn short(sha: &str) -> &str {
    &sha[..7.min(sha.len())]
}

/// A fence of backticks longer than any run inside `code`.
fn fence(code: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in code.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    "`".repeat((longest + 1).max(3))
}

/// The Prompt text: an English preamble, then general Comments, then the ones
/// on files, then the ones on lines, each with its path, lines and code.
pub fn build(pre: &Preamble, comments: &[&Comment]) -> String {
    let mut out = format!(
        "You are the author of branch `{}` (commits {}..{}, {} commits).\n\
         Below are review comments from the reviewer (Prompt #{}). \
         Answer every question and make the requested changes.\n",
        pre.branch,
        short(pre.base),
        short(pre.head),
        pre.commits,
        pre.number,
    );
    let general: Vec<_> = comments
        .iter()
        .filter(|c| c.target == Target::Range)
        .collect();
    if !general.is_empty() {
        out.push_str("\n## General comments\n");
        for (n, c) in general.iter().enumerate() {
            out.push_str(&format!("\n{}. {}\n", n + 1, c.text));
        }
    }
    let files: Vec<_> = comments
        .iter()
        .filter_map(|c| match &c.target {
            Target::File { path } => Some((path, c)),
            _ => None,
        })
        .collect();
    if !files.is_empty() {
        out.push_str("\n## File comments\n");
        for (n, (path, c)) in files.iter().enumerate() {
            out.push_str(&format!("\n{}. {path}\n{}\n", n + 1, c.text));
        }
    }
    let mut lines: Vec<_> = comments
        .iter()
        .filter_map(|c| match &c.target {
            Target::Lines {
                path,
                side,
                start,
                end,
                anchor,
            } => Some((path, *side, *start, *end, anchor, c)),
            _ => None,
        })
        .collect();
    lines.sort_by(|a, b| (a.0, a.1 == Side::Old, a.2).cmp(&(b.0, b.1 == Side::Old, b.2)));
    if !lines.is_empty() {
        out.push_str("\n## Line comments\n");
        for (n, (path, side, start, end, anchor, c)) in lines.iter().enumerate() {
            let span = if start == end {
                (start + 1).to_string()
            } else {
                format!("{}-{}", start + 1, end + 1)
            };
            let version = match side {
                Side::Old => " (old version)",
                Side::New => "",
            };
            let code = anchor.snapshot.join("\n");
            let fence = fence(&code);
            out.push_str(&format!(
                "\n{}. {path}:{span}{version}\n{fence}\n{code}\n{fence}\n{}\n",
                n + 1,
                c.text
            ));
        }
    }
    out
}

//! Drawing Mermaid text as box-drawing characters. `mmdflux` does the work;
//! nothing here uses images or graphics protocols of the terminal.
//!
//! The text comes from an LLM, so it is untrusted: it is refused when long,
//! rendered on a thread of its own that is given up on after a while, and a
//! crash of the renderer costs the picture, not the session.
//!
//! A diagram is fitted to the room it is shown in ([`draw_in`]): `mmdflux`
//! decides the width from the graph alone, so several layouts are tried, from
//! the diagram as written to denser ones ([`candidates`]), and the first that
//! fits is shown. The layouts are made by rewriting the Mermaid text that is
//! drawn (a flowchart turned top-down, long labels wrapped, long messages of a
//! sequence diagram shortened) and by closing up the gaps of the picture; the
//! Mermaid source the Reviewer copies is never changed.

use std::sync::mpsc::channel;
use std::sync::Once;
use std::time::Duration;

use mmdflux::{render_diagram, OutputFormat, RenderConfig};
use unicode_width::UnicodeWidthChar;

/// Longest Mermaid source that is drawn, in bytes.
pub const MAX_SOURCE: usize = 20_000;

/// How long the renderer may take before it is given up on.
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(5);

const RENDER_THREAD: &str = "moonhare-render";

/// The cells a picture is shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Room {
    pub width: usize,
    pub height: usize,
}

/// How densely a diagram is laid out: the zoom of a terminal, whose cells
/// cannot shrink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Density {
    /// The whole diagram in Braille dots, without text: see [`overview`].
    Overview,
    /// Gaps closed up and labels wrapped short, whatever the room.
    Compact,
    /// The first layout that fits the room.
    #[default]
    Fit,
    /// The diagram as written, labels whole, gaps widened.
    Roomy,
}

impl Density {
    /// One step denser (`-`), if there is one.
    pub fn denser(self) -> Option<Density> {
        match self {
            Density::Overview => None,
            Density::Compact => Some(Density::Overview),
            Density::Fit => Some(Density::Compact),
            Density::Roomy => Some(Density::Fit),
        }
    }

    /// One step roomier (`+`), if there is one.
    pub fn roomier(self) -> Option<Density> {
        match self {
            Density::Overview => Some(Density::Compact),
            Density::Compact => Some(Density::Fit),
            Density::Fit => Some(Density::Roomy),
            Density::Roomy => None,
        }
    }
}

/// What is done to the gaps of a drawn picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gaps {
    AsDrawn,
    /// Runs of blank or line-only columns and rows cut down to this many.
    Squeeze(usize),
    /// Those runs made twice as long.
    Stretch,
}

/// One layout of a diagram: the Mermaid text that is drawn and what is done
/// to the picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub source: String,
    /// Labels of edges are not wrapped at all (`mmdflux` wraps long ones).
    pub whole_labels: bool,
    pub gaps: Gaps,
}

impl Candidate {
    fn new(source: &str, gaps: Gaps) -> Candidate {
        Candidate {
            source: source.to_string(),
            whole_labels: false,
            gaps,
        }
    }
}

/// The diagram as lines of text, as `mmdflux` draws it, or why it could not
/// be drawn.
pub fn draw(mermaid: &str) -> Result<Vec<String>, String> {
    draw_with(mermaid, |text| {
        render_diagram(text, OutputFormat::Text, &RenderConfig::default())
            .map_err(|e| e.to_string())
    })
}

/// [`draw`] with another renderer.
pub fn draw_with(
    mermaid: &str,
    render: impl FnOnce(&str) -> Result<String, String> + Send + 'static,
) -> Result<Vec<String>, String> {
    sandboxed(mermaid, move |text| {
        render(&text).map(|text| text.lines().map(|l| sanitize(l.trim_end())).collect())
    })
}

/// The diagram laid out for `room` at `density` (see [`Density`]): the
/// first of the [`candidates`] that fits the width and the height, else the
/// shortest that fits the width, else the narrowest; [`Density::Compact`]
/// takes the narrowest of its own. [`Density::Overview`] is
/// drawn as [`Density::Fit`]; [`overview`] makes dots of that.
pub fn draw_in(mermaid: &str, room: Room, density: Density) -> Result<Vec<String>, String> {
    sandboxed(mermaid, move |text| fit(&text, room, density))
}

/// The diagram as SVG, for a viewer outside the terminal.
pub fn svg(mermaid: &str) -> Result<String, String> {
    sandboxed(mermaid, |text| {
        render_diagram(&text, OutputFormat::Svg, &RenderConfig::default())
            .map_err(|e| sanitize(&e.to_string()))
    })
}

/// Run `work` on the Mermaid text on a thread of its own, given up on after
/// [`RENDER_TIMEOUT`]; a text over [`MAX_SOURCE`] is refused.
fn sandboxed<T: Send + 'static>(
    mermaid: &str,
    work: impl FnOnce(String) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    if mermaid.len() > MAX_SOURCE {
        return Err(format!(
            "the source is too long to draw ({} bytes, the limit is {MAX_SOURCE})",
            mermaid.len()
        ));
    }
    silence_renderer_panics();
    let text = mermaid.to_string();
    let (sender, receiver) = channel();
    std::thread::Builder::new()
        .name(RENDER_THREAD.into())
        .spawn(move || {
            let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(text)));
            let _ = sender.send(done);
        })
        .map_err(|e| format!("cannot start the renderer: {e}"))?;
    match receiver.recv_timeout(RENDER_TIMEOUT) {
        Ok(Ok(Ok(done))) => Ok(done),
        Ok(Ok(Err(e))) => Err(sanitize(&e)),
        Ok(Err(_)) => Err("the renderer crashed on this diagram".into()),
        Err(_) => Err("the diagram took too long to draw".into()),
    }
}

fn fit(mermaid: &str, room: Room, density: Density) -> Result<Vec<String>, String> {
    let mut error = None;
    let mut fits_width = None;
    let mut narrowest: Option<(usize, usize, Vec<String>)> = None;
    for candidate in candidates(mermaid, density) {
        let lines = match render(&candidate) {
            Ok(lines) => lines,
            Err(e) => {
                error.get_or_insert(e);
                continue;
            }
        };
        let (width, height) = (width_of(&lines), lines.len());
        if density != Density::Compact && width <= room.width {
            if height <= room.height {
                return Ok(lines);
            }
            if fits_width
                .as_ref()
                .is_none_or(|shortest: &Vec<String>| height < shortest.len())
            {
                fits_width = Some(lines.clone());
            }
        }
        if narrowest
            .as_ref()
            .is_none_or(|(w, h, _)| (width, height) < (*w, *h))
        {
            narrowest = Some((width, height, lines));
        }
    }
    fits_width
        .or(narrowest.map(|(.., lines)| lines))
        .ok_or_else(|| error.unwrap_or_else(|| "nothing to draw".into()))
}

/// The layouts tried for `mermaid` at `density`, the diagram as written first.
pub fn candidates(mermaid: &str, density: Density) -> Vec<Candidate> {
    if density == Density::Roomy {
        return vec![Candidate {
            source: mermaid.to_string(),
            whole_labels: true,
            gaps: Gaps::Stretch,
        }];
    }
    let compact = density == Density::Compact;
    let mut list = Vec::new();
    match mmdflux::detect_diagram(mermaid) {
        Some("flowchart") => {
            let turned = turned_top_down(mermaid);
            let upright = turned.as_deref().unwrap_or(mermaid);
            if !compact {
                list.push(Candidate::new(mermaid, Gaps::AsDrawn));
                list.push(Candidate::new(mermaid, Gaps::Squeeze(2)));
                if let Some(turned) = &turned {
                    list.push(Candidate::new(turned, Gaps::AsDrawn));
                    list.push(Candidate::new(turned, Gaps::Squeeze(2)));
                }
                list.push(Candidate::new(&wrap_labels(upright, 24), Gaps::Squeeze(2)));
            }
            for width in [16, 10] {
                list.push(Candidate::new(
                    &wrap_labels(upright, width),
                    Gaps::Squeeze(1),
                ));
            }
        }
        Some("sequence") => {
            if !compact {
                list.push(Candidate::new(mermaid, Gaps::AsDrawn));
                list.push(Candidate::new(mermaid, Gaps::Squeeze(2)));
                list.push(Candidate::new(
                    &shorten_messages(mermaid, 32),
                    Gaps::Squeeze(1),
                ));
            }
            for width in [20, 12] {
                list.push(Candidate::new(
                    &shorten_messages(mermaid, width),
                    Gaps::Squeeze(1),
                ));
            }
        }
        _ => {
            if !compact {
                list.push(Candidate::new(mermaid, Gaps::AsDrawn));
                list.push(Candidate::new(mermaid, Gaps::Squeeze(2)));
            }
            list.push(Candidate::new(mermaid, Gaps::Squeeze(1)));
        }
    }
    list.dedup();
    list
}

/// Draw one layout, here and now: no thread, no time limit.
pub fn render(candidate: &Candidate) -> Result<Vec<String>, String> {
    let mut config = RenderConfig::default();
    if candidate.whole_labels {
        config.layout.edge_label_max_width = None;
    }
    let text = render_diagram(&candidate.source, OutputFormat::Text, &config)
        .map_err(|e| e.to_string())?;
    let lines: Vec<String> = text.lines().map(|l| sanitize(l.trim_end())).collect();
    Ok(match candidate.gaps {
        Gaps::AsDrawn => lines,
        Gaps::Squeeze(keep) => regap(&lines, Some(keep.max(1))),
        Gaps::Stretch => regap(&lines, None),
    })
}

fn width_of(lines: &[String]) -> usize {
    lines
        .iter()
        .map(|l| unicode_width::UnicodeWidthStr::width(l.as_str()))
        .max()
        .unwrap_or(0)
}

/// The first line of the Mermaid text that is not blank, a comment or front
/// matter: where the kind and the direction of the diagram are.
fn header_index(lines: &[&str]) -> Option<usize> {
    let mut in_front_matter = false;
    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line == "---" {
            in_front_matter = !in_front_matter;
            continue;
        }
        if in_front_matter || line.is_empty() || line.starts_with("%%") {
            continue;
        }
        return Some(i);
    }
    None
}

/// A left-to-right (or right-to-left) flowchart drawn top-down; `None` when
/// it already is.
fn turned_top_down(mermaid: &str) -> Option<String> {
    let mut lines: Vec<&str> = mermaid.lines().collect();
    let at = header_index(&lines)?;
    let header = lines[at];
    let mut words = header.split_whitespace();
    let kind = words.next()?;
    let direction = words.next()?;
    let direction = direction.trim_end_matches(';');
    if !matches!(kind, "graph" | "flowchart") || !matches!(direction, "LR" | "RL") {
        return None;
    }
    let indent = &header[..header.len() - header.trim_start().len()];
    let rest: Vec<&str> = words.collect();
    let turned = format!("{indent}{kind} TD {}", rest.join(" "));
    lines[at] = turned.trim_end();
    let mut text = lines.join("\n");
    if mermaid.ends_with('\n') {
        text.push('\n');
    }
    Some(text)
}

/// Statements of a flowchart that carry no labels to wrap.
const NO_LABELS: &[&str] = &[
    "%%",
    "classDef",
    "class ",
    "style",
    "linkStyle",
    "click",
    "direction",
    "subgraph",
    "end",
];

/// The flowchart with the labels of its nodes (`[…]`, `(…)`, `{…}`) and edges
/// (`|…|`) longer than `width` broken into lines with `<br/>`.
fn wrap_labels(mermaid: &str, width: usize) -> String {
    let lines: Vec<&str> = mermaid.lines().collect();
    let header = header_index(&lines);
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let statement = line.trim_start();
            if Some(i) <= header || NO_LABELS.iter().any(|w| statement.starts_with(w)) {
                line.to_string()
            } else {
                wrap_line(line, width)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn wrap_line(line: &str, width: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        out.push(c);
        i += 1;
        let close: &[char] = match c {
            '[' | '(' | '{' => &[']', ')', '}'],
            '|' => &['|'],
            _ => continue,
        };
        // The rest of a run of openers, as in `((`, `([`, `{{`.
        if c != '|' {
            while i < chars.len() && matches!(chars[i], '[' | '(' | '{') {
                out.push(chars[i]);
                i += 1;
            }
        }
        let (quote, stop): (bool, &[char]) = if chars.get(i) == Some(&'"') {
            out.push('"');
            i += 1;
            (true, &['"'])
        } else {
            (false, close)
        };
        let Some(len) = chars[i..].iter().position(|ch| stop.contains(ch)) else {
            out.extend(&chars[i..]);
            break;
        };
        let label: String = chars[i..i + len].iter().collect();
        out.push_str(&wrap_words(&label, width));
        i += len;
        if quote {
            out.push('"');
            i += 1;
        } else if c == '|' {
            out.push('|');
            i += 1;
        }
    }
    out
}

/// `label` broken with `<br/>` into lines of at most `width` characters, at
/// spaces; one that already has markup is left alone.
fn wrap_words(label: &str, width: usize) -> String {
    if label.chars().count() <= width || label.contains('<') {
        return label.to_string();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in label.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    lines.push(line);
    lines.join("<br/>")
}

/// Statements of a sequence diagram whose text after `:` is not a message.
const NOT_MESSAGES: &[&str] = &[
    "%%",
    "participant",
    "actor",
    "autonumber",
    "title",
    "box",
    "create",
    "destroy",
    "activate",
    "deactivate",
];

/// The sequence diagram with its messages, notes and participant names
/// longer than `width` cut short with `…`: `mmdflux` draws a message on one
/// line, and the longest one sets the gap between all participants.
fn shorten_messages(mermaid: &str, width: usize) -> String {
    let lines: Vec<&str> = mermaid.lines().collect();
    let header = header_index(&lines);
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let statement = line.trim_start();
            if Some(i) <= header {
                return line.to_string();
            }
            if statement.starts_with("participant") || statement.starts_with("actor") {
                return match line.split_once(" as ") {
                    Some((head, name)) => format!("{head} as {}", cut(name.trim(), width)),
                    None => line.to_string(),
                };
            }
            if NOT_MESSAGES.iter().any(|w| statement.starts_with(w)) {
                return line.to_string();
            }
            match line.split_once(':') {
                Some((head, text)) => format!("{head}: {}", cut(text.trim(), width)),
                None => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn cut(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut short: String = text.chars().take(width.saturating_sub(1)).collect();
    short.truncate(short.trim_end().len());
    short.push('…');
    short
}

/// Characters a column may hold and still be a gap: blank or a line across.
const ACROSS: &[char] = &[' ', '─', '━', '┄', '┅', '┈', '┉', '╌', '╍', '═'];

/// Characters a row may hold and still be a gap: blank or a line down.
const DOWN: &[char] = &[' ', '│', '┃', '┆', '┇', '┊', '┋', '╎', '╏', '║'];

/// The picture with every run of gap columns and rows (each the same as the
/// one before it, holding nothing but blanks and lines) cut down to `keep`
/// (counting the one the run repeats), or doubled when `keep` is `None`.
/// Nothing but the length of lines and gaps changes. A picture with a
/// character wider or narrower than a cell is left as it is.
fn regap(lines: &[String], keep: Option<usize>) -> Vec<String> {
    let grid: Vec<Vec<char>> = lines.iter().map(|l| l.chars().collect()).collect();
    if grid.iter().flatten().any(|c| c.width() != Some(1)) {
        return lines.to_vec();
    }
    let width = grid.iter().map(Vec::len).max().unwrap_or(0);
    let cell = |row: &Vec<char>, x: usize| row.get(x).copied().unwrap_or(' ');
    let column_repeats = |x: usize| {
        x > 0
            && grid
                .iter()
                .all(|row| ACROSS.contains(&cell(row, x)) && cell(row, x) == cell(row, x - 1))
    };
    let columns = copies(width, keep, column_repeats);
    let row_repeats = |y: usize| {
        y > 0
            && (0..width).all(|x| {
                DOWN.contains(&cell(&grid[y], x)) && cell(&grid[y], x) == cell(&grid[y - 1], x)
            })
    };
    let rows = copies(grid.len(), keep, row_repeats);
    let mut out = Vec::new();
    for (y, row) in grid.iter().enumerate() {
        let line: String = (0..width)
            .flat_map(|x| std::iter::repeat_n(cell(row, x), columns[x]))
            .collect();
        let line = line.trim_end().to_string();
        out.extend(std::iter::repeat_n(line, rows[y]));
    }
    out
}

/// How many times each of `len` columns (or rows) is kept: one that
/// `repeats` the one before it is dropped past `keep` in a run, or doubled.
fn copies(len: usize, keep: Option<usize>, repeats: impl Fn(usize) -> bool) -> Vec<usize> {
    let mut run = 1;
    (0..len)
        .map(|i| {
            if !repeats(i) {
                run = 1;
                return 1;
            }
            run += 1;
            match keep {
                Some(keep) => usize::from(run <= keep),
                None => 2,
            }
        })
        .collect()
}

/// Keep the panic message of the renderer's thread off the terminal that the
/// TUI owns; other threads panic as before.
fn silence_renderer_panics() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if std::thread::current().name() != Some(RENDER_THREAD) {
                previous(info);
            }
        }));
    });
}

/// `text` with control characters (an escape sequence in it would reach the
/// terminal) replaced by a visible placeholder. Tabs become spaces.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\t' => ' ',
            c if c.is_control() => '\u{fffd}',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn squeezing_cuts_runs_of_gap_columns_and_keeps_everything_else() {
        let picture = strings(&[
            "┌───┐        ┌───┐",
            "│ A │───────►│ B │",
            "└───┘        └───┘",
        ]);
        assert_eq!(
            regap(&picture, Some(1)),
            strings(&["┌───┐  ┌───┐", "│ A │─►│ B │", "└───┘  └───┘"])
        );
        assert_eq!(
            regap(&picture, Some(2)),
            strings(&["┌───┐   ┌───┐", "│ A │──►│ B │", "└───┘   └───┘"])
        );
    }

    #[test]
    fn rows_of_lines_down_are_squeezed_and_stretched() {
        let picture = strings(&[
            "┌───┐",
            "│ A │",
            "└─┬─┘",
            "  │",
            "  │",
            "  │",
            "  ▼",
            "┌───┐",
        ]);
        assert_eq!(
            regap(&picture, Some(1)),
            strings(&["┌───┐", "│ A │", "└─┬─┘", "  │", "  ▼", "┌───┐"])
        );
        assert_eq!(regap(&picture, None).len(), picture.len() + 2);
    }

    #[test]
    fn a_picture_with_wide_characters_is_left_alone() {
        let picture = strings(&["│ 図 │      │", "│ 図 │      │"]);
        assert_eq!(regap(&picture, Some(1)), picture);
    }

    #[test]
    fn a_left_to_right_flowchart_is_turned_top_down() {
        assert_eq!(
            turned_top_down("%% c\nflowchart LR\n  A --> B").as_deref(),
            Some("%% c\nflowchart TD\n  A --> B")
        );
        assert_eq!(turned_top_down("graph TD\n  A --> B"), None);
        assert_eq!(turned_top_down("sequenceDiagram\n  A->>B: x"), None);
    }

    #[test]
    fn labels_of_nodes_and_edges_are_wrapped_and_nothing_else() {
        let wrapped = wrap_labels(
            "graph TD\n  A[one two three four] -->|five six seven| B((\"eight nine ten\"))\n  classDef long fill:#f00,stroke:#0f0",
            9,
        );
        assert_eq!(
            wrapped,
            "graph TD\n  A[one two<br/>three<br/>four] -->|five six<br/>seven| B((\"eight<br/>nine ten\"))\n  classDef long fill:#f00,stroke:#0f0"
        );
    }

    #[test]
    fn long_messages_and_names_of_a_sequence_diagram_are_cut_short() {
        assert_eq!(
            shorten_messages(
                "sequenceDiagram\n  participant A as A rather long name\n  A->>B: a message far too long\n  Note over A: short",
                10
            ),
            "sequenceDiagram\n  participant A as A rather…\n  A->>B: a message…\n  Note over A: short"
        );
    }
}

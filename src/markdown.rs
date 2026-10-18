//! Markdown from the Analyst drawn as lines of a terminal: the Summary, an
//! Explanation and the description of a Group are written in it.
//!
//! `pulldown-cmark` parses (with GFM tables); this module lays the blocks out
//! for a width: it wraps, so a scrolling overlay gets finished lines. Text
//! from the Analyst is untrusted: control characters and text-direction
//! marks never reach the terminal.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use crate::theme::{Theme, DARK};

/// Spaces a tab of a code block takes.
const TAB: &str = "    ";

/// The narrowest a table column is squeezed to before the table turns into
/// cards; a column whose content is narrower stays as it is.
const MIN_COLUMN: usize = 3;
/// A column is not squeezed below its longest word, up to this width.
const MAX_WORD_COLUMN: usize = 10;

/// Draws a fenced block in place of the code: the Summary draws its Mermaid
/// blocks as diagrams. Gets the language and the code.
pub type Pictures<'a> = &'a dyn Fn(&str, &str) -> Option<Vec<String>>;

/// `source` drawn in the colours of `theme` for a box `width` cells wide.
pub fn render(theme: &Theme, source: &str, width: usize) -> Vec<Line<'static>> {
    render_with(theme, source, width, &|_, _| None)
}

/// [`render`], with fenced blocks drawn by `pictures` when it can.
pub fn render_with(
    theme: &Theme,
    source: &str,
    width: usize,
    pictures: Pictures,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    layout(theme, &parse(theme, source), width, false, pictures)
        .into_iter()
        .map(|line| truncate(line, width))
        .collect()
}

/// The code of the fenced blocks in `language`. (Only the text is wanted:
/// any theme parses it.)
pub fn fenced_blocks(source: &str, language: &str) -> Vec<String> {
    let mut found = Vec::new();
    collect_fenced(&parse(&DARK, source), language, &mut found);
    found
}

/// `source` as one line of plain text, for a hint: the words without the
/// marks.
pub fn one_line(source: &str) -> String {
    let mut words = Vec::new();
    flatten(&parse(&DARK, source), &mut words);
    words.join(" ")
}

/// The text of the lines as the screen shows it.
pub fn plain(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect()
}

// ------------------------------------------------------------------ parsing

/// Runs of styled text; a `\n` in it is a hard line break.
type Inline = Vec<(String, Style)>;

enum Block {
    Text(Inline),
    Heading(Inline),
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Code {
        language: String,
        code: String,
    },
    Quote(Vec<Block>),
    Rule,
    Table {
        aligns: Vec<Alignment>,
        head: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
    },
}

enum Frame {
    Blocks(Vec<Block>),
    Quote(Vec<Block>),
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Item(Vec<Block>),
    Code {
        language: String,
        code: String,
    },
    Table {
        aligns: Vec<Alignment>,
        head: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
        row: Vec<Inline>,
    },
}

struct Builder<'t> {
    theme: &'t Theme,
    stack: Vec<Frame>,
    /// The text being collected: a paragraph, a heading, a cell.
    inline: Option<Inline>,
    styles: Vec<Style>,
    links: Vec<String>,
}

fn parse(theme: &Theme, source: &str) -> Vec<Block> {
    let mut b = Builder {
        theme,
        stack: vec![Frame::Blocks(Vec::new())],
        inline: None,
        styles: Vec::new(),
        links: Vec::new(),
    };
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for event in Parser::new_ext(source, options) {
        b.event(event);
    }
    b.flush();
    while b.stack.len() > 1 {
        b.close();
    }
    match b.stack.pop() {
        Some(Frame::Blocks(blocks)) => blocks,
        _ => Vec::new(),
    }
}

impl Builder<'_> {
    fn style(&self) -> Style {
        self.styles
            .iter()
            .fold(Style::new(), |style, s| style.patch(*s))
    }

    fn push_text(&mut self, text: &str, style: Style) {
        if let Some(Frame::Code { code, .. }) = self.stack.last_mut() {
            code.push_str(text);
            return;
        }
        self.inline
            .get_or_insert_with(Vec::new)
            .push((clean(text), style));
    }

    /// The text collected so far becomes a block of its own: a tight list
    /// item has text without a paragraph around it.
    fn flush(&mut self) {
        if let Some(inline) = self.inline.take() {
            if !inline.is_empty() {
                self.push(Block::Text(inline));
            }
        }
    }

    fn push(&mut self, block: Block) {
        match self.stack.last_mut() {
            Some(Frame::Blocks(blocks) | Frame::Quote(blocks) | Frame::Item(blocks)) => {
                blocks.push(block)
            }
            Some(Frame::List { items, .. }) => items.push(vec![block]),
            _ => {}
        }
    }

    /// Close the innermost frame into the block it makes.
    fn close(&mut self) {
        let block = match self.stack.pop() {
            Some(Frame::Quote(blocks)) => Block::Quote(blocks),
            Some(Frame::List { start, items }) => Block::List { start, items },
            Some(Frame::Item(blocks)) => {
                if let Some(Frame::List { items, .. }) = self.stack.last_mut() {
                    items.push(blocks);
                    return;
                }
                Block::Quote(blocks)
            }
            Some(Frame::Code { language, code }) => Block::Code { language, code },
            Some(Frame::Table {
                aligns, head, rows, ..
            }) => Block::Table { aligns, head, rows },
            Some(frame @ Frame::Blocks(_)) => {
                self.stack.push(frame);
                return;
            }
            None => return,
        };
        self.push(block);
    }

    fn in_table(&self) -> bool {
        matches!(self.stack.last(), Some(Frame::Table { .. }))
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) | Event::InlineHtml(text) | Event::InlineMath(text) => {
                let style = self.style();
                self.push_text(&text, style)
            }
            Event::Html(text) => {
                let style = self.style();
                self.push_text(text.trim_end_matches('\n'), style)
            }
            Event::DisplayMath(text) => {
                let style = self.style();
                self.push_text(&text, style)
            }
            Event::Code(text) => {
                let style = self.style().patch(code_style(self.theme));
                self.push_text(&text, style)
            }
            // The Analyst breaks lines where it means to, as in a comment on
            // GitHub: a line break in a paragraph stays.
            Event::SoftBreak => self.push_text("\n", self.style()),
            Event::HardBreak => self.push_text("\n", self.style()),
            Event::Rule => {
                self.flush();
                self.push(Block::Rule);
            }
            Event::FootnoteReference(name) => self.push_text(&format!("[{name}]"), self.style()),
            Event::TaskListMarker(done) => {
                self.push_text(if done { "[x] " } else { "[ ] " }, self.style())
            }
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Emphasis => self
                .styles
                .push(Style::new().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.styles.push(Style::new().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self
                .styles
                .push(Style::new().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                self.styles
                    .push(Style::new().add_modifier(Modifier::UNDERLINED));
                self.links.push(dest_url.to_string());
            }
            Tag::Paragraph | Tag::Heading { .. } | Tag::HtmlBlock => self.flush(),
            Tag::TableCell => self.inline = Some(Vec::new()),
            Tag::TableHead | Tag::TableRow => {}
            Tag::BlockQuote(_) => {
                self.flush();
                self.stack.push(Frame::Quote(Vec::new()));
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.stack.push(Frame::Code {
                    language,
                    code: String::new(),
                });
            }
            Tag::List(start) => {
                self.flush();
                self.stack.push(Frame::List {
                    start,
                    items: Vec::new(),
                });
            }
            Tag::Item => {
                self.flush();
                self.stack.push(Frame::Item(Vec::new()));
            }
            Tag::Table(aligns) => {
                self.flush();
                self.stack.push(Frame::Table {
                    aligns,
                    head: Vec::new(),
                    rows: Vec::new(),
                    row: Vec::new(),
                });
            }
            _ => self.flush(),
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.styles.pop();
            }
            TagEnd::Link | TagEnd::Image => {
                self.styles.pop();
                let url = self.links.pop().unwrap_or_default();
                if !url.is_empty() {
                    let style = self.style().fg(self.theme.dim);
                    self.push_text(&format!(" ({url})"), style);
                }
            }
            TagEnd::Paragraph | TagEnd::HtmlBlock => self.flush(),
            TagEnd::Heading(_) => {
                if let Some(inline) = self.inline.take() {
                    self.push(Block::Heading(inline));
                }
            }
            TagEnd::TableCell => {
                let cell = self.inline.take().unwrap_or_default();
                if let Some(Frame::Table { row, .. }) = self.stack.last_mut() {
                    row.push(cell);
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                if let Some(Frame::Table {
                    head, rows, row, ..
                }) = self.stack.last_mut()
                {
                    let cells = std::mem::take(row);
                    if matches!(tag, TagEnd::TableHead) {
                        *head = cells;
                    } else {
                        rows.push(cells);
                    }
                }
            }
            TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::Table => {
                if !self.in_table() || matches!(tag, TagEnd::Table) {
                    self.flush();
                }
                self.close();
            }
            _ => self.flush(),
        }
    }
}

fn code_style(theme: &Theme) -> Style {
    Style::new()
        .fg(theme.markdown.code)
        .bg(theme.markdown.code_bg)
}

/// `text` without what must not reach a terminal: control characters become
/// U+FFFD, text-direction marks go, a tab is a space.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            !matches!(
                c,
                '\u{200e}' | '\u{200f}' | '\u{61c}' | '\u{2028}' | '\u{2029}'
                    | '\u{202a}'..='\u{202e}'
                    | '\u{2066}'..='\u{2069}'
            )
        })
        .map(|c| match c {
            '\t' => ' ',
            '\n' => '\n',
            c if c.is_control() => '\u{fffd}',
            c => c,
        })
        .collect()
}

fn flatten(blocks: &[Block], out: &mut Vec<String>) {
    let text = |inline: &Inline| -> String {
        let joined: String = inline.iter().map(|(t, _)| t.as_str()).collect();
        joined.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    for block in blocks {
        match block {
            Block::Text(inline) | Block::Heading(inline) => out.push(text(inline)),
            Block::List { items, .. } => {
                for item in items {
                    flatten(item, out);
                }
            }
            Block::Quote(blocks) => flatten(blocks, out),
            Block::Code { code, .. } => {
                out.push(clean(code).split_whitespace().collect::<Vec<_>>().join(" "))
            }
            Block::Rule => {}
            Block::Table { head, rows, .. } => {
                for row in std::iter::once(head).chain(rows) {
                    out.push(row.iter().map(text).collect::<Vec<_>>().join(" · "));
                }
            }
        }
    }
    out.retain(|w| !w.is_empty());
}

fn collect_fenced(blocks: &[Block], language: &str, found: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Code { language: l, code } if l == language => found.push(code.clone()),
            Block::Quote(blocks) => collect_fenced(blocks, language, found),
            Block::List { items, .. } => {
                for item in items {
                    collect_fenced(item, language, found);
                }
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------- layout

fn char_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

fn str_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

fn line_width(line: &Line) -> usize {
    line.spans.iter().map(|s| str_width(&s.content)).sum()
}

/// `blocks` laid out for `width`; a blank line between blocks unless `tight`.
fn layout(
    theme: &Theme,
    blocks: &[Block],
    width: usize,
    tight: bool,
    pictures: Pictures,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 && !tight {
            lines.push(Line::default());
        }
        lines.extend(layout_block(theme, block, width, pictures));
    }
    lines
}

fn layout_block(
    theme: &Theme,
    block: &Block,
    width: usize,
    pictures: Pictures,
) -> Vec<Line<'static>> {
    match block {
        Block::Text(inline) => wrap(inline, width),
        Block::Heading(inline) => {
            let heading = Style::new()
                .fg(theme.markdown.heading)
                .add_modifier(Modifier::BOLD);
            let styled: Inline = inline
                .iter()
                .map(|(text, style)| (text.clone(), heading.patch(*style)))
                .collect();
            wrap(&styled, width)
        }
        Block::List { start, items } => {
            let mut lines = Vec::new();
            for (n, item) in items.iter().enumerate() {
                let marker = match start {
                    Some(first) => format!("{}. ", first.saturating_add(n as u64)),
                    None => "• ".to_string(),
                };
                let indent = str_width(&marker);
                let inner = layout(
                    theme,
                    item,
                    width.saturating_sub(indent).max(1),
                    true,
                    pictures,
                );
                lines.extend(prefixed(
                    inner,
                    Span::styled(marker, Style::new().fg(theme.markdown.heading)),
                    Span::raw(" ".repeat(indent)),
                ));
            }
            lines
        }
        Block::Quote(blocks) => {
            let bar = Span::styled("│ ", Style::new().fg(theme.dim));
            let inner = layout(
                theme,
                blocks,
                width.saturating_sub(2).max(1),
                false,
                pictures,
            );
            prefixed(inner, bar.clone(), bar)
        }
        Block::Code { language, code } => match pictures(language, code) {
            Some(picture) => picture.iter().map(|l| Line::raw(clean(l))).collect(),
            None => code_block(theme, code, width),
        },
        Block::Rule => vec![Line::from(Span::styled(
            "─".repeat(width),
            Style::new().fg(theme.dim),
        ))],
        Block::Table { aligns, head, rows } => table(theme, aligns, head, rows, width),
    }
}

/// `lines` behind `first` on the first line and `rest` on the others; a lone
/// prefix when there are none.
fn prefixed(
    lines: Vec<Line<'static>>,
    first: Span<'static>,
    rest: Span<'static>,
) -> Vec<Line<'static>> {
    if lines.is_empty() {
        return vec![Line::from(first)];
    }
    lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let mut spans = vec![if i == 0 { first.clone() } else { rest.clone() }];
            spans.extend(line.spans);
            Line::from(spans)
        })
        .collect()
}

/// A code block: on a background, one line per line of code, cut with `…`
/// rather than wrapped.
fn code_block(theme: &Theme, code: &str, width: usize) -> Vec<Line<'static>> {
    let style = Style::new().fg(theme.fg).bg(theme.markdown.block_bg);
    code.trim_end_matches('\n')
        .split('\n')
        .map(|line| {
            let text = clean(&line.replace('\t', TAB));
            let text = if str_width(&text) > width {
                let mut cut = cut_to(&text, width.saturating_sub(1));
                cut.push('…');
                cut
            } else {
                text
            };
            let pad = width.saturating_sub(str_width(&text));
            Line::from(Span::styled(format!("{text}{}", " ".repeat(pad)), style))
        })
        .collect()
}

/// The longest start of `text` at most `width` cells wide.
fn cut_to(text: &str, width: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|&c| {
            used += char_width(c);
            used <= width
        })
        .collect()
}

/// `line` cut to `width` cells.
fn truncate(line: Line<'static>, width: usize) -> Line<'static> {
    if line_width(&line) <= width {
        return line;
    }
    let mut left = width;
    let mut spans = Vec::new();
    for span in line.spans {
        let w = str_width(&span.content);
        if w <= left {
            left -= w;
            spans.push(span);
        } else {
            let cut = cut_to(&span.content, left);
            spans.push(Span::styled(cut, span.style));
            break;
        }
    }
    Line::from(spans)
}

enum Token {
    Word(Vec<(String, Style)>),
    Space(Style),
    Break,
}

fn tokens(inline: &Inline) -> Vec<Token> {
    let mut out = Vec::new();
    let mut word: Vec<(String, Style)> = Vec::new();
    let end_word = |word: &mut Vec<(String, Style)>, out: &mut Vec<Token>| {
        if !word.is_empty() {
            out.push(Token::Word(std::mem::take(word)));
        }
    };
    for (text, style) in inline {
        for c in text.chars() {
            match c {
                '\n' => {
                    end_word(&mut word, &mut out);
                    out.push(Token::Break);
                }
                c if c.is_whitespace() => {
                    end_word(&mut word, &mut out);
                    out.push(Token::Space(*style));
                }
                c => match word.last_mut() {
                    Some((part, s)) if s == style => part.push(c),
                    _ => word.push((c.to_string(), *style)),
                },
            }
        }
    }
    end_word(&mut word, &mut out);
    out
}

/// `inline` broken into lines of at most `width` cells at spaces; a word
/// wider than that is cut.
fn wrap(inline: &Inline, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    let mut space: Option<Style> = None;
    for token in tokens(inline) {
        match token {
            Token::Break => {
                lines.push(Line::from(std::mem::take(&mut line)));
                used = 0;
                space = None;
            }
            Token::Space(style) => {
                if used > 0 {
                    space = Some(style);
                }
            }
            Token::Word(parts) => {
                let w: usize = parts.iter().map(|(t, _)| str_width(t)).sum();
                let gap = usize::from(space.is_some());
                if used > 0 && used + gap + w > width {
                    lines.push(Line::from(std::mem::take(&mut line)));
                    used = 0;
                    space = None;
                }
                if let Some(style) = space.take() {
                    line.push(Span::styled(" ", style));
                    used += 1;
                }
                for (text, style) in parts {
                    let mut chunk = String::new();
                    for c in text.chars() {
                        let cw = char_width(c);
                        if used + cw > width && used > 0 {
                            if !chunk.is_empty() {
                                line.push(Span::styled(std::mem::take(&mut chunk), style));
                            }
                            lines.push(Line::from(std::mem::take(&mut line)));
                            used = 0;
                        }
                        chunk.push(c);
                        used += cw;
                    }
                    if !chunk.is_empty() {
                        line.push(Span::styled(chunk, style));
                    }
                }
            }
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(Line::from(line));
    }
    lines
}

// ------------------------------------------------------------------- tables

fn inline_width(inline: &Inline) -> usize {
    let text: String = inline.iter().map(|(t, _)| t.as_str()).collect();
    text.split('\n').map(str_width).max().unwrap_or(0)
}

fn longest_word(inline: &Inline) -> usize {
    let text: String = inline.iter().map(|(t, _)| t.as_str()).collect();
    text.split_whitespace().map(str_width).max().unwrap_or(0)
}

fn bold(inline: &Inline) -> Inline {
    inline
        .iter()
        .map(|(t, s)| (t.clone(), s.add_modifier(Modifier::BOLD)))
        .collect()
}

/// A table in a frame when its columns fit `width`, squeezed with the text
/// wrapped inside the cells when they fit squeezed, and as cards otherwise.
fn table(
    theme: &Theme,
    aligns: &[Alignment],
    head: &[Inline],
    rows: &[Vec<Inline>],
    width: usize,
) -> Vec<Line<'static>> {
    let columns = rows
        .iter()
        .map(Vec::len)
        .chain([head.len()])
        .max()
        .unwrap_or(0);
    if columns == 0 {
        return Vec::new();
    }
    let empty = Inline::new();
    let cell = |row: &[Inline], c: usize| row.get(c).unwrap_or(&empty).clone();
    let head: Vec<Inline> = (0..columns).map(|c| bold(&cell(head, c))).collect();
    let rows: Vec<Vec<Inline>> = rows
        .iter()
        .map(|row| (0..columns).map(|c| cell(row, c)).collect())
        .collect();
    let all = || std::iter::once(&head).chain(rows.iter());
    let natural: Vec<usize> = (0..columns)
        .map(|c| {
            all()
                .map(|row| inline_width(&row[c]))
                .max()
                .unwrap_or(0)
                .max(1)
        })
        .collect();
    let words: Vec<usize> = (0..columns)
        .map(|c| {
            let word = all().map(|row| longest_word(&row[c])).max().unwrap_or(0);
            natural[c].min(word.max(1))
        })
        .collect();
    let least: Vec<usize> = (0..columns)
        .map(|c| words[c].clamp(MIN_COLUMN, MAX_WORD_COLUMN).min(natural[c]))
        .collect();
    let frame = 3 * columns + 1;
    if least.iter().sum::<usize>() + frame > width {
        return cards(&head, &rows, width);
    }
    // Squeeze the widest column, first only where the text wraps between
    // words, then cutting words too.
    let mut widths = natural;
    for floor in [&words, &least] {
        while widths.iter().sum::<usize>() + frame > width {
            let Some(widest) = (0..columns)
                .filter(|&c| widths[c] > floor[c])
                .max_by_key(|&c| (widths[c], std::cmp::Reverse(c)))
            else {
                break;
            };
            widths[widest] -= 1;
        }
    }
    let align = |c: usize| aligns.get(c).copied().unwrap_or(Alignment::None);
    let laid = |row: &[Inline]| -> Vec<Vec<Line<'static>>> {
        row.iter()
            .enumerate()
            .map(|(c, cell)| wrap(cell, widths[c]))
            .collect()
    };
    let head_cells = laid(&head);
    let body: Vec<_> = rows.iter().map(|row| laid(row)).collect();
    let wrapped = std::iter::once(&head_cells)
        .chain(body.iter())
        .any(|row| row.iter().any(|cell| cell.len() > 1));
    let border = |left: &str, mid: &str, right: &str| {
        let parts: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
        Line::from(Span::styled(
            format!("{left}{}{right}", parts.join(mid)),
            Style::new().fg(theme.dim),
        ))
    };
    let mut lines = vec![border("┌", "┬", "┐")];
    let draw_row = |cells: &[Vec<Line<'static>>], lines: &mut Vec<Line<'static>>| {
        let height = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);
        for i in 0..height {
            let mut spans = vec![Span::styled("│", Style::new().fg(theme.dim))];
            for (c, cell) in cells.iter().enumerate() {
                let content = cell.get(i).cloned().unwrap_or_default();
                let room = widths[c].saturating_sub(line_width(&content));
                let (before, after) = match align(c) {
                    Alignment::Right => (room, 0),
                    Alignment::Center => (room / 2, room - room / 2),
                    Alignment::Left | Alignment::None => (0, room),
                };
                spans.push(Span::raw(" ".repeat(before + 1)));
                spans.extend(content.spans);
                spans.push(Span::raw(" ".repeat(after + 1)));
                spans.push(Span::styled("│", Style::new().fg(theme.dim)));
            }
            lines.push(Line::from(spans));
        }
    };
    draw_row(&head_cells, &mut lines);
    lines.push(border("├", "┼", "┤"));
    for (r, row) in body.iter().enumerate() {
        if r > 0 && wrapped {
            lines.push(border("├", "┼", "┤"));
        }
        draw_row(row, &mut lines);
    }
    lines.push(border("└", "┴", "┘"));
    lines
}

/// Each row of a table as "Header: value" lines, a blank line between rows.
fn cards(head: &[Inline], rows: &[Vec<Inline>], width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        if r > 0 {
            lines.push(Line::default());
        }
        for (name, value) in head.iter().zip(row) {
            let mut inline = name.clone();
            if !name.is_empty() {
                inline.push((": ".into(), Style::new().add_modifier(Modifier::BOLD)));
            }
            inline.extend(value.iter().cloned());
            lines.extend(wrap(&inline, width));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;
    use unicode_width::UnicodeWidthStr;

    fn render(source: &str, width: usize) -> Vec<Line<'static>> {
        super::render(&crate::theme::DARK, source, width)
    }

    fn render_with(source: &str, width: usize, pictures: Pictures) -> Vec<Line<'static>> {
        super::render_with(&crate::theme::DARK, source, width, pictures)
    }

    fn text(source: &str, width: usize) -> Vec<String> {
        plain(&render(source, width))
            .into_iter()
            .map(|l| l.trim_end().to_string())
            .collect()
    }

    /// The style of the first span whose text contains `needle`.
    fn style_of(lines: &[Line], needle: &str) -> ratatui::style::Style {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.contains(needle))
            .unwrap_or_else(|| panic!("no span with {needle:?}"))
            .style
    }

    #[test]
    fn headings_lose_their_marks_and_get_a_line_above() {
        let lines = render("Intro\n\n## Риски\n\nText", 40);
        assert_eq!(
            text("Intro\n\n## Риски\n\nText", 40),
            ["Intro", "", "Риски", "", "Text"]
        );
        let style = style_of(&lines, "Риски");
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(style.fg.is_some());
    }

    #[test]
    fn emphasis_and_code_are_styles_not_marks() {
        let source = "a **bold** and *it* and `code` here";
        assert_eq!(text(source, 80), ["a bold and it and code here"]);
        let lines = render(source, 80);
        assert!(style_of(&lines, "bold")
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(style_of(&lines, "it")
            .add_modifier
            .contains(Modifier::ITALIC));
        assert!(style_of(&lines, "code").bg.is_some());
    }

    #[test]
    fn a_line_break_in_a_paragraph_stays() {
        assert_eq!(text("one\ntwo", 40), ["one", "two"]);
    }

    #[test]
    fn paragraphs_wrap_at_spaces() {
        assert_eq!(
            text("one two three four five", 9),
            ["one two", "three", "four five"]
        );
    }

    #[test]
    fn nested_list_wraps_with_a_hanging_indent() {
        let source =
            "- first item that wraps\n  - inner item wraps too\n- second\n\n1. one\n2. two";
        assert_eq!(
            text(source, 14),
            [
                "• first item",
                "  that wraps",
                "  • inner item",
                "    wraps too",
                "• second",
                "",
                "1. one",
                "2. two",
            ]
        );
    }

    #[test]
    fn code_block_is_cut_not_wrapped() {
        let source = "```rust\nlet x = 1;\nlet long_name = something_long();\n```";
        let lines = text(source, 16);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0], "let x = 1;");
        assert!(lines[1].starts_with("let long_name"), "{lines:?}");
        assert!(lines[1].ends_with('…'), "{lines:?}");
        assert_eq!(lines[1].width(), 16);
        let rendered = render(source, 16);
        assert!(style_of(&rendered, "let x").bg.is_some());
    }

    #[test]
    fn quote_has_a_bar() {
        assert_eq!(
            text("> quoted text here", 10),
            ["│ quoted", "│ text", "│ here"]
        );
    }

    #[test]
    fn link_is_underlined_with_a_dim_address() {
        let source = "see [docs](https://x.io) now";
        assert_eq!(text(source, 80), ["see docs (https://x.io) now"]);
        let lines = render(source, 80);
        assert!(style_of(&lines, "docs")
            .add_modifier
            .contains(Modifier::UNDERLINED));
        assert!(style_of(&lines, "https").fg.is_some());
    }

    #[test]
    fn one_line_has_the_words_without_the_marks() {
        assert_eq!(
            one_line("## Риски\n\nЭто **важно**.\n\n- `a`\n- b\n\n| x | y |\n|---|---|\n| 1 | 2 |"),
            "Риски Это важно. a b x · y 1 · 2"
        );
    }

    #[test]
    fn rule_spans_the_width() {
        assert_eq!(text("a\n\n---\n\nb", 5), ["a", "", "─────", "", "b"]);
    }

    #[test]
    fn pictures_replace_fenced_blocks() {
        let pictures = |lang: &str, code: &str| {
            (lang == "mermaid").then(|| vec![format!("[{}]", code.trim())])
        };
        let lines = plain(&render_with("```mermaid\ngraph\n```", 20, &pictures));
        assert_eq!(lines, ["[graph]"]);
    }

    #[test]
    fn control_characters_are_replaced() {
        let lines = text("a\x1b[31mb\u{202e}c", 20);
        assert_eq!(lines, ["a\u{fffd}[31mbc"]);
    }

    #[test]
    fn broken_markdown_does_not_panic() {
        for source in [
            "**unclosed",
            "```\nno end",
            "| a |\n|---",
            "|a|b|\n|-|-|\n|1|",
            "- \n  - \n    -",
            "> > > ",
            "[x](",
            "\u{0}\u{7}",
        ] {
            for width in [0, 1, 2, 5, 40] {
                render(source, width);
            }
        }
    }

    const TABLE: &str = "| Имя | Тип | Описание |\n|:---|:---:|---:|\n| `load` | fn | Загружает данные |\n| 漢字 | ok | x |";

    #[test]
    fn table_that_fits_has_a_frame() {
        assert_eq!(
            text(TABLE, 60),
            [
                "┌──────┬─────┬──────────────────┐",
                "│ Имя  │ Тип │         Описание │",
                "├──────┼─────┼──────────────────┤",
                "│ load │ fn  │ Загружает данные │",
                "│ 漢字 │ ok  │                x │",
                "└──────┴─────┴──────────────────┘",
            ]
        );
        let lines = render(TABLE, 60);
        assert!(style_of(&lines, "Имя")
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(style_of(&lines, "load").bg.is_some());
    }

    #[test]
    fn narrow_table_wraps_inside_cells() {
        let lines = text(TABLE, 30);
        assert!(lines.iter().all(|l| l.width() <= 30), "{lines:?}");
        let widths: Vec<usize> = lines.iter().map(|l| l.width()).collect();
        assert!(widths.iter().all(|&w| w == widths[0]), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("Загружает")), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("данные")), "{lines:?}");
        assert!(!lines.iter().any(|l| l.contains("Загружает данные")));
        // The columns line up on every row.
        let bars = |l: &str| -> Vec<usize> {
            let mut at = 0;
            let mut out = Vec::new();
            for c in l.chars() {
                if "│┼├┤┬┴┌┐└┘".contains(c) {
                    out.push(at);
                }
                at += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            }
            out
        };
        let first = bars(&lines[0]);
        assert!(lines.iter().all(|l| bars(l) == first), "{lines:?}");
    }

    #[test]
    fn table_too_narrow_for_columns_becomes_cards() {
        let lines = text(TABLE, 14);
        assert!(!lines.iter().any(|l| l.contains('┼')), "{lines:?}");
        assert!(lines.contains(&"Имя: load".to_string()), "{lines:?}");
        assert!(lines.contains(&"Тип: fn".to_string()), "{lines:?}");
        assert!(lines.iter().all(|l| l.width() <= 14), "{lines:?}");
    }

    #[test]
    fn delimiter_row_never_reaches_the_screen() {
        let lines = text("| a | b |\n|---|---|\n| 1 | 2 |", 40);
        assert!(!lines.iter().any(|l| l.contains("|---")), "{lines:?}");
    }

    #[test]
    fn nothing_is_wider_than_the_box() {
        let source = format!(
            "# {}\n\n{}\n\n- {}\n\n> {}",
            "w".repeat(50),
            "x".repeat(50),
            "y".repeat(50),
            "z".repeat(50)
        );
        for line in text(&source, 12) {
            assert!(line.width() <= 12, "{line:?}");
        }
    }
}

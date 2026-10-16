// PROTOTYPE shared rendering: chrome (prototype bar, status, command line), the
// diff content renderer, the list renderers used by panes/pickers, and overlays.
// The per-variant *layouts* of the main screen are in va.rs / vb.rs / vc.rs.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::*;
use crate::data::*;

pub const DIM: Color = Color::Indexed(244);
pub const FG: Color = Color::Indexed(252);
pub const ACCENT: Color = Color::Indexed(75);
pub const GREEN: Color = Color::Indexed(114);
pub const YELLOW: Color = Color::Indexed(179);
pub const RED: Color = Color::Indexed(174);
pub const ROUND1: Color = Color::Indexed(74);
pub const ROUND2: Color = Color::Indexed(208);

pub fn variant_name(v: Variant) -> &'static str {
    match v {
        Variant::A => "A  IDE-like",
        Variant::B => "B  Diff-first",
        Variant::C => "C  Review queue",
    }
}

pub fn round_color(round: u8) -> Color {
    if round <= 1 {
        ROUND1
    } else {
        ROUND2
    }
}

pub fn trunc(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w {
        s.to_string()
    } else if w == 0 {
        String::new()
    } else {
        let mut t: String = s.chars().take(w - 1).collect();
        t.push('…');
        t
    }
}

pub fn wrap(text: &str, w: usize) -> Vec<String> {
    let w = w.max(8);
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            let mut word = word.to_string();
            while word.chars().count() > w {
                let head: String = word.chars().take(w).collect();
                word = word.chars().skip(w).collect();
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                out.push(head);
            }
            if cur.is_empty() {
                cur = word;
            } else if cur.chars().count() + 1 + word.chars().count() <= w {
                cur.push(' ');
                cur.push_str(&word);
            } else {
                out.push(std::mem::take(&mut cur));
                cur = word;
            }
        }
        out.push(cur);
    }
    out
}

const KEYWORDS: &[&str] = &[
    "fn", "let", "pub", "use", "mod", "struct", "enum", "impl", "trait", "for", "in", "if", "else", "match", "return", "async",
    "await", "mut", "loop", "where", "const", "self", "Self", "crate", "super", "as", "move", "true", "false",
];

fn fg_colors(c: &[char]) -> Vec<Color> {
    let mut out = vec![FG; c.len()];
    let mut i = 0;
    while i < c.len() {
        if c[i] == '/' && i + 1 < c.len() && c[i + 1] == '/' {
            for o in out.iter_mut().skip(i) {
                *o = Color::Indexed(243);
            }
            break;
        }
        if c[i] == '"' {
            let mut j = i + 1;
            while j < c.len() && c[j] != '"' {
                j += 1;
            }
            let end = (j + 1).min(c.len());
            for o in out.iter_mut().take(end).skip(i) {
                *o = Color::Indexed(150);
            }
            i = end;
            continue;
        }
        if c[i].is_alphabetic() || c[i] == '_' {
            let mut j = i;
            while j < c.len() && (c[j].is_alphanumeric() || c[j] == '_') {
                j += 1;
            }
            let w: String = c[i..j].iter().collect();
            let col = if KEYWORDS.contains(&w.as_str()) {
                Color::Indexed(176)
            } else if c[i].is_uppercase() {
                Color::Indexed(110)
            } else {
                FG
            };
            for o in out.iter_mut().take(j).skip(i) {
                *o = col;
            }
            i = j;
            continue;
        }
        if c[i].is_ascii_digit() {
            let mut j = i;
            while j < c.len() && (c[j].is_ascii_digit() || c[j] == '_') {
                j += 1;
            }
            for o in out.iter_mut().take(j).skip(i) {
                *o = Color::Indexed(180);
            }
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

/// Render `text` into exactly `w` cells: syntax colours, optional line background and span background.
pub fn code_spans(text: &str, w: usize, span: Option<(usize, usize)>, base_bg: Option<Color>, span_bg: Option<Color>) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let fg = fg_colors(&chars);
    let cut = chars.len() > w;
    let shown = if cut { w.saturating_sub(1) } else { chars.len() };
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut cur = String::new();
    let mut cur_style: Option<Style> = None;
    for i in 0..w {
        let (ch, fgc, in_span) = if i < shown {
            (chars[i], fg[i], span.map_or(false, |(a, b)| i >= a && i < b))
        } else if i == shown && cut {
            ('…', DIM, false)
        } else {
            (' ', FG, false)
        };
        let bg = if in_span { span_bg } else { base_bg };
        let mut st = Style::new().fg(fgc);
        if let Some(b) = bg {
            st = st.bg(b);
        }
        if Some(st) == cur_style {
            cur.push(ch);
        } else {
            if let Some(s) = cur_style {
                out.push(Span::styled(std::mem::take(&mut cur), s));
            }
            cur.push(ch);
            cur_style = Some(st);
        }
    }
    if let Some(s) = cur_style {
        out.push(Span::styled(cur, s));
    }
    out
}

// ---------------------------------------------------------------- chrome

pub fn proto_bar(f: &mut Frame, area: Rect, app: &App) {
    let st = Style::new().fg(Color::Indexed(231)).bg(Color::Indexed(53));
    let line = Line::from(vec![
        Span::styled(" PROTOTYPE ", st.add_modifier(Modifier::BOLD).bg(Color::Indexed(161))),
        Span::styled(format!(" variant {}   F1/F2/F3 or :variant a|b|c   (throwaway: fake data, nothing is written) ", variant_name(app.variant)), st),
    ]);
    let w = area.width as usize;
    let used: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
    let mut spans = line.spans;
    if used < w {
        spans.push(Span::styled(" ".repeat(w - used), st));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub fn status_line(f: &mut Frame, area: Rect, app: &App) {
    let (name, col) = match (app.mode, app.editor.as_ref().map(|e| e.insert)) {
        (Mode::Comment, Some(true)) => ("INSERT", Color::Indexed(114)),
        (Mode::Comment, _) => ("COMMENT", Color::Indexed(215)),
        (Mode::Visual, _) => ("VISUAL", Color::Indexed(178)),
        (Mode::Command, _) => ("COMMAND", Color::Indexed(141)),
        (Mode::Search, _) => ("SEARCH", Color::Indexed(141)),
        (Mode::Normal, _) => ("NORMAL", ACCENT),
    };
    let bar = Style::new().fg(FG).bg(Color::Indexed(236));
    let (done, total) = app.progress();
    let mut left = vec![
        Span::styled(format!(" {name} "), Style::new().fg(Color::Indexed(16)).bg(col).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  Scope: {}", app.scope_label()), bar),
        Span::styled(format!("  ✓ {done}/{total}"), bar.fg(GREEN)),
    ];
    if app.unreviewed_only {
        left.push(Span::styled("  [unreviewed only]", bar.fg(YELLOW)));
    }
    left.push(Span::styled(format!("  Round {}", app.round), bar.fg(round_color(app.round))));
    let right = format!(
        "{}  ? help ",
        match app.dlayout {
            DiffLayout::Auto => "layout:auto",
            DiffLayout::Side => "layout:side",
            DiffLayout::Inline => "layout:inline",
        }
    );
    let used: usize = left.iter().map(|s| s.content.chars().count()).sum();
    let w = area.width as usize;
    if used + right.chars().count() < w {
        left.push(Span::styled(" ".repeat(w - used - right.chars().count()), bar));
        left.push(Span::styled(right, bar.fg(DIM)));
    } else if used < w {
        left.push(Span::styled(" ".repeat(w - used), bar));
    }
    f.render_widget(Paragraph::new(Line::from(left)), area);
}

pub fn cmd_line(f: &mut Frame, area: Rect, app: &App) {
    let line = match app.mode {
        Mode::Command => Line::from(vec![Span::raw(":"), Span::raw(app.cmd.clone()), Span::styled("█", Style::new().fg(ACCENT))]),
        Mode::Search => Line::from(vec![Span::raw("/"), Span::raw(app.search_input.clone()), Span::styled("█", Style::new().fg(ACCENT))]),
        Mode::Comment => {
            let ins = app.editor.as_ref().map_or(false, |e| e.insert);
            Line::styled(
                if ins { "-- INSERT --   Esc: back   Enter: newline" } else { "-- COMMENT --   i: type   Enter: save   Esc: cancel" },
                Style::new().fg(YELLOW),
            )
        }
        _ => {
            if let Some(t) = &app.toast {
                Line::styled(t.clone(), Style::new().fg(YELLOW))
            } else if let Some(p) = app.pending {
                Line::styled(format!("{p}…"), Style::new().fg(DIM))
            } else if !app.search_hits.is_empty() {
                Line::styled(format!("/{}  [{}/{}]   n/N next/prev   Esc clears", app.search, app.search_idx + 1, app.search_hits.len()), Style::new().fg(DIM))
            } else {
                Line::styled(hint(app), Style::new().fg(DIM))
            }
        }
    };
    f.render_widget(Paragraph::new(line), area);
}

fn hint(app: &App) -> String {
    match app.variant {
        Variant::A => "j/k move  Ctrl-w/h/l panes  gc commits  gf files  m reviewed  u unreviewed  c comment  Y prompt  S summary  ? help".into(),
        Variant::B => "j/k move  ]f/[f file  ]h/[h hunk  gc commits  gf files  m reviewed  u unreviewed  c comment  Y prompt  S summary  ? help".into(),
        Variant::C => "n/p next/prev in queue  m done+next  gc commits  gf groups  u browse all  c comment  Y prompt  S summary  ? help".into(),
    }
}

// ---------------------------------------------------------------- lists (shared by panes and pickers)

pub fn status_glyph(s: Status) -> Span<'static> {
    match s {
        Status::Done => Span::styled("✓", Style::new().fg(GREEN)),
        Status::Partial => Span::styled("◐", Style::new().fg(YELLOW)),
        Status::Todo => Span::styled("·", Style::new().fg(DIM)),
    }
}

/// Commit list lines with Round separators. Returns (lines, line index of the cursor commit).
pub fn commit_lines(app: &App, width: usize, show_cursor: bool) -> (Vec<Line<'static>>, usize) {
    let mut lines = Vec::new();
    let mut cursor_line = 0;
    for r in 1..=app.round {
        let title = if r == 1 { "Round 1".to_string() } else { format!("Round {r}  (after Prompt #{})", r - 1) };
        lines.push(Line::styled(format!("── {title} ──"), Style::new().fg(round_color(r)).add_modifier(Modifier::DIM)));
        let mut any = false;
        for (i, c) in app.commits.iter().enumerate() {
            if c.round != r {
                continue;
            }
            any = true;
            let in_scope = app.in_scope(i);
            let partial = !app.scope_is_whole();
            let is_cursor = show_cursor && i == app.commit_cursor;
            if i == app.commit_cursor {
                cursor_line = lines.len();
            }
            let marker = if is_cursor {
                Span::styled("▶", Style::new().fg(YELLOW))
            } else if partial && in_scope {
                Span::styled("▌", Style::new().fg(ACCENT))
            } else {
                Span::raw(" ")
            };
            let text = trunc(&format!(" c{} {} {}", i + 1, c.sha, c.msg), width.saturating_sub(1));
            let style = if partial && !in_scope { Style::new().fg(DIM) } else { Style::new().fg(FG) };
            lines.push(Line::from(vec![marker, Span::styled(text, style)]));
        }
        if !any {
            lines.push(Line::styled("   waiting for the Author agent…", Style::new().fg(DIM)));
        }
    }
    (lines, cursor_line)
}

/// Groups + files lines. `cursor` is the index into visible_files(); returns (lines, cursor line).
pub fn file_lines(app: &App, width: usize, cursor: usize, highlight: bool) -> (Vec<Line<'static>>, usize) {
    let vf = app.visible_files();
    let mut lines = Vec::new();
    let mut cursor_line = 0;
    let mut last_group = usize::MAX;
    for (pos, fi) in vf.iter().enumerate() {
        let f = &app.files[*fi];
        if f.group != last_group {
            last_group = f.group;
            let g = &app.groups[f.group];
            let (d, t) = app.group_progress(f.group);
            let head = format!("▾ {}. {}{}", f.group + 1, g.name, if g.mechanical { "  [mech]" } else { "" });
            let tail = format!(" {d}/{t}");
            let pad = width.saturating_sub(head.chars().count() + tail.chars().count());
            lines.push(Line::from(vec![
                Span::styled(trunc(&head, width.saturating_sub(tail.len())), Style::new().add_modifier(Modifier::BOLD).fg(Color::Indexed(250))),
                Span::raw(" ".repeat(pad)),
                Span::styled(tail, Style::new().fg(DIM)),
            ]));
        }
        if pos == cursor {
            cursor_line = lines.len();
        }
        let mut name = f.path.to_string();
        let room = width.saturating_sub(7);
        if name.chars().count() > room {
            // keep the tail of the path: that is the informative part
            let skip = name.chars().count() - room + 1;
            name = format!("…{}", name.chars().skip(skip).collect::<String>());
        }
        let cm = app.comment_count(*fi);
        let mut spans = vec![Span::raw("  "), status_glyph(app.file_status(*fi)), Span::raw(" "), Span::raw(name.clone())];
        if cm > 0 {
            spans.push(Span::styled(" ✎", Style::new().fg(YELLOW)));
        }
        let mut line = Line::from(spans);
        if pos == cursor && highlight {
            line = line.style(Style::new().add_modifier(Modifier::REVERSED));
        } else if pos == cursor {
            line = line.style(Style::new().bg(Color::Indexed(238)));
        }
        lines.push(line);
    }
    if vf.is_empty() {
        lines.push(Line::styled("  nothing to review here", Style::new().fg(DIM)));
    }
    (lines, cursor_line)
}

/// Keep `cursor_line` inside a window of `h` lines; returns the scroll offset.
pub fn scroll_for(total: usize, h: usize, cursor_line: usize) -> usize {
    if total <= h {
        return 0;
    }
    let off = cursor_line.saturating_sub(h / 2);
    off.min(total - h)
}

// ---------------------------------------------------------------- diff content

pub struct DLine {
    pub src: Option<usize>,
    pub line: Line<'static>,
}

const ADD_BG: Color = Color::Indexed(22);
const ADD_SPAN: Color = Color::Indexed(28);
const DEL_BG: Color = Color::Indexed(52);
const DEL_SPAN: Color = Color::Indexed(88);

fn gutter(app: &App, r: &Row, src: usize, first: bool) -> Vec<Span<'static>> {
    let is_cursor = src == app.diff_cursor;
    let sel = app.diff_anchor.map_or(false, |a| src >= a.min(app.diff_cursor) && src <= a.max(app.diff_cursor));
    let m = if first && is_cursor {
        Span::styled("▶", Style::new().fg(YELLOW).add_modifier(Modifier::BOLD))
    } else if sel {
        Span::styled("┃", Style::new().fg(ACCENT))
    } else {
        Span::raw(" ")
    };
    let tag = if r.kind != Kind::Ctx && r.commit != usize::MAX && first {
        let round = app.commits[r.commit].round;
        Span::styled(format!("{}", r.commit + 1), Style::new().fg(round_color(round)).add_modifier(Modifier::BOLD))
    } else {
        Span::raw(" ")
    };
    vec![m, tag, Span::raw(" ")]
}

fn lineno(n: usize, cursor: bool) -> Span<'static> {
    let s = if n == 0 { "    ".to_string() } else { format!("{n:>4}") };
    let mut st = Style::new().fg(DIM);
    if cursor {
        st = st.fg(YELLOW);
    }
    Span::styled(s, st)
}

fn side_line(app: &App, r: &Row, src: usize, width: usize) -> Line<'static> {
    let avail = width.saturating_sub(3 + 1);
    let lw = avail / 2;
    let rw = avail - lw;
    let cur = src == app.diff_cursor;
    let mut spans = gutter(app, r, src, true);
    let blank = |w: usize| Span::raw(" ".repeat(w));
    let tw_l = lw.saturating_sub(5);
    let tw_r = rw.saturating_sub(5);
    match r.kind {
        Kind::Ctx => {
            spans.push(lineno(r.old_no, cur));
            spans.push(Span::raw(" "));
            spans.extend(code_spans(&r.old, tw_l, None, None, None));
            spans.push(Span::styled("│", Style::new().fg(DIM)));
            spans.push(lineno(r.new_no, cur));
            spans.push(Span::raw(" "));
            spans.extend(code_spans(&r.new, tw_r, None, None, None));
        }
        Kind::Del => {
            spans.push(lineno(r.old_no, cur));
            spans.push(Span::raw(" "));
            spans.extend(code_spans(&r.old, tw_l, None, Some(DEL_BG), None));
            spans.push(Span::styled("│", Style::new().fg(DIM)));
            spans.push(blank(rw));
        }
        Kind::Add => {
            spans.push(blank(lw));
            spans.push(Span::styled("│", Style::new().fg(DIM)));
            spans.push(lineno(r.new_no, cur));
            spans.push(Span::raw(" "));
            spans.extend(code_spans(&r.new, tw_r, None, Some(ADD_BG), None));
        }
        Kind::Chg => {
            spans.push(lineno(r.old_no, cur));
            spans.push(Span::raw(" "));
            spans.extend(code_spans(&r.old, tw_l, r.old_span, Some(DEL_BG), Some(DEL_SPAN)));
            spans.push(Span::styled("│", Style::new().fg(DIM)));
            spans.push(lineno(r.new_no, cur));
            spans.push(Span::raw(" "));
            spans.extend(code_spans(&r.new, tw_r, r.new_span, Some(ADD_BG), Some(ADD_SPAN)));
        }
    }
    Line::from(spans)
}

fn inline_lines(app: &App, r: &Row, src: usize, width: usize) -> Vec<Line<'static>> {
    let tw = width.saturating_sub(3 + 4 + 1 + 2);
    let cur = src == app.diff_cursor;
    let mk = |first: bool, no: usize, marker: &str, text: &str, span: Option<(usize, usize)>, bg: Option<Color>, sbg: Option<Color>, mcol: Color| {
        let mut spans = gutter(app, r, src, first);
        spans.push(lineno(no, cur && first));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(format!("{marker} "), Style::new().fg(mcol)));
        spans.extend(code_spans(text, tw, span, bg, sbg));
        Line::from(spans)
    };
    match r.kind {
        Kind::Ctx => vec![mk(true, r.new_no, " ", &r.new, None, None, None, DIM)],
        Kind::Add => vec![mk(true, r.new_no, "+", &r.new, None, Some(ADD_BG), None, GREEN)],
        Kind::Del => vec![mk(true, r.old_no, "-", &r.old, None, Some(DEL_BG), None, RED)],
        Kind::Chg => vec![
            mk(true, r.old_no, "-", &r.old, r.old_span, Some(DEL_BG), Some(DEL_SPAN), RED),
            mk(false, r.new_no, "+", &r.new, r.new_span, Some(ADD_BG), Some(ADD_SPAN), GREEN),
        ],
    }
}

fn comment_box(app: &App, width: usize, header: String, body: &str, col: Color, extra: Option<String>) -> Vec<Line<'static>> {
    let _ = app;
    let inner = width.saturating_sub(10);
    let mut out = Vec::new();
    out.push(Line::from(vec![Span::raw("     "), Span::styled(format!("┌ {header}"), Style::new().fg(col))]));
    for l in wrap(body, inner) {
        out.push(Line::from(vec![Span::raw("     "), Span::styled("│ ", Style::new().fg(col)), Span::styled(l, Style::new().fg(FG))]));
    }
    if let Some(e) = extra {
        out.push(Line::from(vec![Span::raw("     "), Span::styled(format!("└ {e}"), Style::new().fg(DIM))]));
    }
    out
}

pub fn diff_lines(app: &App, fi: usize, width: usize, inline: bool) -> Vec<DLine> {
    let mut out = Vec::new();
    for v in app.rows_view(fi) {
        match v {
            RV::Elide(n) => out.push(DLine { src: None, line: Line::styled(format!("      ⋯ {n} unchanged lines"), Style::new().fg(DIM).add_modifier(Modifier::DIM)) }),
            RV::Row(i, r) => {
                if inline {
                    for (k, l) in inline_lines(app, &r, i, width).into_iter().enumerate() {
                        out.push(DLine { src: if k == 0 { Some(i) } else { None }, line: l });
                    }
                } else {
                    out.push(DLine { src: Some(i), line: side_line(app, &r, i, width) });
                }
                for c in app.comments.iter().filter(|c| c.file == fi && c.b == i) {
                    let (state, col) = match c.state {
                        CState::Open => ("open", YELLOW),
                        CState::Sent => ("sent", DIM),
                    };
                    let lines = if c.a == c.b { String::new() } else { format!(" · lines {}-{}", app.files[fi].rows[c.a].new_no, app.files[fi].rows[c.b].new_no) };
                    for l in comment_box(app, width, format!("Comment · {state} · c{}{lines}", c.commit + 1), &c.text, col, None) {
                        out.push(DLine { src: None, line: l });
                    }
                }
                if let Some(ed) = &app.editor {
                    if ed.file == fi && ed.b == i {
                        let body = format!("{}{}", ed.text, if ed.insert { "█" } else { "" });
                        let body = if body.is_empty() { "(empty)".to_string() } else { body };
                        let hint = if ed.insert { "Esc: back to COMMENT mode".to_string() } else { "i insert · Enter save · Esc cancel".to_string() };
                        for l in comment_box(app, width, format!("new Comment · {}", if ed.insert { "INSERT" } else { "COMMENT" }), &body, Color::Indexed(215), Some(hint)) {
                            out.push(DLine { src: None, line: l });
                        }
                    }
                }
            }
        }
    }
    out
}

/// Draw the current file's diff into `area`, wrapped in `block`.
pub fn draw_diff(f: &mut Frame, area: Rect, app: &mut App, block: Block<'static>) {
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.view_h = inner.height as usize;
    let Some(fi) = app.current_file() else {
        let msg = vec![
            Line::raw(""),
            Line::styled("  ✓ Nothing to review in this Scope / filter.", Style::new().fg(GREEN)),
            Line::styled("  u: show reviewed too    a / gc: change Scope", Style::new().fg(DIM)),
        ];
        f.render_widget(Paragraph::new(msg), inner);
        return;
    };
    let w = inner.width as usize;
    // auto: narrow panes go inline, and so does a diff with nothing on the old side
    // (a created file would waste half the width in side-by-side)
    let only_added = app.rows_view(fi).iter().all(|v| match v {
        RV::Row(_, r) => r.kind == Kind::Add,
        RV::Elide(_) => true,
    });
    let inline = match app.dlayout {
        DiffLayout::Side => false,
        DiffLayout::Inline => true,
        DiffLayout::Auto => w < 100 || only_added,
    };
    let lines = diff_lines(app, fi, w, inline);
    let h = inner.height as usize;
    let cur = lines.iter().position(|d| d.src == Some(app.diff_cursor)).unwrap_or(0);
    if cur < app.diff_scroll {
        app.diff_scroll = cur;
    } else if cur >= app.diff_scroll + h {
        app.diff_scroll = cur + 1 - h;
    }
    app.diff_scroll = app.diff_scroll.min(lines.len().saturating_sub(h));
    let vis: Vec<Line<'static>> = lines.into_iter().skip(app.diff_scroll).take(h).map(|d| d.line).collect();
    f.render_widget(Paragraph::new(vis), inner);
}

pub fn file_title(app: &App, fi: usize) -> String {
    let f = &app.files[fi];
    format!("{}  ·  {}. {}", f.path, f.group + 1, app.groups[f.group].name)
}

// ---------------------------------------------------------------- overlays

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    let [_, mid, _] = Layout::vertical([Constraint::Min(0), Constraint::Length(h), Constraint::Min(0)]).areas(area);
    let [_, r, _] = Layout::horizontal([Constraint::Min(0), Constraint::Length(w), Constraint::Min(0)]).areas(mid);
    r
}

fn overlay_box(f: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>, scroll: usize, w: u16, h: u16) {
    let h = (lines.len() as u16 + 2).min(h);
    let r = centered(area, w, h);
    f.render_widget(Clear, r);
    let block = Block::bordered().title(format!(" {title} ")).border_style(Style::new().fg(ACCENT));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let max = lines.len().saturating_sub(inner.height as usize);
    let sc = scroll.min(max);
    let vis: Vec<Line<'static>> = lines.into_iter().skip(sc).take(inner.height as usize).collect();
    f.render_widget(Paragraph::new(vis), inner);
}

pub fn draw_overlay(f: &mut Frame, area: Rect, app: &mut App) {
    let Some(o) = app.overlay else { return };
    let w = area.width.saturating_sub(4).min(110);
    let h = area.height.saturating_sub(2);
    let iw = w.saturating_sub(4) as usize;
    match o {
        Overlay::Help => overlay_box(f, area, "Help · Esc/q closes · j/k scrolls", help_lines(), app.overlay_scroll, w, h),
        Overlay::Summary => overlay_box(f, area, "Summary · Analyst (ru) · Esc/q closes", summary_lines(app, iw), app.overlay_scroll, w, h),
        Overlay::Diagram => overlay_box(f, area, "Diagram · sequence (Mermaid → Unicode, hardcoded) · Esc/q closes", diagram_lines(), app.overlay_scroll, w, h),
        Overlay::Prompt => {
            let (title, text) = match &app.last_prompt {
                Some(p) => (format!("Prompt #{} · last copied", app.prompt_no), p.clone()),
                None => ("Prompt · preview of open Comments".to_string(), app.build_prompt(&app.open_comment_idxs())),
            };
            let lines: Vec<Line<'static>> = text.lines().flat_map(|l| wrap(l, iw)).map(|l| Line::styled(l, Style::new().fg(FG))).collect();
            overlay_box(f, area, &title, lines, app.overlay_scroll, w, h);
        }
        Overlay::Commits => {
            let pw = area.width.saturating_sub(4).min(64);
            let (lines, cl) = commit_lines(app, pw.saturating_sub(4) as usize, true);
            let ph = (lines.len() as u16 + 4).min(area.height.saturating_sub(2));
            let sc = scroll_for(lines.len(), ph.saturating_sub(4) as usize, cl);
            let mut all = lines;
            all.push(Line::raw(""));
            all.push(Line::styled("j/k scope=commit  v range  a whole Range  Enter ok  Esc cancel", Style::new().fg(DIM)));
            overlay_box(f, area, &format!("Commits · Scope: {}", app.scope_label()), all, sc, pw, ph);
        }
        Overlay::Files => {
            let pw = area.width.saturating_sub(4).min(64);
            let (lines, cl) = file_lines(app, pw.saturating_sub(4) as usize, app.overlay_cursor, true);
            let ph = (lines.len() as u16 + 4).min(area.height.saturating_sub(2));
            let sc = scroll_for(lines.len(), ph.saturating_sub(4) as usize, cl);
            let mut all = lines;
            all.push(Line::raw(""));
            all.push(Line::styled("j/k move  Enter open  Esc close", Style::new().fg(DIM)));
            let title = if app.variant == Variant::C { "Groups (reading order)" } else { "Groups / files" };
            overlay_box(f, area, title, all, sc, pw, ph);
        }
    }
}

fn help_lines() -> Vec<Line<'static>> {
    let rows: &[(&str, &str)] = &[
        ("MODES", "NORMAL · VISUAL (v) · COMMENT/INSERT (c, i) · COMMAND (:) · SEARCH (/)"),
        ("j k / ↓ ↑", "move (diff lines, files or commits depending on the focused pane)"),
        ("Ctrl-d Ctrl-u", "half page"),
        ("gg  G", "top / bottom"),
        ("]h  [h", "next / previous hunk"),
        ("]f  [f", "next / previous file in the list (queue in variant C)"),
        ("]c  [c", "next / previous commit: narrows the Scope to one commit"),
        ("/  n  N", "search in this file, next / previous  (variant C: n/p walk the queue, use Ctrl-n/Ctrl-p)"),
        ("v + motion", "commits: build a contiguous Scope; diff: select lines for a Comment"),
        ("a", "whole Range as Scope (commit list focus or picker)  [also :scope all]"),
        ("m", "toggle Reviewed for this file in the current Scope"),
        ("u", "toggle the unreviewed-only filter (shows just the delta for re-changed files)"),
        ("s", "cycle diff layout: auto → side-by-side → inline"),
        ("c", "comment on the current line / visual range  (i type, Esc, Enter saves, Esc cancels)"),
        ("Y", "copy Prompt from open Comments (OSC 52 in the real tool), close the Round"),
        ("S", "Summary overlay"),
        ("Ctrl-w h/j/k/l, Tab", "variant A: move between Commits / Files / Diff panes  (also h / l)"),
        ("gc  gf", "A: focus Commits / Files pane.  B, C: open the picker overlay"),
        ("?", "this help"),
        (":q", "quit        :variant a|b|c  or F1 F2 F3"),
        (":diagram", "sequence diagram overlay    :prompt show  last Prompt    :prompt save  :reload  :analyze  :reset"),
        (":scope all|N|N-M", "set the Scope by commit numbers"),
        ("", ""),
        ("GUTTER", "▶ cursor · digit = commit that made the change (cyan Round 1, orange Round 2)"),
        ("LISTS", "✓ reviewed  ◐ partly reviewed  · not reviewed  ✎ has Comments"),
    ];
    rows.iter()
        .map(|(k, v)| {
            Line::from(vec![Span::styled(format!("{k:<20}"), Style::new().fg(YELLOW)), Span::styled(v.to_string(), Style::new().fg(FG))])
        })
        .collect()
}

fn summary_lines(app: &App, w: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let head = |s: &str| Line::styled(s.to_string(), Style::new().fg(ACCENT).add_modifier(Modifier::BOLD));
    out.push(head("Что делает Range"));
    for l in wrap(
        "Добавляет в HTTP-клиент повтор запросов с экспоненциальной задержкой и jitter. Политика (RetryPolicy) решает, какие ошибки повторять и сколько раз; Client::send теперь оборачивает send_once в retry::run. Во втором раунде добавлены верхняя граница задержки и учёт заголовка Retry-After на 429.",
        w,
    ) {
        out.push(Line::raw(l));
    }
    out.push(Line::raw(""));
    out.push(head("Читать внимательно"));
    for l in wrap("src/retry/backoff.rs: арифметика задержек и верхняя граница. src/client.rs: какие статусы повторяем (в Prompt №1 уже был вопрос про 4xx).", w) {
        out.push(Line::raw(l));
    }
    out.push(Line::raw(""));
    out.push(head("Группы в порядке чтения"));
    for (g, grp) in app.groups.iter().enumerate() {
        let (d, t) = app.group_progress(g);
        out.push(Line::from(vec![
            Span::styled(format!("{}. {}", g + 1, grp.name), Style::new().add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {d}/{t}{}", if grp.mechanical { "  механическое" } else { "" }), Style::new().fg(DIM)),
        ]));
        out.push(Line::styled(format!("   {}", grp.note), Style::new().fg(DIM)));
    }
    out.push(Line::raw(""));
    out.push(head("Замечания"));
    for l in wrap("Тест gives_up_after_max_attempts ждёт 5 обращений, а во втором раунде max_attempts снижен до 4; снапшот retry_policy.snap тоже ещё содержит 5.", w) {
        out.push(Line::raw(l));
    }
    out
}

fn diagram_lines() -> Vec<Line<'static>> {
    let names = ["Client", "retry::run", "RetryPolicy", "Server"];
    let msgs: &[(usize, usize, &str, bool)] = &[
        (0, 1, "send(req)", false),
        (1, 3, "attempt 0", false),
        (3, 1, "503", true),
        (1, 2, "should_retry(e, 0)", false),
        (2, 1, "true", true),
        (1, 2, "delay(0)", false),
        (2, 1, "~200ms", true),
        (1, 3, "attempt 1", false),
        (3, 1, "429  Retry-After: 2", true),
        (1, 3, "attempt 2", false),
        (3, 1, "200", true),
        (1, 0, "Response", true),
    ];
    let step = 24usize;
    let cx: Vec<usize> = (0..names.len()).map(|i| 8 + i * step).collect();
    let width = cx[cx.len() - 1] + 12;
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut top = vec![' '; width];
    let mut mid = vec![' '; width];
    let mut bot = vec![' '; width];
    for (i, n) in names.iter().enumerate() {
        let bw = n.chars().count() + 2;
        let x0 = cx[i] - bw / 2;
        top[x0] = '┌';
        mid[x0] = '│';
        bot[x0] = '└';
        for k in 1..bw - 1 {
            top[x0 + k] = '─';
            bot[x0 + k] = '─';
        }
        top[x0 + bw - 1] = '┐';
        mid[x0 + bw - 1] = '│';
        bot[x0 + bw - 1] = '┘';
        for (k, ch) in n.chars().enumerate() {
            mid[x0 + 1 + k] = ch;
        }
        bot[cx[i]] = '┬';
    }
    for row in [top, mid, bot] {
        out.push(Line::raw(row.into_iter().collect::<String>()));
    }
    let lifelines = |buf: &mut Vec<char>| {
        for x in &cx {
            buf[*x] = '│';
        }
    };
    for (from, to, label, dashed) in msgs {
        let mut lab = vec![' '; width];
        lifelines(&mut lab);
        let (a, b) = (cx[*from].min(cx[*to]), cx[*from].max(cx[*to]));
        for (k, ch) in label.chars().enumerate() {
            if a + 2 + k < b {
                lab[a + 2 + k] = ch;
            }
        }
        out.push(Line::raw(lab.into_iter().collect::<String>()));
        let mut arrow = vec![' '; width];
        lifelines(&mut arrow);
        for x in a + 1..b {
            arrow[x] = if *dashed { '┈' } else { '─' };
        }
        if cx[*from] < cx[*to] {
            arrow[cx[*from]] = '├';
            arrow[cx[*to] - 1] = '►';
        } else {
            arrow[cx[*from]] = '┤';
            arrow[cx[*to] + 1] = '◄';
        }
        out.push(Line::raw(arrow.into_iter().collect::<String>()));
    }
    let mut end = vec![' '; width];
    lifelines(&mut end);
    out.push(Line::raw(end.into_iter().collect::<String>()));
    out.push(Line::raw(""));
    out.push(Line::styled("(prototype: hardcoded messages, drawn by a 40-line function; the real tool renders Mermaid text with a library)", Style::new().fg(DIM)));
    out
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let [bar, main, status, cmd] = Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)]).areas(area);
    proto_bar(f, bar, app);
    match app.variant {
        Variant::A => crate::va::draw(f, main, app),
        Variant::B => crate::vb::draw(f, main, app),
        Variant::C => crate::vc::draw(f, main, app),
    }
    status_line(f, status, app);
    cmd_line(f, cmd, app);
    draw_overlay(f, main, app);
}

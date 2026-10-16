// PROTOTYPE 4 rendering: ONE adaptive layout (sidebar + diff, or strip + full-width diff),
// the diff renderer (which also records mouse geometry), pickers, overlays, which-key,
// hint bar. Everything is fake-data; nothing is written anywhere.

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
/// Mouse / VISUAL-copy selection highlight (dump --sel-cells looks for this background).
pub const SEL_BG: Color = Color::Indexed(25);
pub const SEL_FG: Color = Color::Indexed(231);

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

/// Render `chars[skip..]` into exactly `w` cells: syntax colours, optional line background,
/// changed-token background and the selection highlight. `span` and `sel` are in absolute
/// char coordinates of the whole text.
pub fn code_spans(chars: &[char], skip: usize, w: usize, span: Option<(usize, usize)>, base_bg: Option<Color>, span_bg: Option<Color>, sel: Option<(usize, usize)>) -> Vec<Span<'static>> {
    let fg = fg_colors(chars);
    let avail = chars.len().saturating_sub(skip);
    let cut = avail > w;
    let shown = if cut { w.saturating_sub(1) } else { avail };
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut cur = String::new();
    let mut cur_style: Option<Style> = None;
    for i in 0..w {
        let (ch, fgc, ci) = if i < shown {
            (chars[skip + i], fg[skip + i], Some(skip + i))
        } else if i == shown && cut {
            ('…', DIM, None)
        } else {
            (' ', FG, None)
        };
        let in_span = ci.map_or(false, |c| span.map_or(false, |(a, b)| c >= a && c < b));
        let in_sel = ci.map_or(false, |c| sel.map_or(false, |(a, b)| c >= a && c < b));
        let bg = if in_sel {
            Some(SEL_BG)
        } else if in_span {
            span_bg
        } else {
            base_bg
        };
        let mut st = Style::new().fg(if in_sel { SEL_FG } else { fgc });
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

fn plain(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Give chars [from, to) of a styled line the selection highlight.
fn hl_line(line: Line<'static>, from: usize, to: usize) -> Line<'static> {
    let style = line.style;
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut idx = 0;
    for sp in line.spans {
        let chars: Vec<char> = sp.content.chars().collect();
        let (s, e) = (idx, idx + chars.len());
        let (a, b) = (from.max(s), to.min(e));
        if a >= b {
            out.push(sp);
        } else {
            if a > s {
                out.push(Span::styled(chars[..a - s].iter().collect::<String>(), sp.style));
            }
            out.push(Span::styled(chars[a - s..b - s].iter().collect::<String>(), sp.style.bg(SEL_BG).fg(SEL_FG)));
            if b < e {
                out.push(Span::styled(chars[b - s..].iter().collect::<String>(), sp.style));
            }
        }
        idx = e;
    }
    Line::from(out).style(style)
}

fn line_sel(osel: Option<OSel>, li: usize, len: usize) -> Option<(usize, usize)> {
    let ((l1, c1), (l2, c2)) = osel?;
    let ((lo_l, lo_c), (hi_l, hi_c)) = if (l1, c1) <= (l2, c2) { ((l1, c1), (l2, c2)) } else { ((l2, c2), (l1, c1)) };
    if li < lo_l || li > hi_l {
        return None;
    }
    let from = if li == lo_l { lo_c } else { 0 };
    let to = if li == hi_l { hi_c + 1 } else { len };
    let (from, to) = (from.min(len), to.min(len));
    if from < to {
        Some((from, to))
    } else {
        None
    }
}

// ---------------------------------------------------------------- chrome

pub fn proto_bar(f: &mut Frame, area: Rect, app: &App) {
    let st = Style::new().fg(Color::Indexed(231)).bg(Color::Indexed(53));
    let sidebar = match (app.sidebar_manual, app.sidebar_visible()) {
        (None, v) => format!("sidebar auto:{}", if v { "shown" } else { "hidden" }),
        (Some(_), v) => format!("sidebar manual:{}", if v { "shown" } else { "hidden" }),
    };
    let line = Line::from(vec![
        Span::styled(" PROTOTYPE 4 ", st.add_modifier(Modifier::BOLD).bg(Color::Indexed(161))),
        Span::styled(format!(" adaptive layout · {sidebar} · mouse {} · Space = leader · throwaway: fake data ", if app.mouse_on { "on" } else { "off" }), st),
    ]);
    let w = area.width as usize;
    let used: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
    let mut spans = line.spans;
    if used < w {
        spans.push(Span::styled(" ".repeat(w - used), st));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub fn progress_bar(done: usize, total: usize, w: usize) -> String {
    let filled = if total == 0 { 0 } else { done * w / total };
    format!("{}{}", "█".repeat(filled), "░".repeat(w - filled))
}

pub fn scope_short(app: &App) -> String {
    let (lo, hi) = app.scope;
    if app.scope_is_whole() {
        "whole Range".into()
    } else if lo == hi {
        format!("c{} {}", lo + 1, app.commits[lo].sha)
    } else {
        format!("c{}..c{}", lo + 1, hi + 1)
    }
}

/// Draws the status line; returns the rect of the clickable `Range ✎N` counter, if any.
pub fn status_line(f: &mut Frame, area: Rect, app: &App) -> Option<Rect> {
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
        Span::styled(app.count.map_or(String::new(), |c| format!(" {c}")), bar.fg(YELLOW).add_modifier(Modifier::BOLD)),
    ];
    // The same status line in both layouts: Scope, progress, filter, Round and the counters.
    left.push(Span::styled(format!(" Scope {}", scope_short(app)), bar));
    left.push(Span::styled(format!("  {} {done}/{total}", progress_bar(done, total, 8)), bar.fg(GREEN)));
    if app.unreviewed_only {
        left.push(Span::styled("  [unreviewed]", bar.fg(YELLOW)));
    }
    let empty_round = !app.commits.iter().any(|c| c.round == app.round);
    left.push(Span::styled(format!("  Round {}{}", app.round, if empty_round { " (ждём коммиты)" } else { "" }), bar.fg(round_color(app.round))));
    let hidden = app.hidden_comments();
    if hidden > 0 {
        left.push(Span::styled(format!("  скрыто Comments: {hidden}"), bar.fg(YELLOW)));
    }
    // Range-level Comments have no code row: the counter is clickable and opens the commit picker (Q49)
    let mut chip = None;
    let rc = app.range_comment_count();
    if rc > 0 {
        let before: usize = left.iter().map(|s| s.content.chars().count()).sum();
        let text = format!("Range ✎{rc}");
        chip = Some(Rect { x: area.x + (before + 2) as u16, y: area.y, width: text.chars().count() as u16, height: 1 });
        left.push(Span::styled(format!("  {text}"), bar.fg(YELLOW)));
    }
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
    chip
}

pub fn cmd_line(f: &mut Frame, area: Rect, app: &App) {
    let line = match app.mode {
        Mode::Command => Line::from(vec![Span::raw(":"), Span::raw(app.cmd.clone()), Span::styled("█", Style::new().fg(ACCENT))]),
        Mode::Search => Line::from(vec![Span::raw("/"), Span::raw(app.search_input.clone()), Span::styled("█", Style::new().fg(ACCENT))]),
        _ => {
            if let Some(t) = &app.toast {
                Line::styled(t.clone(), Style::new().fg(YELLOW))
            } else if let Some(ch) = app.chord {
                let name = match ch {
                    Chord::G => "g",
                    Chord::Next => "]",
                    Chord::Prev => "[",
                    Chord::CtrlW => "Ctrl-w",
                    Chord::Leader => "Space",
                    Chord::LeaderPrompt => "Space p",
                };
                Line::styled(format!("{name}…   (подсказка через 0.3 с, Esc отменяет)"), Style::new().fg(DIM))
            } else if !app.search_hits.is_empty() && app.mode == Mode::Normal {
                Line::styled(format!("/{}  [{}/{}]   n/N next/prev   Esc clears", app.search, app.search_idx + 1, app.search_hits.len()), Style::new().fg(DIM))
            } else {
                Line::styled(hint(app, area.width as usize), Style::new().fg(DIM))
            }
        }
    };
    f.render_widget(Paragraph::new(line), area);
}

/// Join as many hint items as fit the width (never cut mid-item).
fn fit(items: &[&str], w: usize) -> String {
    let mut s = String::new();
    for it in items {
        let add = it.chars().count() + if s.is_empty() { 0 } else { 3 };
        if s.chars().count() + add > w {
            break;
        }
        if !s.is_empty() {
            s.push_str(" · ");
        }
        s.push_str(it);
    }
    s
}

/// Mode- and pane-dependent key hints (bottom line), most important first.
fn hint(app: &App, w: usize) -> String {
    let panes = if app.sidebar_visible() { "Ctrl-w + h/j/k/l: окна" } else { "Space c / f / e: коммиты / файлы / сайдбар" };
    match app.mode {
        Mode::Visual => {
            if app.commit_anchor.is_some() {
                fit(&["VISUAL коммиты: j/k расширить", "Enter применить Scope", "Esc отмена"], w)
            } else {
                fit(&["VISUAL: j/k расширить", "c комментарий на выделение", "y копировать код", "Space y код + Comments", "Esc отмена"], w)
            }
        }
        Mode::Comment => match app.editor.as_ref() {
            Some(e) if e.confirm => fit(&["Discard? y — удалить Comment", "n / Esc — вернуться"], w),
            Some(e) if e.insert => fit(&["INSERT: Esc → назад в normal", "Enter — новая строка"], w),
            _ => fit(&["COMMENT: i — печатать", "Enter — сохранить", "Esc — отмена"], w),
        },
        Mode::Command => fit(&["Enter выполнить", "Esc отмена", ":scope all", ":comment", ":set nomouse", ":layout auto|side|inline", ":q"], w),
        Mode::Search => fit(&["Enter искать", "Esc отмена", "потом n / N"], w),
        Mode::Normal => match app.focus {
            Pane::Diff => fit(&["j/k строки", "v выделить", "c Comment", "m просмотрено", "Space меню", panes, "h/l вбок", "]f/[f файл", "]u/[u непросмотренное", "C на файл", "? справка"], w),
            Pane::Files => fit(&["j/k файлы", "l/h раскрыть-свернуть Группу", "Enter открыть diff", "m просмотрено", "Space меню", panes, "M и дальше", "C Comment на файл", "? справка"], w),
            Pane::Commits => fit(&["j/k курсор", "v диапазон", "Enter применить Scope", "a весь Range", "Space меню", panes, "? справка"], w),
        },
    }
}

// ---------------------------------------------------------------- which-key

fn chord_entries(c: Chord) -> (&'static str, Vec<(&'static str, &'static str)>) {
    match c {
        Chord::Leader => (
            "Space — leader",
            vec![
                ("f", "файлы: picker Groups/files"),
                ("c", "коммиты: picker и Scope"),
                ("e", "сайдбар: показать / скрыть"),
                ("s", "Summary Аналитика"),
                ("u", "фильтр: только непросмотренное"),
                ("d", "диаграмма"),
                ("p", "+prompt: копировать Comments как Prompt…"),
                ("y", "копировать выделение вместе с Comments"),
            ],
        ),
        Chord::LeaderPrompt => (
            "Space p — Prompt",
            vec![
                ("p", "все open Comments (то же, что Y)"),
                ("f", "Comments текущего файла"),
                ("c", "Comment под курсором"),
                ("s", "показать Prompt (overlay)"),
            ],
        ),
        Chord::G => (
            "g",
            vec![("g", "в начало"), ("d", "к определению — позже (LSP)"), ("r", "ссылки — позже (LSP)")],
        ),
        Chord::Next => (
            "]",
            vec![("h", "следующий хунк"), ("f", "следующий файл"), ("u", "следующий непросмотренный файл"), ("c", "следующий коммит (Scope = он)")],
        ),
        Chord::Prev => (
            "[",
            vec![("h", "предыдущий хунк"), ("f", "предыдущий файл"), ("u", "предыдущий непросмотренный файл"), ("c", "предыдущий коммит (Scope = он)")],
        ),
        Chord::CtrlW => (
            "Ctrl-w — окна",
            vec![("h", "окно слева (сайдбар)"), ("j", "окно ниже"), ("k", "окно выше"), ("l", "окно справа (diff)"), ("w", "следующее окно")],
        ),
    }
}

fn draw_whichkey(f: &mut Frame, area: Rect, app: &App) {
    let Some(ch) = app.chord else { return };
    if !app.whichkey {
        return;
    }
    let (title, entries) = chord_entries(ch);
    let inner_w = entries.iter().map(|(k, d)| 4 + k.chars().count() + d.chars().count()).max().unwrap_or(10).max(title.chars().count() + 2);
    let w = ((inner_w + 4) as u16).min(area.width);
    let h = ((entries.len() + 2) as u16).min(area.height);
    let r = Rect { x: area.x + area.width.saturating_sub(w), y: area.y + area.height.saturating_sub(h), width: w, height: h };
    f.render_widget(Clear, r);
    let block = Block::bordered().title(format!(" {title} ")).border_style(Style::new().fg(ACCENT));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let lines: Vec<Line<'static>> = entries
        .iter()
        .map(|(k, d)| Line::from(vec![Span::styled(format!(" {k}  "), Style::new().fg(YELLOW).add_modifier(Modifier::BOLD)), Span::styled(d.to_string(), Style::new().fg(FG))]))
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

// ---------------------------------------------------------------- lists (sidebar panes and pickers)

pub fn status_glyph(s: Status) -> Span<'static> {
    match s {
        Status::Done => Span::styled("✓", Style::new().fg(GREEN)),
        Status::Partial => Span::styled("◐", Style::new().fg(YELLOW)),
        Status::Todo => Span::styled("·", Style::new().fg(DIM)),
    }
}

fn round_title(app: &App, r: u8) -> String {
    let after = &app.round_after[(r - 1) as usize];
    match after.len() {
        0 => format!("Round {r}"),
        1 => format!("Round {r}  (after Prompt #{})", after[0]),
        _ => format!("Round {r}  (after Prompts {})", after.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ")),
    }
}

/// Commit list lines with Round separators (Rounds without commits get no header: they merge
/// into the next one). Returns (lines, line index of the cursor commit, line → commit index).
pub fn commit_lines(app: &App, width: usize, show_cursor: bool) -> (Vec<Line<'static>>, usize, Vec<Option<usize>>) {
    let mut lines = Vec::new();
    let mut map = Vec::new();
    let mut cursor_line = 0;
    let (vlo, vhi) = app.commit_selection();

    // pinned "▣ Весь Range" row (Q49): selecting it applies the whole-Range Scope, and the
    // Range-level Comments hang under it the way line Comments hang under code
    let rc: Vec<&Comment> = app.comments.iter().filter(|c| c.kind == CKind::Range).collect();
    let whole = app.scope_is_whole();
    let range_cursor = show_cursor && app.on_range;
    let marker = if range_cursor {
        Span::styled("▶", Style::new().fg(YELLOW))
    } else if whole {
        Span::styled("▌", Style::new().fg(ACCENT))
    } else {
        Span::raw(" ")
    };
    let mut label = " ▣ Весь Range".to_string();
    if !rc.is_empty() {
        label.push_str(&format!("  ✎{}", rc.len()));
    }
    let label_style = if whole { Style::new().fg(FG).add_modifier(Modifier::BOLD) } else { Style::new().fg(DIM) };
    lines.push(Line::from(vec![marker, Span::styled(trunc(&label, width.saturating_sub(1)), label_style)]));
    map.push(Some(crate::app::RANGE_ROW));
    for c in &rc {
        let (tag, col) = if c.state == CState::Open { ("open", YELLOW) } else { ("sent", DIM) };
        lines.push(Line::styled(format!("   ┌ Comment · {tag}"), Style::new().fg(col)));
        map.push(None);
        let wrapped = wrap(&c.text, width.saturating_sub(6).max(8));
        let shown = wrapped.len().min(2);
        for (k, l) in wrapped.iter().take(shown).enumerate() {
            let more = k + 1 == shown && wrapped.len() > shown;
            let text = if more { format!("{}…", trunc(l, width.saturating_sub(7).max(7))) } else { l.clone() };
            lines.push(Line::styled(format!("   │ {text}"), Style::new().fg(col)));
            map.push(None);
        }
        lines.push(Line::styled("   └", Style::new().fg(col)));
        map.push(None);
    }

    for r in 1..=app.round {
        if !app.commits.iter().any(|c| c.round == r) {
            continue;
        }
        lines.push(Line::styled(format!("── {} ──", round_title(app, r)), Style::new().fg(round_color(r)).add_modifier(Modifier::DIM)));
        map.push(None);
        for (i, c) in app.commits.iter().enumerate() {
            if c.round != r {
                continue;
            }
            let in_scope = app.in_scope(i);
            let partial = !app.scope_is_whole();
            let is_cursor = show_cursor && !app.on_range && i == app.commit_cursor;
            if !app.on_range && i == app.commit_cursor {
                cursor_line = lines.len();
            }
            let in_visual = show_cursor && app.commit_anchor.is_some() && i >= vlo && i <= vhi;
            let marker = if is_cursor {
                Span::styled("▶", Style::new().fg(YELLOW))
            } else if in_visual {
                Span::styled("┃", Style::new().fg(YELLOW))
            } else if partial && in_scope {
                Span::styled("▌", Style::new().fg(ACCENT))
            } else {
                Span::raw(" ")
            };
            let text = trunc(&format!(" c{} {} {}", i + 1, c.sha, c.msg), width.saturating_sub(1));
            let style = if partial && !in_scope { Style::new().fg(DIM) } else { Style::new().fg(FG) };
            lines.push(Line::from(vec![marker, Span::styled(text, style)]));
            map.push(Some(i));
        }
    }
    (lines, cursor_line, map)
}

/// " Commits 50/100 " (the commit under the cursor), or " Commits 100 " while the cursor is on the Range row.
pub fn commits_title(app: &App) -> String {
    let n = app.commits.len();
    if app.on_range {
        format!(" Commits {n} ")
    } else {
        format!(" Commits {}/{n} ", app.commit_cursor + 1)
    }
}

/// One line per item (Group header or file). `cursor` is an index into `items`.
pub fn item_lines(app: &App, items: &[ListItem], width: usize, cursor: usize, highlight: bool, honor_folds: bool) -> (Vec<Line<'static>>, usize) {
    let vf = app.visible_files();
    let mut lines = Vec::new();
    let mut cursor_line = 0;
    for (ix, it) in items.iter().enumerate() {
        let mut line = match *it {
            ListItem::Group(g) => {
                let gr = &app.groups[g];
                let (d, t) = app.group_progress(g);
                let glyph = if honor_folds && app.folded.contains(&g) { "▸" } else { "▾" };
                let head = format!("{glyph} {}. {}{}", g + 1, gr.name, if gr.mechanical { "  [mech]" } else { "" });
                let tail = format!(" {d}/{t}");
                let pad = width.saturating_sub(head.chars().count() + tail.chars().count());
                Line::from(vec![
                    Span::styled(trunc(&head, width.saturating_sub(tail.chars().count())), Style::new().add_modifier(Modifier::BOLD).fg(Color::Indexed(250))),
                    Span::raw(" ".repeat(pad)),
                    Span::styled(tail, Style::new().fg(DIM)),
                ])
            }
            ListItem::File(pos) => {
                let fi = vf[pos];
                let f = &app.files[fi];
                let mut name = f.path.to_string();
                let room = width.saturating_sub(7);
                if name.chars().count() > room {
                    let skip = name.chars().count() - room + 1;
                    name = format!("…{}", name.chars().skip(skip).collect::<String>());
                }
                let cm = app.comment_count(fi);
                let mut spans = vec![Span::raw("  "), status_glyph(app.file_status(fi)), Span::raw(" "), Span::raw(name)];
                if cm > 0 {
                    spans.push(Span::styled(" ✎", Style::new().fg(YELLOW)));
                }
                Line::from(spans)
            }
        };
        if ix == cursor {
            cursor_line = lines.len();
            line = if highlight { line.style(Style::new().add_modifier(Modifier::REVERSED)) } else { line.style(Style::new().bg(Color::Indexed(238))) };
        }
        lines.push(line);
    }
    if items.is_empty() {
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
    pub sub: u8,
    pub segs: Vec<Seg>,
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

/// Mouse selection highlight for display line (src, sub) in column `col`.
fn sel_range(app: &App, src: usize, sub: u8, col: u8, len: usize) -> Option<(usize, usize)> {
    let s = app.dsel?;
    if s.col != col {
        return None;
    }
    let (lo, hi) = s.norm();
    let key = (src, sub);
    if key < (lo.src, lo.sub) || key > (hi.src, hi.sub) {
        return None;
    }
    let from = if key == (lo.src, lo.sub) { lo.ch } else { 0 };
    let to = if key == (hi.src, hi.sub) { hi.ch + 1 } else { len };
    let (from, to) = (from.min(len), to.min(len));
    if from < to {
        Some((from, to))
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn push_code(app: &App, spans: &mut Vec<Span<'static>>, segs: &mut Vec<Seg>, col: u8, x0: usize, tw: usize, text: &[char], src: usize, sub: u8, mark: char, span: Option<(usize, usize)>, bg: Option<Color>, sbg: Option<Color>) {
    let sel = sel_range(app, src, sub, col, text.len());
    spans.extend(code_spans(text, app.hscroll, tw, span, bg, sbg, sel));
    if tw > 0 {
        segs.push(Seg { col, x0: x0 as u16, w: tw as u16, text: text.to_vec(), skip: app.hscroll, mark });
    }
}

fn side_line(app: &App, r: &Row, src: usize, width: usize) -> (Line<'static>, Vec<Seg>) {
    let avail = width.saturating_sub(4);
    let lw = avail / 2;
    let rw = avail - lw;
    let cur = src == app.diff_cursor;
    let mut spans = gutter(app, r, src, true);
    let mut segs = Vec::new();
    let tw_l = lw.saturating_sub(5);
    let tw_r = rw.saturating_sub(5);
    let x_l = 8usize;
    let x_r = 3 + lw + 1 + 5;
    let old: Vec<char> = r.old.chars().collect();
    let new: Vec<char> = r.new.chars().collect();
    let sep = || Span::styled("│", Style::new().fg(DIM));
    match r.kind {
        Kind::Ctx => {
            spans.push(lineno(r.old_no, cur));
            spans.push(Span::raw(" "));
            push_code(app, &mut spans, &mut segs, 0, x_l, tw_l, &old, src, 0, ' ', None, None, None);
            spans.push(sep());
            spans.push(lineno(r.new_no, cur));
            spans.push(Span::raw(" "));
            push_code(app, &mut spans, &mut segs, 1, x_r, tw_r, &new, src, 0, ' ', None, None, None);
        }
        Kind::Del => {
            spans.push(lineno(r.old_no, cur));
            spans.push(Span::raw(" "));
            push_code(app, &mut spans, &mut segs, 0, x_l, tw_l, &old, src, 0, '-', None, Some(DEL_BG), None);
            spans.push(sep());
            spans.push(Span::raw(" ".repeat(rw)));
        }
        Kind::Add => {
            spans.push(Span::raw(" ".repeat(lw)));
            spans.push(sep());
            spans.push(lineno(r.new_no, cur));
            spans.push(Span::raw(" "));
            push_code(app, &mut spans, &mut segs, 1, x_r, tw_r, &new, src, 0, '+', None, Some(ADD_BG), None);
        }
        Kind::Chg => {
            spans.push(lineno(r.old_no, cur));
            spans.push(Span::raw(" "));
            push_code(app, &mut spans, &mut segs, 0, x_l, tw_l, &old, src, 0, '-', r.old_span, Some(DEL_BG), Some(DEL_SPAN));
            spans.push(sep());
            spans.push(lineno(r.new_no, cur));
            spans.push(Span::raw(" "));
            push_code(app, &mut spans, &mut segs, 1, x_r, tw_r, &new, src, 0, '+', r.new_span, Some(ADD_BG), Some(ADD_SPAN));
        }
    }
    (Line::from(spans), segs)
}

fn inline_lines(app: &App, r: &Row, src: usize, width: usize) -> Vec<(Line<'static>, Vec<Seg>)> {
    let tw = width.saturating_sub(10);
    let cur = src == app.diff_cursor;
    #[allow(clippy::too_many_arguments)]
    let mk = |first: bool, sub: u8, no: usize, marker: &str, text: &str, span: Option<(usize, usize)>, bg: Option<Color>, sbg: Option<Color>, mcol: Color, mark: char| {
        let mut spans = gutter(app, r, src, first);
        spans.push(lineno(no, cur && first));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(format!("{marker} "), Style::new().fg(mcol)));
        let chars: Vec<char> = text.chars().collect();
        let mut segs = Vec::new();
        push_code(app, &mut spans, &mut segs, 2, 10, tw, &chars, src, sub, mark, span, bg, sbg);
        (Line::from(spans), segs)
    };
    match r.kind {
        Kind::Ctx => vec![mk(true, 0, r.new_no, " ", &r.new, None, None, None, DIM, ' ')],
        Kind::Add => vec![mk(true, 0, r.new_no, "+", &r.new, None, Some(ADD_BG), None, GREEN, '+')],
        Kind::Del => vec![mk(true, 0, r.old_no, "-", &r.old, None, Some(DEL_BG), None, RED, '-')],
        Kind::Chg => vec![
            mk(true, 0, r.old_no, "-", &r.old, r.old_span, Some(DEL_BG), Some(DEL_SPAN), RED, '-'),
            mk(false, 1, r.new_no, "+", &r.new, r.new_span, Some(ADD_BG), Some(ADD_SPAN), GREEN, '+'),
        ],
    }
}

fn comment_box(width: usize, header: String, body: &str, col: Color, extra: Option<String>) -> Vec<Line<'static>> {
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

fn editor_hint(ed: &Editor) -> String {
    if ed.confirm {
        "Discard? y — удалить · n / Esc — вернуться".into()
    } else if ed.insert {
        "INSERT · Esc → normal · Enter — новая строка".into()
    } else {
        "i печатать · Enter сохранить · Esc отмена".into()
    }
}

fn saved_comment_header(app: &App, c: &Comment) -> String {
    let state = match c.state {
        CState::Open => "open",
        CState::Sent => "sent",
    };
    let what = match c.kind {
        CKind::Line if c.a != c.b => format!(" · {}", app.anchor_text(c.kind, c.file, c.a, c.b)),
        CKind::File => " · на файл".to_string(),
        _ => String::new(),
    };
    format!("Comment · {state} · c{}{what}", c.commit + 1)
}

pub fn diff_lines(app: &App, fi: usize, width: usize, inline: bool) -> Vec<DLine> {
    let mut out: Vec<DLine> = Vec::new();
    let nometa = |line: Line<'static>| DLine { src: None, sub: 0, segs: Vec::new(), line };
    // file-level Comments sit above the first row
    for c in app.comments.iter().filter(|c| c.kind == CKind::File && c.file == fi) {
        let col = if c.state == CState::Open { YELLOW } else { DIM };
        for l in comment_box(width, saved_comment_header(app, c), &c.text, col, None) {
            out.push(nometa(l));
        }
    }
    for v in app.rows_view(fi) {
        match v {
            RV::Elide(n) => out.push(nometa(Line::styled(format!("      ⋯ {n} unchanged lines"), Style::new().fg(DIM).add_modifier(Modifier::DIM)))),
            RV::Row(i, r) => {
                if inline {
                    for (k, (l, segs)) in inline_lines(app, &r, i, width).into_iter().enumerate() {
                        out.push(DLine { src: Some(i), sub: k as u8, segs, line: l });
                    }
                } else {
                    let (l, segs) = side_line(app, &r, i, width);
                    out.push(DLine { src: Some(i), sub: 0, segs, line: l });
                }
                for c in app.comments.iter().filter(|c| c.kind == CKind::Line && c.file == fi && c.b == i) {
                    let col = if c.state == CState::Open { YELLOW } else { DIM };
                    for l in comment_box(width, saved_comment_header(app, c), &c.text, col, None) {
                        out.push(nometa(l));
                    }
                }
                if let Some(ed) = &app.editor {
                    if ed.kind == CKind::Line && ed.file == fi && ed.b == i {
                        let body = format!("{}{}", ed.text, if ed.insert { "█" } else { "" });
                        let body = if body.is_empty() { "(empty)".to_string() } else { body };
                        let header = format!("Comment на {} · {}", app.anchor_text(ed.kind, ed.file, ed.a, ed.b), if ed.insert { "INSERT" } else { "COMMENT" });
                        for l in comment_box(width, header, &body, Color::Indexed(215), Some(editor_hint(ed))) {
                            out.push(nometa(l));
                        }
                    }
                }
            }
        }
    }
    out
}

/// Draw the current file's diff into `area`, wrapped in `block`; records mouse geometry.
pub fn draw_diff(f: &mut Frame, area: Rect, app: &mut App, block: Block<'static>) {
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.view_h = inner.height as usize;
    app.hits.panes.push((area, Pane::Diff));
    let Some(fi) = app.current_file() else {
        let msg = vec![
            Line::raw(""),
            Line::styled("  ✓ Nothing to review in this Scope / filter.", Style::new().fg(GREEN)),
            Line::styled("  Space u: показать и просмотренное    Space c: сменить Scope", Style::new().fg(DIM)),
        ];
        f.render_widget(Paragraph::new(msg), inner);
        app.hits.diff = None;
        return;
    };
    let w = inner.width as usize;
    // auto: a narrow diff pane goes inline, and so does a diff with nothing on the old side
    let only_added = app.rows_view(fi).iter().all(|v| match v {
        RV::Row(_, r) => r.kind == Kind::Add,
        RV::Elide(_) => true,
    });
    let inline = match app.dlayout {
        DiffLayout::Side => false,
        DiffLayout::Inline => true,
        DiffLayout::Auto => w < 100 || only_added,
    };
    app.last_inline = inline;
    let lines = diff_lines(app, fi, w, inline);
    let h = inner.height as usize;
    // scroll is anchored to a diff row, so it survives a switch between inline / side-by-side
    let mut off = if app.scroll_fi == Some(fi) {
        lines.iter().position(|d| d.src == Some(app.scroll_src)).unwrap_or(0)
    } else {
        0
    };
    let mut cur = lines.iter().position(|d| d.src == Some(app.diff_cursor)).unwrap_or(0);
    let max_off = lines.len().saturating_sub(h);
    if app.scroll_delta != 0 {
        // Ctrl-e/y/f/b: scroll the view; the cursor moves only if it left the view (Vim)
        off = (off as isize + app.scroll_delta).clamp(0, max_off as isize) as usize;
        app.scroll_delta = 0;
        let want = cur.clamp(off, (off + h).saturating_sub(1));
        if want != cur {
            // nearest line that can hold the cursor (elisions and Comment boxes have no src)
            let pick = (want..lines.len())
                .chain((0..want).rev())
                .find(|&i| lines[i].src.is_some() && i >= off && i < off + h);
            if let Some(i) = pick {
                cur = i;
                app.diff_cursor = lines[i].src.unwrap();
            }
        }
    }
    if cur < off {
        off = cur;
    } else if cur >= off + h {
        off = cur + 1 - h;
    }
    off = off.min(max_off);
    app.scroll_fi = Some(fi);
    app.scroll_src = lines.iter().skip(off).find_map(|d| d.src).unwrap_or(app.diff_cursor);
    app.diff_scroll = off;
    let mut metas = Vec::new();
    let mut vis: Vec<Line<'static>> = Vec::new();
    for d in lines.into_iter().skip(off).take(h) {
        metas.push(LineMeta { src: d.src, sub: d.sub, segs: d.segs });
        vis.push(d.line);
    }
    f.render_widget(Paragraph::new(vis), inner);
    app.hits.diff = Some(DiffGeo { inner, fi, inline, width: w, lines: metas });
}

pub fn file_title(app: &App, fi: usize) -> String {
    let f = &app.files[fi];
    let mut t = format!("{}  ·  {}. {}", f.path, f.group + 1, app.groups[f.group].name);
    if app.hscroll > 0 {
        t.push_str(&format!("  ·  col +{}", app.hscroll));
    }
    let hid = app.hidden_in_file(fi);
    if hid > 0 {
        t.push_str(&format!("  ·  скрыто Comments: {hid}"));
    }
    t
}

// ---------------------------------------------------------------- the adaptive main area

fn border(app: &App, p: Pane) -> Style {
    if app.focus == p {
        Style::new().fg(ACCENT)
    } else {
        Style::new().fg(Color::Indexed(240))
    }
}

fn draw_with_sidebar(f: &mut Frame, area: Rect, app: &mut App) {
    let left_w = if area.width >= 150 {
        44
    } else if area.width >= 100 {
        36
    } else {
        26
    };
    let [left, right] = Layout::horizontal([Constraint::Length(left_w), Constraint::Min(10)]).areas(area);
    let want = commit_lines(app, 10, false).0.len() as u16 + 2;
    // the commit pane takes at most ~35% of the sidebar, but never fewer than 11 rows with borders
    // (a short Range with Round headers and a pinned Range row fits); files get the rest
    let commits_h = want.min((left.height * 35 / 100).max(11).min(left.height.saturating_sub(3)));
    let [cpane, fpane] = Layout::vertical([Constraint::Length(commits_h), Constraint::Min(3)]).areas(left);

    // commits
    let cb = Block::bordered().title(commits_title(app)).border_style(border(app, Pane::Commits));
    let ci = cb.inner(cpane);
    f.render_widget(cb, cpane);
    app.hits.panes.push((cpane, Pane::Commits));
    let (lines, cl, map) = commit_lines(app, ci.width as usize, app.focus == Pane::Commits);
    let h = ci.height as usize;
    app.commit_h = h;
    let sc = scroll_for(lines.len(), h, cl);
    let mut vis: Vec<Line<'static>> = lines.iter().skip(sc).take(h).cloned().collect();
    let mut vmap: Vec<Option<usize>> = map.iter().skip(sc).take(h).cloned().collect();
    if sc > 0 && h >= 6 {
        // sticky head while scrolled: the "▣ Весь Range" row, and the Round header the visible commits sit under
        vis[0] = lines[0].clone();
        vmap[0] = Some(RANGE_ROW);
        let is_header = |i: usize| map[i].is_none() && lines[i].spans.first().map_or(false, |s| s.content.starts_with("──"));
        if let Some(hi) = (0..(sc + 2).min(lines.len())).rev().find(|&i| is_header(i)) {
            vis[1] = lines[hi].clone();
            vmap[1] = None;
        }
    }
    for (k, m) in vmap.iter().enumerate() {
        if let Some(idx) = m {
            app.hits.commit_rows.push((Rect { x: ci.x, y: ci.y + k as u16, width: ci.width, height: 1 }, *idx));
        }
    }
    f.render_widget(Paragraph::new(vis), ci);

    // groups / files
    let (d, t) = app.progress();
    let fb = Block::bordered().title(format!(" Groups / files  ✓{d}/{t} ")).border_style(border(app, Pane::Files));
    let fi = fb.inner(fpane);
    f.render_widget(fb, fpane);
    app.hits.panes.push((fpane, Pane::Files));
    let items = app.list_items();
    let (lines, cl) = item_lines(app, &items, fi.width as usize, app.list_cursor, app.focus == Pane::Files, true);
    let sc = scroll_for(lines.len(), fi.height as usize, cl);
    let vis: Vec<Line<'static>> = lines.into_iter().skip(sc).take(fi.height as usize).collect();
    for k in 0..vis.len() {
        if sc + k < items.len() {
            app.hits.list_rows.push((Rect { x: fi.x, y: fi.y + k as u16, width: fi.width, height: 1 }, sc + k));
        }
    }
    f.render_widget(Paragraph::new(vis), fi);

    // diff
    let title = match app.current_file() {
        Some(i) => format!(" {} ", trunc(&file_title(app, i), right.width.saturating_sub(4) as usize)),
        None => " (no file) ".to_string(),
    };
    let db = Block::bordered().title(title).border_style(border(app, Pane::Diff));
    draw_diff(f, right, app, db);
}

fn draw_strip_layout(f: &mut Frame, area: Rect, app: &mut App) {
    // One "file bar" line: without the sidebar the diff has no pane border to name the file.
    // Scope, Round, progress and the counters are in the bottom status line, in both modes.
    let [strip, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).areas(area);
    let bg = Style::new().bg(Color::Indexed(235)).fg(FG);
    let w = area.width as usize;

    let vf = app.visible_files();
    let mut l2 = vec![Span::styled(" ", bg)];
    match app.current_file() {
        Some(fi) => {
            let fl = &app.files[fi];
            let atoms = app.atoms(fi);
            let rev = atoms.iter().filter(|c| app.reviewed.contains(&(fi, **c))).count();
            l2.push(Span::styled(fl.path.to_string(), bg.add_modifier(Modifier::BOLD)));
            l2.push(Span::styled(" ", bg));
            let mut g = status_glyph(app.file_status(fi));
            g.style = g.style.bg(Color::Indexed(235));
            l2.push(g);
            l2.push(Span::styled(format!(" {rev}/{} changes reviewed  ·  {}. {}", atoms.len(), fl.group + 1, app.groups[fl.group].name), bg.fg(DIM)));
            if app.hscroll > 0 {
                l2.push(Span::styled(format!("  ·  col +{}", app.hscroll), bg.fg(DIM)));
            }
            let hid = app.hidden_in_file(fi);
            if hid > 0 {
                l2.push(Span::styled(format!("  ·  скрыто Comments: {hid}"), bg.fg(YELLOW)));
            }
        }
        None => l2.push(Span::styled("(no file in this Scope / filter)", bg.fg(DIM))),
    }
    let tail = format!("file {}/{} ", if vf.is_empty() { 0 } else { app.file_cursor + 1 }, vf.len());
    let used: usize = l2.iter().map(|s| s.content.chars().count()).sum();
    if used + tail.chars().count() < w {
        l2.push(Span::styled(" ".repeat(w - used - tail.chars().count()), bg));
        l2.push(Span::styled(tail, bg.fg(DIM)));
    } else if used < w {
        l2.push(Span::styled(" ".repeat(w - used), bg));
    }
    f.render_widget(Paragraph::new(Line::from(l2)), strip);

    draw_diff(f, body, app, Block::new());
}

// ---------------------------------------------------------------- overlays

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

/// Draws a bordered box with `lines`; returns the box rect and the text geometry for selection.
fn overlay_box(f: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>, scroll: usize, w: u16, h: u16, osel: Option<OSel>) -> (Rect, OverlayGeo, usize) {
    let h = (lines.len() as u16 + 2).min(h);
    let r = centered(area, w, h);
    f.render_widget(Clear, r);
    let block = Block::bordered().title(format!(" {title} ")).border_style(Style::new().fg(ACCENT));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    let max = lines.len().saturating_sub(inner.height as usize);
    let sc = scroll.min(max);
    let vis: Vec<Line<'static>> = lines
        .into_iter()
        .enumerate()
        .skip(sc)
        .take(inner.height as usize)
        .map(|(li, l)| match line_sel(osel, li, texts[li].chars().count()) {
            Some((a, b)) => hl_line(l, a, b),
            None => l,
        })
        .collect();
    f.render_widget(Paragraph::new(vis), inner);
    (r, OverlayGeo { inner, scroll: sc, texts }, sc)
}

pub fn draw_overlay(f: &mut Frame, area: Rect, app: &mut App) {
    let Some(o) = app.overlay else { return };
    let w = area.width.saturating_sub(4).min(110);
    let h = area.height.saturating_sub(2);
    let iw = w.saturating_sub(4) as usize;
    let osel = app.osel;
    let scroll = app.overlay_scroll;
    match o {
        Overlay::Help | Overlay::Summary | Overlay::Diagram | Overlay::Prompt => {
            let (title, lines): (String, Vec<Line<'static>>) = match o {
                Overlay::Help => ("Help · Esc/q closes · j/k scrolls · drag selects and copies".into(), help_lines(iw)),
                Overlay::Summary => ("Summary · Analyst (ru) · Esc/q closes".into(), summary_lines(app, iw)),
                Overlay::Diagram => ("Diagram · sequence (Mermaid → Unicode, hardcoded) · Esc/q closes".into(), diagram_lines()),
                _ => {
                    let (title, text) = match &app.last_prompt {
                        Some(p) => (format!("Prompt #{} · last copied · Esc/q closes", app.prompt_no), p.clone()),
                        None => ("Prompt · preview of open Comments · Esc/q closes".to_string(), app.build_prompt(&app.open_comment_idxs())),
                    };
                    (title, text.lines().flat_map(|l| wrap(l, iw)).map(|l| Line::styled(l, Style::new().fg(FG))).collect())
                }
            };
            let (r, geo, sc) = overlay_box(f, area, &title, lines, scroll, w, h, osel);
            app.overlay_scroll = sc;
            app.hits.overlay_box = Some(r);
            app.hits.overlay = Some(geo);
        }
        Overlay::Commits => {
            let pw = area.width.saturating_sub(4).min(64);
            let (lines, cl, map) = commit_lines(app, pw.saturating_sub(4) as usize, true);
            let ph = (lines.len() as u16 + 4).min(area.height.saturating_sub(2));
            let sc = scroll_for(lines.len(), ph.saturating_sub(4) as usize, cl);
            let mut all = lines;
            all.push(Line::raw(""));
            all.push(Line::styled("j/k курсор · v диапазон · Enter применить Scope · a весь Range · Esc", Style::new().fg(DIM)));
            let n = map.len();
            let (r, _, _) = overlay_box(f, area, &format!("{}· Scope: {}", commits_title(app).trim_start(), app.scope_label()), all, sc, pw, ph, None);
            app.hits.overlay_box = Some(r);
            let inner = Rect { x: r.x + 1, y: r.y + 1, width: r.width.saturating_sub(2), height: r.height.saturating_sub(2) };
            for k in 0..inner.height as usize {
                if sc + k < n {
                    if let Some(idx) = map[sc + k] {
                        app.hits.picker_rows.push((Rect { x: inner.x, y: inner.y + k as u16, width: inner.width, height: 1 }, idx));
                    }
                }
            }
        }
        Overlay::Files => {
            let pw = area.width.saturating_sub(4).min(64);
            let items = items_for(app, false);
            let cur_ix = items.iter().position(|i| *i == ListItem::File(app.overlay_cursor)).unwrap_or(0);
            let (lines, cl) = item_lines(app, &items, pw.saturating_sub(4) as usize, cur_ix, true, false);
            let ph = (lines.len() as u16 + 4).min(area.height.saturating_sub(2));
            let sc = scroll_for(lines.len(), ph.saturating_sub(4) as usize, cl);
            let mut all = lines;
            all.push(Line::raw(""));
            all.push(Line::styled("j/k курсор · Enter открыть · Esc закрыть · клик по файлу", Style::new().fg(DIM)));
            let (r, _, _) = overlay_box(f, area, "Groups / files", all, sc, pw, ph, None);
            app.hits.overlay_box = Some(r);
            let inner = Rect { x: r.x + 1, y: r.y + 1, width: r.width.saturating_sub(2), height: r.height.saturating_sub(2) };
            for k in 0..inner.height as usize {
                if let Some(ListItem::File(pos)) = items.get(sc + k).copied() {
                    app.hits.picker_rows.push((Rect { x: inner.x, y: inner.y + k as u16, width: inner.width, height: 1 }, pos));
                }
            }
        }
    }
}

/// File- and Range-level Comments have no row to hang under: their editor is a popup.
fn draw_editor_popup(f: &mut Frame, area: Rect, app: &App) {
    let Some(ed) = &app.editor else { return };
    if ed.kind == CKind::Line {
        return;
    }
    let w = area.width.saturating_sub(4).min(72);
    let iw = w.saturating_sub(4) as usize;
    let mut title = format!("Comment на {}", app.anchor_text(ed.kind, ed.file, ed.a, ed.b));
    if ed.kind == CKind::File {
        title.push_str(&format!(" · {}", app.files[ed.file].path));
    }
    let body = format!("{}{}", ed.text, if ed.insert { "█" } else { "" });
    let mut lines: Vec<Line<'static>> = if body.is_empty() { vec![Line::styled("(empty)", Style::new().fg(DIM))] } else { wrap(&body, iw).into_iter().map(|l| Line::styled(l, Style::new().fg(FG))).collect() };
    lines.push(Line::raw(""));
    lines.push(Line::styled(editor_hint(ed), Style::new().fg(YELLOW)));
    let h = (lines.len() as u16 + 2).min(area.height);
    let r = centered(area, w, h);
    f.render_widget(Clear, r);
    let block = Block::bordered().title(format!(" {title} ")).border_style(Style::new().fg(Color::Indexed(215)));
    let inner = block.inner(r);
    f.render_widget(block, r);
    f.render_widget(Paragraph::new(lines), inner);
}

fn help_lines(iw: usize) -> Vec<Line<'static>> {
    let rows: &[(&str, &str)] = &[
        ("MODES", "NORMAL · VISUAL (v) · COMMENT/INSERT (c, i) · COMMAND (:) · SEARCH (/)"),
        ("j k / ↓ ↑", "move: diff rows, files or commits depending on the focused pane"),
        ("h l / ← →", "diff: scroll sideways (0 resets) · Files: h folds / l unfolds a Group, l on a file opens the diff"),
        ("Ctrl-d Ctrl-u", "half page (moves the cursor)"),
        ("Ctrl-f Ctrl-b", "a page down / up (scrolls the view; NOTE: zellij binds Ctrl-b to Tmux mode by default)"),
        ("Ctrl-e Ctrl-y", "scroll the view one line down / up, the cursor stays unless it leaves the view"),
        ("gg  G", "top / bottom"),
        ("gd  gr", "go to definition / references: later (LSP slice), listed here so the keys are reserved"),
        ("]h [h", "next / previous hunk"),
        ("]f [f", "next / previous file (ALL files: a reviewed one is always reachable)"),
        ("]u [u", "next / previous file that still has unreviewed changes"),
        ("]c [c", "next / previous commit: an explicit command, so Scope = that commit at once"),
        ("/  n  N", "search in this file, next / previous"),
        ("Ctrl-w h/j/k/l", "move between Commits / Files / Diff (Tab cycles)"),
        ("Ctrl-h/j/k/l", "aliases of Ctrl-w h/j/k/l. NOTE: zellij binds Ctrl-h (Move mode) by default, so Ctrl-w is the primary"),
        ("v + motion", "commits: build a contiguous Scope (Enter applies it, a = whole Range); diff: select lines"),
        ("m", "toggle Reviewed for this file in the current Scope (stays on the file)"),
        ("M", "mark Reviewed and jump to the next unreviewed file"),
        ("s", "switch side-by-side / inline; the manual choice beats the width rule until :layout auto"),
        ("c  C  :comment", "Comment on the line / VISUAL range · on the whole file · on the whole Range"),
        ("o", "reopen the sent Comment under the cursor"),
        ("y  (VISUAL)", "copy the selected code only: plain code, or a diff with ' ' '+' '-' when it has both removed and added lines"),
        ("Space y", "copy the selection (VISUAL or mouse) with the anchored Comments inserted as '> ' quoted lines"),
        ("Y  Space p p", "Prompt from ALL open Comments (OSC 52) → they become sent, the Round closes"),
        ("Space p f", "Prompt from the open Comments of the current file"),
        ("Space p c", "Prompt from the Comment under the cursor"),
        ("Space p s", "show the Prompt overlay"),
        ("Space f / c", "pickers: Groups-files / commits (Scope)"),
        ("Space e", "show / hide the sidebar (it also hides itself below 100 columns)"),
        ("Space s / d / u", "Summary / diagram / unreviewed-only filter"),
        ("Esc", "cancel VISUAL / clear selection / clear search"),
        ("?", "this help"),
        (":q  :comment", ":scope all|N|N-M · :layout auto|side|inline · :set mouse|nomouse · :set sidebar|nosidebar"),
        (":diagram …", ":prompt show|save · :reload · :analyze · :reset"),
        ("", ""),
        ("MOUSE", "on by default. Click selects a file / commit / pane or places the diff cursor; double-click a commit applies it as Scope; wheel scrolls the pane under the pointer."),
        ("MOUSE SELECT", "drag in the diff selects CODE ONLY (never line numbers, gutter, +/- signs, boxes or the other column) and releasing the button copies it. Side-by-side: stays in the column where the drag started. Inline: spans context, added and removed lines (diff format when it has both added and removed). Double-click = word, triple-click = line. Esc or a click elsewhere clears it. Space y copies it again with Comments. :set nomouse gives the terminal its native selection back."),
        ("CLIPBOARD", "every copy is a real OSC 52 write. Caveats (not verified here): terminals may cap the payload size; mosh drops an identical repeated copy and needs 1.4.0+ on both ends; zellij copy_command would copy on the HOST."),
        ("", ""),
        ("GUTTER", "▶ cursor · digit = commit that made the change (cyan Round 1, orange Round 2)"),
        ("LISTS", "✓ reviewed  ◐ partly reviewed  · not reviewed  ✎ has Comments"),
    ];
    let mut out: Vec<Line<'static>> = Vec::new();
    for (k, v) in rows {
        let wrapped = if v.is_empty() { vec![String::new()] } else { wrap(v, iw.saturating_sub(18)) };
        for (n, l) in wrapped.into_iter().enumerate() {
            let key = if n == 0 { format!("{k:<18}") } else { " ".repeat(18) };
            out.push(Line::from(vec![Span::styled(key, Style::new().fg(YELLOW)), Span::styled(l, Style::new().fg(FG))]));
        }
    }
    out
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
    let rc: Vec<&Comment> = app.comments.iter().filter(|c| c.kind == CKind::Range).collect();
    if !rc.is_empty() {
        out.push(Line::raw(""));
        out.push(head("Comments на весь Range"));
        for c in rc {
            for l in wrap(&format!("[{}] {}", if c.state == CState::Open { "open" } else { "sent" }, c.text), w) {
                out.push(Line::raw(l));
            }
        }
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
    out.push(Line::styled("(prototype: hardcoded messages, drawn by a 40-line function; the real tool renders Mermaid text with mmdflux)", Style::new().fg(DIM)));
    out
}

fn draw_too_small(f: &mut Frame, area: Rect, app: &App) {
    f.render_widget(Clear, area);
    let lines = vec![
        Line::styled("терминал слишком мал", Style::new().fg(YELLOW).add_modifier(Modifier::BOLD)),
        Line::styled(format!("сейчас {}×{}, нужно хотя бы {}×{}", area.width, area.height, MIN_W, MIN_H), Style::new().fg(FG)),
        Line::styled("увеличь окно: состояние сохранено", Style::new().fg(DIM)),
        Line::styled(format!("({} · {})", scope_short(app), app.current_file().map_or("-", |i| app.files[i].path)), Style::new().fg(DIM)),
    ];
    let h = (lines.len() as u16).min(area.height);
    let r = Rect { x: area.x, y: area.y + area.height.saturating_sub(h) / 2, width: area.width, height: h };
    let lines: Vec<Line<'static>> = lines.into_iter().map(|l| l.centered()).collect();
    f.render_widget(Paragraph::new(lines), r);
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    app.size = (area.width, area.height);
    app.hits = Hits::default();
    if area.width < MIN_W || area.height < MIN_H {
        draw_too_small(f, area, app);
        return;
    }
    if !app.sidebar_visible() && app.focus != Pane::Diff {
        app.focus = Pane::Diff;
    }
    let [bar, main, status, cmd] = Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)]).areas(area);
    proto_bar(f, bar, app);
    if app.sidebar_visible() {
        draw_with_sidebar(f, main, app);
    } else {
        draw_strip_layout(f, main, app);
    }
    app.hits.range_chip = status_line(f, status, app);
    cmd_line(f, cmd, app);
    draw_editor_popup(f, main, app);
    draw_overlay(f, main, app);
    draw_whichkey(f, main, app);
}

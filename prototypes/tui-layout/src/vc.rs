// PROTOTYPE variant C "Review queue": the unreviewed queue is the navigation.
// One file at a time; n / p (or ]f / [f) step through it; m means "done, next".
// A header shows position, Group and Round; a footer shows what comes next.
// Commits and Groups are overlays (gc / gf). Primary affordance: "what's next?".

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::*;
use crate::ui::*;

fn bar(done: usize, total: usize, w: usize) -> String {
    let filled = if total == 0 { 0 } else { done * w / total };
    format!("{}{}", "█".repeat(filled), "░".repeat(w - filled))
}

fn rounds_label(app: &App, fi: usize) -> String {
    let rs: Vec<u8> = app
        .atoms(fi)
        .iter()
        .filter(|c| !app.unreviewed_only || !app.reviewed.contains(&(fi, **c)))
        .map(|c| app.commits[*c].round)
        .collect();
    match (rs.iter().min(), rs.iter().max()) {
        (Some(a), Some(b)) if a == b => format!("Round {a}"),
        (Some(a), Some(b)) => format!("Rounds {a}–{b}"),
        _ => "Round -".into(),
    }
}

pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    let [head, body, foot] = Layout::vertical([Constraint::Length(3), Constraint::Min(3), Constraint::Length(1)]).areas(area);
    let vf = app.visible_files();
    let (d, t) = app.progress();
    let title = Style::new().add_modifier(Modifier::BOLD);

    let queue_word = if app.unreviewed_only { "Queue" } else { "Browse" };
    let pos = if vf.is_empty() { 0 } else { app.file_cursor + 1 };
    let mut l1 = vec![
        Span::styled(format!(" {queue_word} {pos}/{}", vf.len()), title.fg(ACCENT)),
        Span::styled(format!("  [{}] {d}/{t} done", bar(d, t, 14)), Style::new().fg(GREEN)),
    ];
    let mut l2: Vec<Span<'static>> = vec![Span::raw(" ")];
    match app.current_file() {
        Some(fi) => {
            let fl = &app.files[fi];
            let g = &app.groups[fl.group];
            l1.push(Span::styled(format!("  ·  Group {}/{} «{}»{}", fl.group + 1, app.groups.len(), g.name, if g.mechanical { " [mechanical]" } else { "" }), Style::new().fg(FG)));
            l1.push(Span::styled(format!("  ·  {}", rounds_label(app, fi)), Style::new().fg(round_color(app.commits[*app.atoms(fi).last().unwrap()].round))));
            l2.push(Span::styled(fl.path.to_string(), title));
            l2.push(Span::raw("  "));
            l2.push(status_glyph(app.file_status(fi)));
            let atoms = app.atoms(fi);
            let todo: Vec<String> = atoms.iter().filter(|c| !app.reviewed.contains(&(fi, **c))).map(|c| format!("c{}", c + 1)).collect();
            let done: Vec<String> = atoms.iter().filter(|c| app.reviewed.contains(&(fi, **c))).map(|c| format!("c{}", c + 1)).collect();
            let note = if done.is_empty() {
                format!("  first look: {}", todo.join(" "))
            } else if todo.is_empty() {
                format!("  all reviewed: {}", done.join(" "))
            } else if app.unreviewed_only {
                format!("  delta since you reviewed {} → new {}", done.join(" "), todo.join(" "))
            } else {
                format!("  reviewed {} · unreviewed {}", done.join(" "), todo.join(" "))
            };
            l2.push(Span::styled(note, Style::new().fg(DIM)));
        }
        None => {
            l1.push(Span::styled("  ·  queue is empty", Style::new().fg(GREEN)));
            l2.push(Span::styled("every file in this Scope is reviewed", Style::new().fg(DIM)));
        }
    }
    let rule = Line::styled("─".repeat(area.width as usize), Style::new().fg(Color::Indexed(240)));
    f.render_widget(Paragraph::new(vec![Line::from(l1), Line::from(l2), rule]), head);

    draw_diff(f, body, app, Block::new().borders(Borders::NONE));

    let next = vf.get(app.file_cursor + 1).map(|fi| {
        let fl = &app.files[*fi];
        format!(" Next: {}  ({}. {})   ·  then {} more", fl.path, fl.group + 1, app.groups[fl.group].name, vf.len().saturating_sub(app.file_cursor + 2))
    });
    let text = next.unwrap_or_else(|| " Next: — end of queue —".to_string());
    f.render_widget(Paragraph::new(Line::styled(text, Style::new().fg(Color::Indexed(250)).bg(Color::Indexed(235)))), foot);
}

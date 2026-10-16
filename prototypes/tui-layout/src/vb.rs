// PROTOTYPE variant B "Diff-first": the diff owns the whole width (built for narrow
// terminals such as mosh). A two-line strip on top carries Scope, progress and the
// current file; commits and Groups/files are pickers opened with gc / gf.
// Primary affordance: the diff itself; navigation is by ]f / [f and the pickers.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::app::*;
use crate::ui::*;

pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    let [strip, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(3)]).areas(area);
    let bg = Style::new().bg(Color::Indexed(235)).fg(FG);
    let w = area.width as usize;

    let vf = app.visible_files();
    let (d, t) = app.progress();
    let mut l1 = vec![
        Span::styled(format!(" Scope {}", app.scope_label()), bg.fg(ACCENT)),
        Span::styled(format!("  │  Round {}", app.round), bg.fg(round_color(app.round))),
        Span::styled(format!("  │  ✓ {d}/{t} files"), bg.fg(GREEN)),
    ];
    if app.unreviewed_only {
        l1.push(Span::styled("  │  unreviewed only", bg.fg(YELLOW)));
    }
    l1.push(Span::styled(format!("  │  file {}/{}", if vf.is_empty() { 0 } else { app.file_cursor + 1 }, vf.len()), bg.fg(DIM)));

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
        }
        None => l2.push(Span::styled("(no file in this Scope / filter)", bg.fg(DIM))),
    }
    let tail = "gc commits · gf files ";
    let used: usize = l2.iter().map(|s| s.content.chars().count()).sum();
    if used + tail.chars().count() < w {
        l2.push(Span::styled(" ".repeat(w - used - tail.chars().count()), bg));
        l2.push(Span::styled(tail, bg.fg(DIM)));
    } else if used < w {
        l2.push(Span::styled(" ".repeat(w - used), bg));
    }
    let used1: usize = l1.iter().map(|s| s.content.chars().count()).sum();
    if used1 < w {
        l1.push(Span::styled(" ".repeat(w - used1), bg));
    }
    f.render_widget(Paragraph::new(vec![Line::from(l1), Line::from(l2)]), strip);

    draw_diff(f, body, app, Block::new());
}

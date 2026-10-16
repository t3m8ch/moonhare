// PROTOTYPE variant A "IDE-like": the Q21 sketch.
// Left column: Commits (Scope) stacked above Groups/files. Right: the diff, Comments inline.
// Primary affordance: three always-visible panes, you move focus between them.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::app::*;
use crate::ui::*;

fn border(app: &App, p: Pane) -> Style {
    if app.focus == p {
        Style::new().fg(ACCENT)
    } else {
        Style::new().fg(Color::Indexed(240))
    }
}

pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    let left_w = if area.width >= 150 {
        44
    } else if area.width >= 100 {
        36
    } else {
        26
    };
    let [left, right] = Layout::horizontal([Constraint::Length(left_w), Constraint::Min(10)]).areas(area);
    let want = commit_lines(app, 10, false).0.len() as u16 + 2;
    let commits_h = want.min((left.height * 55 / 100).max(5));
    let [cpane, fpane] = Layout::vertical([Constraint::Length(commits_h), Constraint::Min(3)]).areas(left);

    // commits
    let cb = Block::bordered().title(" Commits ").border_style(border(app, Pane::Commits));
    let ci = cb.inner(cpane);
    f.render_widget(cb, cpane);
    let (lines, cl) = commit_lines(app, ci.width as usize, app.focus == Pane::Commits);
    let sc = scroll_for(lines.len(), ci.height as usize, cl);
    let vis: Vec<Line<'static>> = lines.into_iter().skip(sc).take(ci.height as usize).collect();
    f.render_widget(Paragraph::new(vis), ci);

    // groups / files
    let (d, t) = app.progress();
    let fb = Block::bordered().title(format!(" Groups / files  ✓{d}/{t} ")).border_style(border(app, Pane::Files));
    let fi = fb.inner(fpane);
    f.render_widget(fb, fpane);
    let (lines, cl) = file_lines(app, fi.width as usize, app.file_cursor, app.focus == Pane::Files);
    let sc = scroll_for(lines.len(), fi.height as usize, cl);
    let vis: Vec<Line<'static>> = lines.into_iter().skip(sc).take(fi.height as usize).collect();
    f.render_widget(Paragraph::new(vis), fi);

    // diff
    let title = match app.current_file() {
        Some(i) => format!(" {} ", trunc(&file_title(app, i), right.width.saturating_sub(4) as usize)),
        None => " (no file) ".to_string(),
    };
    let db = Block::bordered().title(title).border_style(border(app, Pane::Diff));
    draw_diff(f, right, app, db);
}

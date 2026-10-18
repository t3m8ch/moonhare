//! Drawing Mermaid text: limits, a crashing or hanging renderer.

use std::time::{Duration, Instant};

use moonhare::diagram::{draw, draw_with, MAX_SOURCE};

#[test]
fn a_source_over_the_limit_is_refused_without_rendering() {
    let long = format!("graph LR\n{}", "A --> B\n".repeat(MAX_SOURCE));
    let err = draw(&long).unwrap_err();
    assert!(err.contains("too long"), "{err}");
}

#[test]
fn a_panicking_renderer_costs_the_picture_only() {
    let err = draw_with("x", |_| panic!("boom")).unwrap_err();
    assert!(err.contains("crashed"), "{err}");
}

#[test]
fn a_renderer_that_hangs_is_given_up_on() {
    let started = Instant::now();
    let err = draw_with("x", |_| {
        std::thread::sleep(Duration::from_secs(60));
        Ok(String::new())
    })
    .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(err.contains("too long to draw"), "{err}");
}

use moonhare::diagram::{candidates, draw_in, render, Density, Room};

fn width(lines: &[String]) -> usize {
    lines
        .iter()
        .map(|l| unicode_width::UnicodeWidthStr::width(l.as_str()))
        .max()
        .unwrap_or(0)
}

fn text(lines: &[String]) -> String {
    lines.join("\n")
}

const WIDE: &str = "graph LR\n    A[Alpha stage] --> B[Bravo stage] --> C[Charlie stage] --> D[Delta stage] --> E[Echo stage] --> F[Foxtrot stage]";

fn room(width: usize, height: usize) -> Room {
    Room { width, height }
}

#[test]
fn a_diagram_that_fits_is_drawn_as_written() {
    let small = "graph LR\n    A[Collect] --> B[Render]";
    assert_eq!(
        draw_in(small, room(80, 20), Density::Fit).unwrap(),
        draw(small).unwrap()
    );
}

#[test]
fn a_wide_left_to_right_flowchart_fits_once_turned_top_down() {
    let as_written = draw(WIDE).unwrap();
    assert!(width(&as_written) > 60, "{}", text(&as_written));

    let fitted = draw_in(WIDE, room(60, 60), Density::Fit).unwrap();

    assert!(width(&fitted) <= 60, "{}", text(&fitted));
    assert!(fitted.len() > as_written.len(), "{}", text(&fitted));
    for name in ["Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot"] {
        assert!(text(&fitted).contains(name), "{name}: {}", text(&fitted));
    }
}

#[test]
fn long_labels_of_nodes_and_edges_are_wrapped() {
    let source = "graph TD\n    A[the first step of a rather long process] -->|when the configuration says so| B[the second step of it]";
    assert!(width(&draw(source).unwrap()) > 40);

    let fitted = draw_in(source, room(30, 60), Density::Fit).unwrap();

    assert!(width(&fitted) <= 30, "{}", text(&fitted));
    let shown = text(&fitted);
    for word in ["first", "rather", "process", "configuration", "second"] {
        assert!(shown.contains(word), "{word}: {shown}");
    }
}

#[test]
fn a_sequence_diagram_with_long_messages_fits_the_width() {
    let source = "sequenceDiagram\n    participant U as User\n    participant S as Session\n    participant A as Analyst\n    U->>S: types the diagram command on the line under the cursor\n    S->>A: asks for a diagram of the fragment around that line\n    A-->>S: answers with Mermaid text that is drawn\n    S-->>U: shows the overlay with the picture";
    assert!(width(&draw(source).unwrap()) > 100);

    let fitted = draw_in(source, room(70, 40), Density::Fit).unwrap();

    assert!(width(&fitted) <= 70, "{}", text(&fitted));
    let shown = text(&fitted);
    for start in [
        "User", "Session", "Analyst", "types", "asks", "answers", "shows",
    ] {
        assert!(shown.contains(start), "{start}: {shown}");
    }
}

#[test]
fn when_nothing_fits_the_narrowest_drawing_is_chosen() {
    let tiny = room(5, 5);
    let narrowest = candidates(WIDE, Density::Fit)
        .iter()
        .filter_map(|c| render(c).ok())
        .map(|lines| width(&lines))
        .min()
        .unwrap();

    let fitted = draw_in(WIDE, tiny, Density::Fit).unwrap();

    assert_eq!(width(&fitted), narrowest, "{}", text(&fitted));
}

#[test]
fn a_diagram_that_cannot_be_drawn_reports_why() {
    let err = draw_in("graph LR\n    A[Open --> ???", room(80, 20), Density::Fit).unwrap_err();
    assert!(!err.is_empty());
}

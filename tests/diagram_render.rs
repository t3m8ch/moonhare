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
    // Top-down: one block under another, joined by arrows down.
    let rows: Vec<usize> = ["Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot"]
        .iter()
        .map(|name| {
            fitted
                .iter()
                .position(|l| l.contains(name))
                .unwrap_or_else(|| panic!("{name}: {}", text(&fitted)))
        })
        .collect();
    assert!(rows.windows(2).all(|w| w[0] < w[1]), "{}", text(&fitted));
    assert!(text(&fitted).contains('▼'), "{}", text(&fitted));
    assert!(!text(&fitted).contains('►'), "{}", text(&fitted));
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

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use moonhare::diagram::{check_svg, svg, without_styling, Drawings, Renderer};

/// `mmdflux`, slowed down by `delay` for the texts `slow` picks.
fn slowed(delay: Duration, slow: fn(&str) -> bool) -> Renderer {
    Arc::new(move |source: &str, _| {
        if slow(source) {
            std::thread::sleep(delay);
        }
        draw(source).map(|lines| lines.join("\n"))
    })
}

#[test]
fn layouts_not_drawn_in_time_leave_the_best_of_those_drawn() {
    // The diagram as written is drawn at once; the turned layouts take long.
    let drawings = Drawings::with(slowed(Duration::from_secs(3), |s| !s.contains("LR")));
    let started = Instant::now();

    let fitted = drawings
        .fit(WIDE, room(60, 60), Density::Fit, Duration::from_millis(300))
        .unwrap();

    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(text(&fitted).contains("Alpha"), "{}", text(&fitted));
    assert!(text(&fitted).contains('►'), "{}", text(&fitted));
}

#[test]
fn a_diagram_too_slow_for_one_room_is_tried_again_for_the_next() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let renderer: Renderer = Arc::new(move |source: &str, _| {
        counted.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(400));
        draw(source).map(|lines| lines.join("\n"))
    });
    let drawings = Drawings::with(renderer);
    let small = "graph TD\n    A[Collect] --> B[Render]";

    let err = drawings
        .fit(small, room(80, 20), Density::Fit, Duration::from_millis(50))
        .unwrap_err();
    assert!(err.contains("too long"), "{err}");
    assert!(drawings
        .fit_now(small, room(80, 20), Density::Fit)
        .is_none());

    // The layout given up on is kept once it is done, and not drawn twice.
    std::thread::sleep(Duration::from_secs(1));
    let fitted = drawings
        .fit(small, room(70, 20), Density::Fit, Duration::from_millis(50))
        .unwrap();
    assert!(text(&fitted).contains("Collect"), "{}", text(&fitted));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(drawings
        .fit_now(small, room(60, 20), Density::Fit)
        .is_some());
}

/// What the Analyst could be talked into writing by the code it reads.
const ATTACKS: &[&str] = &[
    "graph LR\n  A[x] --> B\n  style A fill:#f00\" onmouseover=\"alert(document.domain)",
    "graph LR\n  A[x] --> B\n  style A fill:#f00\"/><script>alert(1)</script><rect x=\"0",
    "graph LR\n  A[x] --> B\n  linkStyle 0 stroke:red\"/><script>alert(2)</script><path d=\"",
    "graph LR\n  A[x] --> B\n  style A fill:url(http://evil.example/x)",
    "graph LR\n  A[x] --> B\n  classDef foo fill:#f00\"/><script>alert(3)</script><rect x=\"0\n  class A foo",
    "graph LR\n  A[x] --> B;style A fill:#f00\" onload=\"alert(4)",
    "graph LR\n  A[x] --> B\n  click A \"javascript:alert(5)\"",
    "---\nconfig:\n  themeVariables:\n    primaryColor: '\"/><script>alert(6)</script>'\n---\ngraph LR\n  A --> B",
    "%%{init: {'themeVariables': {'primaryColor': '\"/><script>alert(7)</script>'}}}%%\ngraph LR\n  A --> B",
    "graph LR\n  A[\"<script>alert(8)</script>\"] -->|<img src=x onerror=alert(9)>| B",
    "sequenceDiagram\n  A->>B: <script>alert(10)</script>",
    // Any case, any end of line, a `>` inside quotes.
    "graph TD\n A-->B\n STYLE A fill:#f00\" foo=\"a>\" onmouseover=\"alert(document.domain)",
    "graph TD\r A-->B\r style A fill:#f00\" foo=\"a>\" onmouseover=\"alert(11)",
    "graph TD\r\n A-->B\r\n Style A fill:#f00\" onmouseover=\"alert(12)",
    "graph TD\n A-->B\n LINKSTYLE 0 stroke:red\" foo=\"a>\" onmouseover=\"alert(13)",
    "graph TD\n A-->B\n CLASSDEF c fill:red\" foo=\"a>\" onmouseover=\"alert(14)\n CLASS A c",
    "graph TD\n A-->B\n style A fill:#f00\" x=\"--\" onmouseover=\"alert(15)",
];

#[test]
fn an_svg_has_nothing_in_it_that_runs_or_reaches_out() {
    for attack in ATTACKS {
        let Ok(picture) = svg(attack) else {
            continue;
        };
        let lower = picture.to_ascii_lowercase();
        for active in [
            "<script",
            " onmouseover=",
            " onload=",
            " onerror=",
            "javascript:",
            "url(http",
        ] {
            assert!(
                !lower.contains(active),
                "{active} from {attack}:\n{picture}"
            );
        }
        assert_eq!(check_svg(&picture), Ok(()), "{attack}");
    }
}

#[test]
fn an_svg_with_anything_active_is_refused() {
    for active in [
        "<svg><rect fill=\"#f00\" foo=\"a>\" onmouseover=\"alert(1)\"/></svg>",
        "<svg><rect fill='a>' onmouseover='alert(1)'/></svg>",
        "<svg><rect fill=\"#f00\" ONMOUSEOVER=\"alert(1)\"/></svg>",
        "<svg><rect fill=\"&#117;rl(http://evil.example)\"/></svg>",
        "<svg><rect style=\"fill:u\\72l(http://evil.example)\"/></svg>",
        "<svg><rect fill=\"#f00\"/><!-- x --></svg>",
        "<svg><text><![CDATA[x]]></text></svg>",
        "<svg><rect fill=\"&evil;\"/></svg>",
        "<svg><rect fill=\"#f00\" fill=\"#0f0\"/></svg>",
        "<svg><rect fill=#f00 /></svg>",
        "<svg><g></svg>",
        "<svg></svg><svg></svg>",
        "<rect/>",
        "",
        "<svg><script>alert(1)</script></svg>",
        "<svg><rect onload=\"alert(1)\"/></svg>",
        "<svg><rect fill=\"#f00\" onmouseover = 'x'/></svg>",
        "<svg><a href=\"https://evil.example\"><text>x</text></a></svg>",
        "<svg><image xlink:href=\"https://evil.example/x.png\"/></svg>",
        "<svg><rect fill=\"url(https://evil.example/x)\"/></svg>",
        "<svg><rect style=\"fill:url('http://evil.example')\"/></svg>",
        "<svg><foreignObject><div>x</div></foreignObject></svg>",
        "<!DOCTYPE svg [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><svg>&x;</svg>",
        "<?xml-stylesheet href=\"https://evil.example/x.css\"?><svg/>",
        "<svg><style>@import url(https://evil.example)</style></svg>",
        "<svg><set attributeName=\"fill\" to=\"red\"/></svg>",
        "<svg><path d=\"M0 0\" fill=\"javascript:alert(1)\"/></svg>",
    ] {
        assert!(check_svg(active).is_err(), "{active}");
    }
}

#[test]
fn ordinary_diagrams_draw_as_svg() {
    for source in [
        "graph LR\n    A[Collect] --> B[Render]",
        "graph TD\n    A[on load] -->|href=x| B[url(here)]",
        "sequenceDiagram\n    A->>B: hello\n    B-->>A: data: done",
    ] {
        let picture = svg(source).unwrap();
        assert!(picture.contains("<svg"), "{picture}");
    }
}

#[test]
fn styling_means_nothing_to_the_picture_in_the_terminal() {
    let plain = "graph LR\n    A[Collect] --> B[Render]";
    for styled in [
        "graph LR\n    A[Collect] --> B[Render]\n    style A fill:#f00\"/><script>x</script>",
        "graph LR\n    A[Collect] --> B[Render]; classDef c fill:red\n    class A c",
        "%%{init: {'theme': 'dark'}}%%\ngraph LR\n    A[Collect] --> B[Render]\n    click A \"javascript:x\"",
    ] {
        assert_eq!(
            draw_in(styled, room(80, 20), Density::Fit).unwrap(),
            draw_in(plain, room(80, 20), Density::Fit).unwrap(),
            "{styled}"
        );
    }
}

#[test]
fn front_matter_directives_and_styling_are_left_out_and_nothing_else() {
    let source = "---\nconfig:\n  theme: dark\n---\n%%{init: {}}%%\ngraph LR\n  A[style] --> B; style A fill:red\n  %% a comment\n  linkStyle 0 stroke:red\n  B --> C";
    assert_eq!(
        without_styling(source),
        "graph LR\n  A[style] --> B\n  %% a comment\n  B --> C"
    );
}

#[test]
fn each_guard_of_the_svg_holds_on_its_own() {
    // Left out by the first guard, the styling cannot spoil the picture.
    let styled = "graph TD\n A-->B\n STYLE A fill:#f00\" onmouseover=\"alert(1)";
    let picture = svg(styled).unwrap();
    assert!(!picture.contains("onmouseover"), "{picture}");
    // Past the first guard (an edge-like `--` in it), the second refuses it.
    let past = "graph TD\n A-->B\n style A fill:#f00\" x=\"--\" onmouseover=\"alert(1)";
    assert!(without_styling(past).contains("onmouseover"));
    let err = svg(past).unwrap_err();
    assert!(err.contains("not opened"), "{err}");
}

#[test]
fn styling_is_left_out_whatever_its_case_and_ends_of_lines() {
    for source in [
        "graph TD\r  A-->B\r  style A fill:red\r  B-->C",
        "graph TD\r\n  A-->B\r\n  STYLE A fill:red\r\n  B-->C",
        "graph TD\n  A-->B\n  ClassDef c fill:red\n  CLASS A c\n  LinkStyle 0 stroke:red\n  B-->C",
    ] {
        assert_eq!(
            without_styling(source),
            "graph TD\n  A-->B\n  B-->C",
            "{source:?}"
        );
    }
}

#[test]
fn what_only_looks_like_styling_is_kept() {
    for source in [
        // A class diagram declares classes with `class`.
        "classDiagram\n  class Animal{\n    +String name\n  }\n  class List~T~\n  Animal <|-- Duck",
        // `;` inside a label or a message.
        "graph TD\n  A[\"x; class y z\"] --> B(\"a; style b\")",
        "sequenceDiagram\n  S->>S: style check; class names",
        // An edge from a node whose name is a keyword.
        "graph TD\n  Style --> Click",
    ] {
        assert_eq!(without_styling(source), source);
    }
    let picture = draw("classDiagram\n  class Animal{\n    +String name\n  }")
        .unwrap()
        .join("\n");
    assert!(picture.contains("name"), "{picture}");
}

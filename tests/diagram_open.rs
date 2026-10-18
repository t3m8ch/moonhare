//! `o` draws the diagram as SVG into a temporary file and hands it to the
//! system viewer (`xdg-open`, `open` on macOS). The viewer is a fake found
//! first on `PATH`; this file has one test because it changes `PATH` and
//! `TMPDIR` of the whole process.

mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use moonhare::analyst::{Analyst, AnalystError, Diagram, DiagramRequest, Summary, SummaryRequest};
use moonhare::session::Session;
use support::{feature_branch, Harness};

const FLOW: &str = "graph LR\n    A[Collect] --> B[Render]";

struct Drawing;

impl Analyst for Drawing {
    fn summarize(&self, _: &SummaryRequest) -> Result<Summary, AnalystError> {
        Ok(Summary {
            text: "Summary text".into(),
            diagram: Some("graph TD\n    S[Summarised] --> T[Shown]".into()),
        })
    }

    fn diagram(&self, _: &DiagramRequest) -> Result<Diagram, AnalystError> {
        Ok(Diagram {
            mermaid: FLOW.into(),
        })
    }
}

/// A viewer that writes what it was given to `log` and exits with `status`.
fn fake_viewer(dir: &Path, log: &Path, status: i32) {
    for name in ["xdg-open", "open"] {
        let path = dir.join(name);
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\nexit {status}\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// The lines of `log` once there are `count` of them.
fn wait_for_lines(log: &Path, count: usize) -> Vec<String> {
    let started = Instant::now();
    loop {
        let lines: Vec<String> = std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect();
        if lines.len() >= count || started.elapsed() > Duration::from_secs(10) {
            return lines;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_screen(h: &mut Harness, text: &str) -> String {
    let started = Instant::now();
    loop {
        h.wait(20);
        let screen = h.screen();
        if screen.contains(text) || started.elapsed() > Duration::from_secs(10) {
            return screen;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn o_writes_the_diagram_as_svg_and_hands_it_to_the_system_viewer() {
    let fake = tempfile::tempdir().unwrap();
    let bin = fake.path().join("bin");
    let tmp = fake.path().join("tmp");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    let log = fake.path().join("viewer.log");
    fake_viewer(&bin, &log, 0);
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{path}", bin.display()));
    std::env::set_var("TMPDIR", &tmp);

    let fx = feature_branch();
    let mut session = Session::open(fx.path(), 100, 30).expect("session opens");
    session.set_analyst(Arc::new(Drawing));
    let mut h = Harness::from_session(session);
    // The system viewer, found on `PATH`: the fake above.
    h.session().set_viewer(moonhare::session::system_viewer());
    h.session().wait_for_analyst();

    // SVGs of an earlier session: one left over for days, one fresh.
    let left_over = tmp.join("moonhare-diagram-left-over.svg");
    let fresh = tmp.join("moonhare-diagram-fresh.svg");
    for file in [&left_over, &fresh] {
        std::fs::write(file, "<svg/>").unwrap();
    }
    std::fs::File::options()
        .write(true)
        .open(&left_over)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - Duration::from_secs(3 * 24 * 60 * 60))
        .unwrap();

    // The diagram overlay.
    h.keys(":diagram<CR>");
    h.session().wait_for_analyst();
    h.keys("o");
    let opened = wait_for_lines(&log, 1);
    assert_eq!(opened.len(), 1, "{opened:?}");
    let svg = Path::new(&opened[0]);
    assert!(svg.starts_with(&tmp), "{svg:?}");
    assert_eq!(svg.extension().unwrap(), "svg");
    let text = std::fs::read_to_string(svg).unwrap();
    assert!(text.contains("<svg") && text.contains("Collect"), "{text}");
    let screen = wait_for_screen(&mut h, "Opened");
    assert!(screen.contains("Opened"), "{screen}");
    assert!(!left_over.exists() && fresh.exists());
    let first = svg.to_path_buf();

    // The diagram of the Summary, from the Summary.
    h.keys("q<Space>so");
    let opened = wait_for_lines(&log, 2);
    assert_eq!(opened.len(), 2, "{opened:?}");
    let text = std::fs::read_to_string(&opened[1]).unwrap();
    assert!(text.contains("Summarised"), "{text}");

    // A viewer that fails is reported, with where the file is.
    fake_viewer(&bin, &log, 3);
    h.keys("q:diagram<CR>");
    h.session().wait_for_analyst();
    h.keys("o");
    let screen = wait_for_screen(&mut h, "failed");
    assert!(
        screen.contains("xdg-open failed") || screen.contains("open failed"),
        "{screen}"
    );

    // The session takes the SVGs it wrote with it.
    assert!(first.exists());
    drop(h);
    assert!(!first.exists(), "{first:?}");
    assert!(fresh.exists());
}

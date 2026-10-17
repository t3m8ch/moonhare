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

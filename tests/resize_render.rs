//! The terminal may change size before the session hears of it: between the
//! size read at start and the first event, or between a resize and its event.
//! The screen is drawn for the buffer it is given, never past its edges.

mod support;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use support::{feature_branch, Harness};

fn draw(h: &mut Harness, width: u16, height: u16) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    h.session().render(&mut buf);
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_session_opened_for_one_size_draws_into_a_buffer_of_another() {
    let fx = feature_branch();
    for (width, height) in [(1, 1), (40, 10), (59, 15), (60, 15), (120, 40), (200, 60)] {
        let mut h = Harness::open(&fx, 120, 40);
        let screen = draw(&mut h, width, height);
        if width >= 60 && height >= 15 {
            assert!(screen.contains("README.md"), "{width}x{height}:\n{screen}");
        } else {
            assert!(screen.starts_with('T'), "{width}x{height}:\n{screen}");
        }
    }
}

#[test]
fn the_next_frame_keeps_the_size_of_the_last_buffer() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 120, 40);
    draw(&mut h, 200, 60);
    let rows = h.screen().lines().count();
    assert_eq!(rows, 60, "the session adopts the size it was drawn at");
}

#[test]
fn a_series_of_unannounced_resizes_with_overlays_open_does_not_panic() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 120, 40);
    let sizes = [
        (1, 1),
        (200, 60),
        (2, 3),
        (61, 16),
        (1, 50),
        (300, 2),
        (60, 15),
        (80, 24),
    ];
    for keys in ["", "?", "<Esc><Space>c", "<Esc><Space>f", "<Esc>:"] {
        h.keys(keys);
        for &(width, height) in sizes.iter().chain(sizes.iter().rev()) {
            draw(&mut h, width, height);
        }
    }
}

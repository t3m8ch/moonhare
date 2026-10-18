//! A file of one huge line (minified JS, a JSON dump) is drawn as fast as a
//! short one: a frame builds cells only for the columns on screen, however
//! long the line and however many tokens it has.

mod support;

use std::time::{Duration, Instant};

use support::{Fixture, Harness};

/// One line of `n` small functions: ~36 bytes and a dozen tokens each.
/// Function `edited` returns a different number.
fn bundle(n: usize, edited: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let k = if i == edited { 1000 } else { (i * 37) % 97 };
        s.push_str(&format!("var a{i}=function(b,c){{return b*{k}+c}};"));
    }
    s.push('\n');
    s
}

/// A frame of the diff, with how long it took to draw.
fn timed_screen(h: &mut Harness) -> (String, Duration) {
    let start = Instant::now();
    let screen = h.screen_now();
    (screen, start.elapsed())
}

/// Far more than a visible-window frame needs even in a debug build on a slow
/// CI machine, and far less than the minutes a frame took when every frame
/// styled the whole line.
const FRAME_BUDGET: Duration = Duration::from_secs(2);

/// A repository whose one file is a single ~414 KB line, edited in the
/// middle; and that line as it was.
fn bundle_fixture() -> (Fixture, String) {
    let n = 11_500;
    let base = bundle(n, usize::MAX);
    assert!(base.len() >= 400_000, "{} bytes", base.len());
    let fx = Fixture::new();
    fx.write("bundle.min.js", &base).commit("base");
    fx.branch("feature");
    fx.write("bundle.min.js", &bundle(n, n / 2)).commit("edit");
    (fx, base)
}

/// Side by side, the old and the new start of the line share a row.
fn side_by_side(screen: &str) -> bool {
    screen.lines().any(|l| l.matches("var a0=").count() == 2)
}

#[test]
fn a_400_kb_line_draws_quickly_in_both_layouts_and_when_scrolled_far_right() {
    let (fx, base) = bundle_fixture();
    let expected = &base[8000..8012];
    for (width, height) in [(200, 50), (80, 24)] {
        let mut h = Harness::open(&fx, width, height);
        let first_layout = side_by_side(&h.screen_now());
        for pass in 0..2 {
            let (screen, took) = timed_screen(&mut h);
            let layout = if side_by_side(&screen) {
                "side-by-side"
            } else {
                "inline"
            };
            let at = format!("{width}x{height} {layout}");
            assert_eq!(
                side_by_side(&screen),
                first_layout == (pass == 0),
                "{at}:\n{screen}"
            );
            assert!(took < FRAME_BUDGET, "{at}: {took:?}");

            // 1000 steps right: the window shows the line from column 8000.
            h.keys_now("1000l");
            let (screen, took) = timed_screen(&mut h);
            assert!(
                screen.contains(expected),
                "{at} at 8000 {expected:?}:\n{screen}"
            );
            assert!(!screen.contains("var a0="), "{at}:\n{screen}");
            assert!(took < FRAME_BUDGET, "{at} at 8000: {took:?}");

            // The far end of the line, and a step back from it.
            h.keys_now("100000l");
            let (screen, took) = timed_screen(&mut h);
            assert!(screen.contains("+c};"), "{at} at the end:\n{screen}");
            assert!(took < FRAME_BUDGET, "{at} at the end: {took:?}");
            h.keys_now("h");
            let (screen, took) = timed_screen(&mut h);
            assert!(screen.contains("+c};"), "{at} a step back:\n{screen}");
            assert!(took < FRAME_BUDGET, "{at} a step back: {took:?}");

            h.keys_now("0s");
        }
    }
}

#[test]
fn code_dragged_over_far_right_on_a_400_kb_line_is_copied_and_a_comment_on_it_shows_quickly() {
    let (fx, base) = bundle_fixture();
    let expected = &base[8000..8012];
    let mut h = Harness::open(&fx, 80, 24);
    for layout in ["the first layout", "the other layout"] {
        h.keys_now("1000l");
        let (x, y) = h.frame_now().find(expected).expect("text at column 8000");
        h.drag((x, y), (x + 11, y));
        assert_eq!(h.copied(), vec![expected.to_string()], "{layout}");
        let (screen, took) = timed_screen(&mut h);
        assert!(screen.contains(expected), "{layout}:\n{screen}");
        assert!(
            took < FRAME_BUDGET,
            "{layout}, frame with a selection: {took:?}"
        );
        h.keys_now("<Esc>0s");
    }

    // A Comment on the long line sits under it and costs a frame nothing.
    h.keys_now("1000lcinote on the bundle<Esc><CR>");
    let (screen, took) = timed_screen(&mut h);
    assert!(screen.contains("note on the bundle"), "{screen}");
    assert!(screen.contains(expected), "{screen}");
    assert!(took < FRAME_BUDGET, "frame with a Comment: {took:?}");
}

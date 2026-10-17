//! Copying through the terminal: the OSC 52 escape sequence.

use moonhare::clipboard::osc52;

#[test]
fn osc52_carries_the_text_in_base64() {
    assert_eq!(osc52("abc"), "\x1b]52;c;YWJj\x07");
    assert_eq!(osc52("ab"), "\x1b]52;c;YWI=\x07");
    assert_eq!(osc52("a"), "\x1b]52;c;YQ==\x07");
    assert_eq!(osc52("Привет"), "\x1b]52;c;0J/RgNC40LLQtdGC\x07");
}

#[test]
fn sanitize_drops_invisible_direction_marks_and_line_separators() {
    let dirty = "a\u{200e}b\u{200f}c\u{61c}d\u{2028}e\u{2029}f\u{202e}g\u{2066}h";
    assert_eq!(moonhare::clipboard::sanitize(dirty), "abcdefgh");
}

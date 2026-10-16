//! Colour depth. The UI is drawn with the 256-colour palette, which every
//! modern terminal has; with truecolor on, the same palette is emitted as
//! exact RGB so it looks the same in every terminal theme.

use ratatui::buffer::Buffer;
use ratatui::style::Color;

/// Whether `$COLORTERM` announces truecolor support.
pub fn truecolor_from_colorterm(colorterm: Option<&str>) -> bool {
    matches!(colorterm, Some("truecolor" | "24bit"))
}

/// The RGB value of an xterm 256-colour palette entry.
pub fn indexed_to_rgb(i: u8) -> (u8, u8, u8) {
    const BASIC: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    match i {
        0..=15 => BASIC[i as usize],
        16..=231 => {
            let level = |n: u8| if n == 0 { 0 } else { 55 + 40 * n };
            let n = i - 16;
            (level(n / 36), level(n / 6 % 6), level(n % 6))
        }
        232..=255 => {
            let v = 8 + 10 * (i - 232);
            (v, v, v)
        }
    }
}

/// Replace every palette colour in the buffer by its RGB value.
pub fn to_truecolor(buf: &mut Buffer) {
    let convert = |c: Color| match c {
        Color::Indexed(i) => {
            let (r, g, b) = indexed_to_rgb(i);
            Color::Rgb(r, g, b)
        }
        other => other,
    };
    for cell in buf.content.iter_mut() {
        cell.fg = convert(cell.fg);
        cell.bg = convert(cell.bg);
    }
}

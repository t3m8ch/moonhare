//! Copying to the Reviewer's clipboard through the terminal (OSC 52), which
//! works over ssh and needs no clipboard library.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// The escape sequence that asks the terminal to copy `text`.
pub fn osc52(text: &str) -> String {
    let mut out = String::from("\x1b]52;c;");
    for chunk in text.as_bytes().chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out.push('\x07');
    out
}

/// `text` as it may be handed over: a terminal or an agent reading it must
/// not meet escape sequences or text-direction tricks that came from code.
/// Line breaks and tabs stay, `\r` goes, other control characters become U+FFFD.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            c != '\r'
                && !matches!(
                    c,
                    '\u{200e}' | '\u{200f}' | '\u{61c}' | '\u{2028}' | '\u{2029}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{2066}'..='\u{2069}'
                )
        })
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect()
}

/// The most text one OSC 52 carries. Terminals and multiplexers drop or cut
/// longer sequences (a payload of 100000 base64 bytes is the common ceiling),
/// so a longer text is refused instead of arriving cut.
pub const MAX_TEXT: usize = 74_994;

/// [`osc52`], or `None` when `text` is longer than [`MAX_TEXT`] bytes.
pub fn osc52_limited(text: &str) -> Option<String> {
    (text.len() <= MAX_TEXT).then(|| osc52(text))
}

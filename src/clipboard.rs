//! Copying to the Reviewer's clipboard through the terminal (OSC 52).

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `text` as it may be handed over: a terminal or an agent reading it must
/// not meet escape sequences or text-direction tricks that came from code.
/// Line breaks and tabs stay, `\r` goes, other control characters become U+FFFD.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .filter(|&c| c != '\r' && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
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

/// The sequence that makes the terminal put `text` into the clipboard, or
/// `None` when `text` is longer than [`MAX_TEXT`] bytes.
pub fn osc52(text: &str) -> Option<String> {
    (text.len() <= MAX_TEXT).then(|| format!("\x1b]52;c;{}\x07", base64(text.as_bytes())))
}

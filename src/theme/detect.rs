//! Telling the terminal background: the reply to an OSC 11 query, else
//! `$COLORFGBG`.

use super::{luminance, Background};
use crate::color::indexed_to_rgb;

/// Relative luminance of a mid grey (CIE L* = 50): lighter is a light
/// background.
const MIDPOINT: f64 = 0.184;

fn from_rgb(rgb: (u8, u8, u8)) -> Background {
    if luminance(rgb) > MIDPOINT {
        Background::Light
    } else {
        Background::Dark
    }
}

/// The background named by the terminal's reply to `OSC 11 ; ?`, such as
/// `ESC ] 11 ; rgb:ffff/ffff/ffff ESC \`. `None` when `reply` holds none.
pub fn background_from_osc11(reply: &str) -> Option<Background> {
    let at = reply.find("]11;")?;
    let body = &reply[at + 4..];
    let end = body.find(['\x07', '\x1b']).unwrap_or(body.len());
    let spec = &body[..end];
    let channels = spec
        .strip_prefix("rgba:")
        .or_else(|| spec.strip_prefix("rgb:"))?;
    let mut rgb = [0u8; 3];
    let mut parts = channels.split('/');
    for c in rgb.iter_mut() {
        let hex = parts.next()?;
        if hex.is_empty() || hex.len() > 4 {
            return None;
        }
        let value = u32::from_str_radix(hex, 16).ok()?;
        let max = (1u32 << (4 * hex.len())) - 1;
        *c = (value * 255 / max) as u8;
    }
    Some(from_rgb((rgb[0], rgb[1], rgb[2])))
}

/// The background named by `$COLORFGBG` (`fg;bg` or `fg;default;bg`, as
/// palette indices). `None` when it names none.
pub fn background_from_colorfgbg(value: &str) -> Option<Background> {
    let bg: u8 = value.rsplit(';').next()?.trim().parse().ok()?;
    Some(match bg {
        0..=6 | 8 => Background::Dark,
        7 | 9..=15 => Background::Light,
        n => from_rgb(indexed_to_rgb(n)),
    })
}

/// The terminal background: what `query` tells, else what `$COLORFGBG`
/// (`colorfgbg`) does. `None` when neither knows.
pub fn detect(
    query: impl FnOnce() -> Option<Background>,
    colorfgbg: Option<&str>,
) -> Option<Background> {
    query().or_else(|| colorfgbg.and_then(background_from_colorfgbg))
}

/// Ask the terminal behind `/dev/tty` for its background, waiting at most
/// `timeout`. `None` when there is no terminal or it does not tell.
#[cfg(unix)]
pub fn query_background(timeout: std::time::Duration) -> Option<Background> {
    use ratatui::crossterm::terminal;
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    let was_raw = terminal::is_raw_mode_enabled().unwrap_or(false);
    if !was_raw {
        terminal::enable_raw_mode().ok()?;
    }
    let input = tty.try_clone().ok();
    let got = input.and_then(|input| query_background_on(&input, &mut tty, timeout));
    if !was_raw {
        let _ = terminal::disable_raw_mode();
    }
    got
}

/// Write the OSC 11 query to `output` and read the reply from `input`,
/// waiting at most `timeout`. The query is followed by a request for the
/// device attributes, which every terminal answers: once that answer is in,
/// a terminal that has not told its background never will, and there is no
/// need to wait out the timeout.
#[cfg(unix)]
pub fn query_background_on(
    input: &impl std::os::fd::AsFd,
    output: &mut impl std::io::Write,
    timeout: std::time::Duration,
) -> Option<Background> {
    use std::os::fd::AsRawFd;
    use std::time::Instant;

    output.write_all(b"\x1b]11;?\x1b\\\x1b[c").ok()?;
    output.flush().ok()?;
    let fd = input.as_fd().as_raw_fd();
    let deadline = Instant::now() + timeout;
    let mut reply = Vec::new();
    loop {
        let text = String::from_utf8_lossy(&reply);
        if let Some(bg) = background_from_osc11(&text) {
            // Read on to the attributes so they do not reach the screen as keys.
            if attributes_answered(&reply) {
                return Some(bg);
            }
        } else if attributes_answered(&reply) {
            return None;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return background_from_osc11(&text);
        }
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ms = left.as_millis().clamp(1, i32::MAX as u128) as i32;
        // SAFETY: one valid pollfd for the duration of the call.
        let ready = unsafe { libc::poll(&mut pfd, 1, ms) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return None;
        }
        if ready == 0 {
            continue;
        }
        let mut chunk = [0u8; 256];
        // SAFETY: `chunk` is writable for its whole length.
        let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n <= 0 {
            return background_from_osc11(&String::from_utf8_lossy(&reply));
        }
        reply.extend_from_slice(&chunk[..n as usize]);
    }
}

/// Whether `reply` holds the answer to the device attributes request,
/// `ESC [ ? … c`.
#[cfg(unix)]
fn attributes_answered(reply: &[u8]) -> bool {
    reply.windows(3).enumerate().any(|(at, w)| {
        w == b"\x1b[?"
            && reply[at + 3..]
                .iter()
                .find(|b| !(b.is_ascii_digit() || **b == b';'))
                == Some(&b'c')
    })
}

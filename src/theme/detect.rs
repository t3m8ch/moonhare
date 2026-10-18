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

/// What came of asking the terminal for its background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Told(Background),
    /// No terminal, or one that answered without telling.
    Silent,
    /// The wait ran out: a reply may still come, late, as input.
    TimedOut,
}

impl Reply {
    pub fn background(self) -> Option<Background> {
        match self {
            Reply::Told(background) => Some(background),
            Reply::Silent | Reply::TimedOut => None,
        }
    }
}

/// Ask the terminal behind `/dev/tty` for its background, waiting at most
/// `timeout`.
#[cfg(unix)]
pub fn query_background(timeout: std::time::Duration) -> Reply {
    use std::os::unix::fs::OpenOptionsExt;
    // A descriptor of its own, non-blocking: a read never waits past the
    // deadline even where the wait for input fails to.
    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open("/dev/tty");
    let Ok(mut tty) = tty else {
        return Reply::Silent;
    };
    let Some(_raw) = RawMode::enable() else {
        return Reply::Silent;
    };
    let Ok(input) = tty.try_clone() else {
        return Reply::Silent;
    };
    query_background_on(&input, &mut tty, timeout)
}

/// Raw mode for as long as the query runs, also when it panics.
#[cfg(unix)]
struct RawMode {
    was_raw: bool,
}

#[cfg(unix)]
impl RawMode {
    fn enable() -> Option<RawMode> {
        use ratatui::crossterm::terminal;
        let was_raw = terminal::is_raw_mode_enabled().unwrap_or(false);
        if !was_raw {
            terminal::enable_raw_mode().ok()?;
        }
        Some(RawMode { was_raw })
    }
}

#[cfg(unix)]
impl Drop for RawMode {
    fn drop(&mut self) {
        if !self.was_raw {
            let _ = ratatui::crossterm::terminal::disable_raw_mode();
        }
    }
}

/// Write the OSC 11 query to `output` and read the reply from `input`,
/// waiting at most `timeout`. The query is followed by a request for the
/// device attributes, which every terminal answers: once that answer is in,
/// a terminal that has not told its background never will, and there is no
/// need to wait out the timeout. Any failure to wait or read is "no reply".
///
/// The wait is `select(2)`: `poll(2)` does not work on terminals on macOS.
#[cfg(unix)]
pub fn query_background_on(
    input: &impl std::os::fd::AsFd,
    output: &mut impl std::io::Write,
    timeout: std::time::Duration,
) -> Reply {
    use std::io::ErrorKind;
    use std::os::fd::AsRawFd;
    use std::time::Instant;

    let asked = output.write_all(b"\x1b]11;?\x1b\\\x1b[c");
    if asked.and_then(|()| output.flush()).is_err() {
        return Reply::Silent;
    }
    let fd = input.as_fd().as_raw_fd();
    if fd >= libc::FD_SETSIZE as i32 {
        return Reply::Silent;
    }
    let deadline = Instant::now() + timeout;
    let mut reply = Vec::new();
    loop {
        let text = String::from_utf8_lossy(&reply);
        if let Some(bg) = background_from_osc11(&text) {
            // Read on to the attributes so they do not reach the screen as keys.
            if attributes_answered(&reply) {
                return Reply::Told(bg);
            }
        } else if attributes_answered(&reply) {
            return Reply::Silent;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return background_from_osc11(&text).map_or(Reply::TimedOut, Reply::Told);
        }
        let mut wait = libc::timeval {
            tv_sec: left.as_secs() as libc::time_t,
            tv_usec: left.subsec_micros() as libc::suseconds_t,
        };
        // SAFETY: `set` is a valid fd_set and `fd` is below FD_SETSIZE.
        let ready = unsafe {
            let mut set: libc::fd_set = std::mem::zeroed();
            libc::FD_ZERO(&mut set);
            libc::FD_SET(fd, &mut set);
            libc::select(
                fd + 1,
                &mut set,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut wait,
            )
        };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == ErrorKind::Interrupted {
                continue;
            }
            return Reply::Silent;
        }
        if ready == 0 {
            continue;
        }
        let mut chunk = [0u8; 256];
        // SAFETY: `chunk` is writable for its whole length.
        let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n < 0 {
            match std::io::Error::last_os_error().kind() {
                ErrorKind::Interrupted => continue,
                // Told ready but nothing to read: wait a little and look again.
                ErrorKind::WouldBlock => {
                    std::thread::sleep(left.min(std::time::Duration::from_millis(5)));
                    continue;
                }
                _ => return Reply::Silent,
            }
        }
        if n == 0 {
            let text = String::from_utf8_lossy(&reply);
            return background_from_osc11(&text).map_or(Reply::Silent, Reply::Told);
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

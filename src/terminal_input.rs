//! Reading the terminal: keys, the mouse, pastes, resizes, and the terminal's
//! replies to what moonhare asks it.
//!
//! moonhare asks the terminal for its background (OSC 11) and to report when
//! its colour scheme changes (DEC mode 2031). crossterm does not know these
//! replies and would hand them on as keys (`ESC ]` as Alt-`]`, and so on), so
//! moonhare reads the terminal itself (ADR 0007): the [`Decoder`] takes the
//! replies out of the stream as [`Reply`]s and gives every other byte to
//! crossterm's own parser, vendored in [`keys`], for the very events crossterm
//! would give.
//!
//! A reply is recognised by its start: `ESC ] 11 ;` or `ESC [ ?`, which keys
//! never send. Bytes that may be such a start (`ESC`, `ESC ]`, `ESC ] 1`, …)
//! are held until the next byte decides, or for [`ESC_GAP`] at most, after
//! which they are keys as crossterm reads them. Once a start is certain the
//! reply is read up to its end, however long the pieces take to come (up to
//! [`REPLY_GAP`] between pieces), and is dropped rather than typed if it
//! never ends.

#[allow(clippy::all)]
mod keys;

use std::collections::VecDeque;
use std::io::{self, Read};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{Event, KeyCode};

use crate::theme::{self, Background};

/// Ask for the terminal background (OSC 11); the reply is a
/// [`Reply::Background`].
pub const QUERY_BACKGROUND: &str = "\x1b]11;?\x1b\\";
/// Ask the terminal to report changes of its colour scheme (DEC mode 2031),
/// and for the scheme now (`CSI ? 996 n`). Replies are
/// [`Reply::Background`].
pub const REPORT_THEME: &str = "\x1b[?2031h\x1b[?996n";
/// Stop reporting changes of the colour scheme.
pub const STOP_THEME_REPORTS: &str = "\x1b[?2031l";

/// How long bytes that may start a reply wait for the next one; a lone Esc
/// is late by this much.
pub const ESC_GAP: Duration = Duration::from_millis(25);
/// How long a reply that has surely begun may wait for its next piece.
pub const REPLY_GAP: Duration = Duration::from_secs(1);
/// Longer than any reply moonhare asks for: dropped when reached.
const REPLY_MAX: usize = 128;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const OSC_BACKGROUND: &[u8] = b"\x1b]11;";

/// A reply of the terminal to a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The terminal background: OSC 11, or the colour scheme reported under
    /// DEC mode 2031 (`CSI ? 997 ; 1 n` dark, `2` light).
    Background(Background),
    /// Any other reply (device attributes, a mode report, a background in a
    /// form not known): nothing to do, but not keys either.
    Other,
}

/// What reading the terminal gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Event(Event),
    Reply(Reply),
}

/// What the bytes held so far are.
enum Held {
    /// The start of a reply; `sure` once no key could start so.
    Start { sure: bool },
    /// A whole reply, of its first `len` bytes.
    Reply(Reply, usize),
    /// Not a reply: keys.
    Keys,
}

fn classify(held: &[u8]) -> Held {
    debug_assert_eq!(held.first(), Some(&ESC));
    match held.get(1) {
        None => Held::Start { sure: false },
        Some(b']') => osc(held),
        Some(b'[') => csi(held),
        Some(_) => Held::Keys,
    }
}

fn osc(held: &[u8]) -> Held {
    let start = OSC_BACKGROUND.len();
    if held.len() <= start {
        return if OSC_BACKGROUND.starts_with(held) {
            Held::Start {
                sure: held.len() == start,
            }
        } else {
            Held::Keys
        };
    }
    if !held.starts_with(OSC_BACKGROUND) {
        return Held::Keys;
    }
    let body = |end: usize| {
        let reply = std::str::from_utf8(&held[start..end])
            .ok()
            .and_then(theme::background_from_osc11)
            .map_or(Reply::Other, Reply::Background);
        reply
    };
    let n = held.len();
    match held[n - 1] {
        BEL => Held::Reply(body(n - 1), n),
        b'\\' if held[n - 2] == ESC => Held::Reply(body(n - 2), n),
        // An Esc that does not end the reply starts something else: the reply
        // was cut off.
        _ if held[n - 2] == ESC => Held::Reply(Reply::Other, n - 2),
        _ if n >= REPLY_MAX => Held::Reply(Reply::Other, n),
        _ => Held::Start { sure: true },
    }
}

fn csi(held: &[u8]) -> Held {
    match held.get(2) {
        None => return Held::Start { sure: false },
        Some(b'?') => {}
        Some(_) => return Held::Keys,
    }
    let n = held.len();
    match held[n - 1] {
        _ if n == 3 => Held::Start { sure: true },
        0x20..=0x3f if n < REPLY_MAX => Held::Start { sure: true },
        b'n' => Held::Reply(
            match &held[3..n - 1] {
                b"997;1" => Reply::Background(Background::Dark),
                b"997;2" => Reply::Background(Background::Light),
                _ => Reply::Other,
            },
            n,
        ),
        0x40..=0x7e => Held::Reply(Reply::Other, n),
        // A control byte or an Esc cuts the reply off and is read anew.
        0x00..=0x1f => Held::Reply(Reply::Other, n - 1),
        _ => Held::Reply(Reply::Other, n),
    }
}

/// Turns the bytes read from the terminal into [`Input`]: replies apart,
/// everything else as crossterm reads it.
#[derive(Default)]
pub struct Decoder {
    /// Bytes of a key sequence crossterm's parser waits to complete.
    keys: Vec<u8>,
    /// Bytes that start (or may start) a reply, each with whether more bytes
    /// followed it in its read, which crossterm reads Esc by.
    held: Vec<(u8, bool)>,
    /// When the last held byte was read.
    held_at: Option<Instant>,
    out: VecDeque<Input>,
}

impl Decoder {
    /// Take in what one read of the terminal gave, at `now`.
    pub fn feed(&mut self, bytes: &[u8], now: Instant) {
        for (i, &b) in bytes.iter().enumerate() {
            self.step(b, i + 1 < bytes.len());
        }
        self.held_at = (!self.held.is_empty()).then_some(now);
    }

    /// When held bytes have waited long enough; `None` when none are held.
    pub fn deadline(&self) -> Option<Instant> {
        let at = self.held_at?;
        Some(at + if self.sure() { REPLY_GAP } else { ESC_GAP })
    }

    /// Time has come to `now` without input: held bytes past their
    /// [`deadline`](Self::deadline) are keys, or, when surely a reply, dropped.
    pub fn expire(&mut self, now: Instant) {
        if self.deadline().is_none_or(|due| due > now) {
            return;
        }
        let held = std::mem::take(&mut self.held);
        self.held_at = None;
        if !matches!(classify(&bytes(&held)), Held::Start { sure: true }) {
            for (b, more) in held {
                self.key(b, more);
            }
        }
    }

    fn sure(&self) -> bool {
        !self.held.is_empty() && matches!(classify(&bytes(&self.held)), Held::Start { sure: true })
    }

    fn step(&mut self, b: u8, more: bool) {
        if !self.held.is_empty() {
            self.held.push((b, more));
            return self.decide();
        }
        if b == ESC {
            // An Esc crossterm still waits on is a key of its own: this one
            // may start a reply.
            if self.keys == [ESC] {
                self.keys.clear();
                self.out
                    .push_back(Input::Event(Event::Key(KeyCode::Esc.into())));
            }
            // Inside a pending key sequence (a paste) an Esc is the
            // sequence's.
            if self.keys.is_empty() {
                self.held.push((b, more));
                return;
            }
        }
        self.key(b, more);
    }

    /// The held bytes grew by one: see whether they are a reply yet.
    fn decide(&mut self) {
        match classify(&bytes(&self.held)) {
            Held::Start { .. } => {}
            Held::Reply(reply, len) => {
                let rest = self.held.split_off(len);
                self.held.clear();
                self.out.push_back(Input::Reply(reply));
                for (b, more) in rest {
                    self.step(b, more);
                }
            }
            Held::Keys => {
                // Everything but the last byte could start a reply, and so
                // holds no other Esc; the last byte may begin anew.
                let (last, more) = self.held.pop().expect("held bytes");
                for (b, m) in std::mem::take(&mut self.held) {
                    self.key(b, m);
                }
                self.step(last, more);
            }
        }
    }

    /// One byte to crossterm's parser, as crossterm's reader feeds it.
    fn key(&mut self, b: u8, more: bool) {
        self.keys.push(b);
        match keys::parse_event(&self.keys, more) {
            Ok(Some(event)) => {
                if let keys::InternalEvent::Event(event) = event {
                    self.out.push_back(Input::Event(event));
                }
                self.keys.clear();
            }
            Ok(None) => {}
            Err(_) => self.keys.clear(),
        }
    }
}

/// The inputs made out of what was fed, oldest first.
impl Iterator for Decoder {
    type Item = Input;

    fn next(&mut self) -> Option<Input> {
        self.out.pop_front()
    }
}

fn bytes(held: &[(u8, bool)]) -> Vec<u8> {
    held.iter().map(|(b, _)| *b).collect()
}

enum Chunk {
    Bytes(Vec<u8>, Instant),
    Resize(u16, u16),
    Closed(io::Error),
}

/// The terminal's input, read on a thread of its own.
pub struct TerminalInput {
    chunks: Receiver<Chunk>,
    decoder: Decoder,
}

impl TerminalInput {
    /// Read the terminal moonhare runs in (stdin, or `/dev/tty` when stdin is
    /// not one), and its resizes.
    pub fn start() -> io::Result<TerminalInput> {
        use std::io::IsTerminal;
        let reader: Box<dyn Read + Send> = if io::stdin().is_terminal() {
            Box::new(io::stdin())
        } else {
            Box::new(std::fs::File::open("/dev/tty")?)
        };
        let (tx, chunks) = mpsc::channel();
        watch_resizes(tx.clone())?;
        spawn_reader(reader, tx)?;
        Ok(TerminalInput {
            chunks,
            decoder: Decoder::default(),
        })
    }

    /// Read `reader` as the terminal (for tests).
    pub fn from_reader(reader: impl Read + Send + 'static) -> TerminalInput {
        let (tx, chunks) = mpsc::channel();
        spawn_reader(Box::new(reader), tx).expect("spawn the input reader");
        TerminalInput {
            chunks,
            decoder: Decoder::default(),
        }
    }

    /// The next input, waiting `timeout` at most. `None` when none came; an
    /// error when the terminal can no longer be read.
    pub fn next(&mut self, timeout: Duration) -> io::Result<Option<Input>> {
        let end = Instant::now() + timeout;
        loop {
            if let Some(input) = self.decoder.next() {
                return Ok(Some(input));
            }
            let now = Instant::now();
            if self.decoder.deadline().is_some_and(|due| due <= now) {
                self.decoder.expire(now);
                continue;
            }
            let until = self.decoder.deadline().map_or(end, |due| due.min(end));
            if until <= now {
                return Ok(None);
            }
            match self.chunks.recv_timeout(until - now) {
                Ok(Chunk::Bytes(bytes, at)) => self.decoder.feed(&bytes, at),
                Ok(Chunk::Resize(w, h)) => return Ok(Some(Input::Event(Event::Resize(w, h)))),
                Ok(Chunk::Closed(e)) => return Err(e),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "the terminal is closed",
                    ))
                }
            }
        }
    }
}

fn spawn_reader(mut reader: Box<dyn Read + Send>, tx: Sender<Chunk>) -> io::Result<()> {
    std::thread::Builder::new()
        .name("moonhare-input".into())
        .spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                let chunk = match reader.read(&mut buf) {
                    Ok(0) => Chunk::Closed(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "the terminal is closed",
                    )),
                    Ok(n) => Chunk::Bytes(buf[..n].to_vec(), Instant::now()),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => Chunk::Closed(e),
                };
                let closed = matches!(chunk, Chunk::Closed(_));
                if tx.send(chunk).is_err() || closed {
                    return;
                }
            }
        })?;
    Ok(())
}

fn watch_resizes(tx: Sender<Chunk>) -> io::Result<()> {
    let mut signals = signal_hook::iterator::Signals::new([signal_hook::consts::SIGWINCH])?;
    std::thread::Builder::new()
        .name("moonhare-resize".into())
        .spawn(move || {
            for _ in signals.forever() {
                let Ok((w, h)) = ratatui::crossterm::terminal::size() else {
                    continue;
                };
                if tx.send(Chunk::Resize(w, h)).is_err() {
                    return;
                }
            }
        })?;
    Ok(())
}

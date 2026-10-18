//! Reading the terminal: keys, the mouse, pastes, resizes, and the terminal's
//! replies to what moonhare asks it.
//!
//! moonhare asks the terminal for its background (OSC 11) and to report when
//! its colour scheme changes (DEC mode 2031). crossterm does not know these
//! replies and would hand them on as keys (`ESC ]` as Alt-`]`, and so on), so
//! moonhare reads the terminal itself (ADR 0007): the [`Decoder`] takes the
//! replies out of the stream as [`Reply`]s and gives every other byte to
//! crossterm's own parser, vendored in `keys`, for the very events crossterm
//! would give.
//!
//! A reply is recognised by its start: `ESC ] 11 ;` or `ESC [ ?`, which keys
//! never send. Bytes that may be such a start (`ESC`, `ESC ]`, `ESC ] 1`, …)
//! are held until the next byte decides, or for [`ESC_GAP`] at most, after
//! which they are keys as crossterm reads them. Once a start is certain the
//! reply is read up to its end (BEL, ST or 8-bit ST; the final byte of the
//! CSI), however its pieces come, for `REPLY_GAP` from its start at most. A
//! byte no such reply holds (a typed `j`, Enter, Ctrl-C) cuts it off and is
//! read anew; a reply cut off, too long or never ending is dropped, never
//! typed.

#[allow(clippy::all)]
mod keys;

use std::collections::VecDeque;
use std::io::{self, Read};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
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
/// How long a reply that has surely begun may take, from its start.
const REPLY_GAP: Duration = Duration::from_secs(1);
/// Longer than any reply moonhare asks for: dropped whole, up to its end.
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
#[derive(Default)]
enum Held {
    #[default]
    Nothing,
    /// Bytes that may start a reply (`ESC`, `ESC ]`, `ESC ] 1`, `ESC ] 11`,
    /// `ESC [`), each with whether more bytes followed it in its read, which
    /// crossterm reads Esc by.
    Maybe(Vec<(u8, bool)>),
    /// Surely the background, after `ESC ] 11 ;`: its body so far (up to
    /// [`REPLY_MAX`] bytes) and its length; with whether more bytes followed
    /// an Esc that may begin its ST.
    Osc {
        body: Vec<u8>,
        len: usize,
        esc: Option<bool>,
    },
    /// Surely a report, after `ESC [ ?`: its parameters so far and their
    /// length.
    Csi { body: Vec<u8>, len: usize },
}

/// What bytes that may start a reply are.
enum Start {
    Maybe,
    Osc,
    Csi,
    Keys,
}

fn start(held: &[(u8, bool)]) -> Start {
    let bytes: Vec<u8> = held.iter().map(|(b, _)| *b).collect();
    match bytes.get(1) {
        None => Start::Maybe,
        Some(b']') if bytes == OSC_BACKGROUND => Start::Osc,
        Some(b']') if OSC_BACKGROUND.starts_with(&bytes) => Start::Maybe,
        Some(b'[') if bytes.len() == 2 => Start::Maybe,
        Some(b'[') if bytes[2] == b'?' => Start::Csi,
        _ => Start::Keys,
    }
}

/// A byte the body of a background reply may hold: `rgb:`/`rgba:` and hex
/// channels, or `#` for a form not known. ASCII only, so an 8-bit ST is
/// never part of a UTF-8 character here.
fn osc_body(b: u8) -> bool {
    b.is_ascii_hexdigit() || matches!(b, b'r' | b'g' | b'a' | b':' | b'/' | b'#')
}

/// The 8-bit String Terminator.
const ST: u8 = 0x9c;

/// Turns the bytes read from the terminal into [`Input`]: replies apart,
/// everything else as crossterm reads it.
#[derive(Default)]
pub struct Decoder {
    /// Bytes of a key sequence crossterm's parser waits to complete.
    keys: Vec<u8>,
    held: Held,
    /// When the bytes held began, and when the last of them was read.
    held_since: Option<Instant>,
    held_last: Option<Instant>,
    /// When the read being taken in was made.
    now: Option<Instant>,
    out: VecDeque<Input>,
}

impl Decoder {
    /// Take in what one read of the terminal gave, at `now`.
    pub fn feed(&mut self, bytes: &[u8], now: Instant) {
        self.now = Some(now);
        for (i, &b) in bytes.iter().enumerate() {
            self.step(b, i + 1 < bytes.len());
        }
        if matches!(self.held, Held::Nothing) {
            self.held_since = None;
            self.held_last = None;
        } else {
            self.held_last = Some(now);
        }
    }

    /// When held bytes have waited long enough; `None` when none are held.
    /// Bytes that may start a reply wait [`ESC_GAP`] for the next one; a reply
    /// that surely began has [`REPLY_GAP`] from its start to end.
    pub fn deadline(&self) -> Option<Instant> {
        match self.held {
            Held::Nothing => None,
            Held::Maybe(_) => Some(self.held_last? + ESC_GAP),
            Held::Osc { .. } | Held::Csi { .. } => Some(self.held_since? + REPLY_GAP),
        }
    }

    /// Time has come to `now` without input: held bytes past their
    /// [`deadline`](Self::deadline) are keys, or, when surely a reply, dropped.
    pub fn expire(&mut self, now: Instant) {
        if self.deadline().is_none_or(|due| due > now) {
            return;
        }
        if let Held::Maybe(held) = self.release() {
            for (b, more) in held {
                self.key(b, more);
            }
        }
    }

    /// Stop holding; what was held.
    fn release(&mut self) -> Held {
        self.held_since = None;
        self.held_last = None;
        std::mem::take(&mut self.held)
    }

    fn hold(&mut self, held: Held) {
        if matches!(self.held, Held::Nothing) {
            self.held_since = self.now;
        }
        self.held = held;
    }

    /// A reply ended (or was cut off, as `reply` `Other`).
    fn reply(&mut self, reply: Reply) {
        self.release();
        self.out.push_back(Input::Reply(reply));
    }

    fn step(&mut self, b: u8, more: bool) {
        match &mut self.held {
            Held::Nothing => {}
            Held::Maybe(held) => {
                held.push((b, more));
                return self.decide();
            }
            Held::Osc { body, len, esc } => {
                match (*esc, b) {
                    (Some(_), b'\\') | (None, BEL | ST) => {
                        let reply = if *len > REPLY_MAX {
                            Reply::Other
                        } else {
                            std::str::from_utf8(body)
                                .ok()
                                .and_then(theme::background_from_osc11)
                                .map_or(Reply::Other, Reply::Background)
                        };
                        self.reply(reply);
                    }
                    (None, ESC) => *esc = Some(more),
                    (None, b) if osc_body(b) => {
                        *len += 1;
                        if body.len() < REPLY_MAX {
                            body.push(b);
                        }
                    }
                    // Anything else cuts the reply off and is read anew: an
                    // Esc that does not end it starts something else.
                    (Some(esc_more), b) => {
                        self.reply(Reply::Other);
                        self.step(ESC, esc_more);
                        self.step(b, more);
                    }
                    (None, b) => {
                        self.reply(Reply::Other);
                        self.step(b, more);
                    }
                }
                return;
            }
            Held::Csi { body, len } => {
                match b {
                    0x20..=0x3f => {
                        *len += 1;
                        if body.len() < REPLY_MAX {
                            body.push(b);
                        }
                    }
                    0x40..=0x7e => {
                        let reply = match (b, &body[..]) {
                            (b'n', b"997;1") => Reply::Background(Background::Dark),
                            (b'n', b"997;2") => Reply::Background(Background::Light),
                            _ => Reply::Other,
                        };
                        self.reply(reply);
                    }
                    // A control byte (Ctrl-C, Enter, an Esc) cuts the report
                    // off and is read anew.
                    _ => {
                        self.reply(Reply::Other);
                        self.step(b, more);
                    }
                }
                return;
            }
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
                return self.hold(Held::Maybe(vec![(b, more)]));
            }
        }
        // `ESC [` timed out into crossterm's parser, and a report goes on:
        // crossterm would wait for its end for ever.
        if b == b'?' && self.keys == b"\x1b[" {
            self.keys.clear();
            return self.hold(Held::Csi {
                body: Vec::new(),
                len: 0,
            });
        }
        self.key(b, more);
    }

    /// The bytes that may start a reply grew by one: see what they are.
    fn decide(&mut self) {
        let Held::Maybe(held) = &self.held else {
            return;
        };
        match start(held) {
            Start::Maybe => {}
            Start::Osc => {
                self.held = Held::Osc {
                    body: Vec::new(),
                    len: 0,
                    esc: None,
                }
            }
            Start::Csi => {
                self.held = Held::Csi {
                    body: Vec::new(),
                    len: 0,
                }
            }
            Start::Keys => {
                let Held::Maybe(mut held) = self.release() else {
                    unreachable!()
                };
                // Everything but the last byte could start a reply, and so
                // holds no other Esc; the last byte may begin anew.
                let (last, more) = held.pop().expect("held bytes");
                for (b, m) in held {
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
        Ok(TerminalInput::reading(chunks))
    }

    /// Read `reader` as the terminal (for tests).
    pub fn from_reader(reader: impl Read + Send + 'static) -> TerminalInput {
        let (tx, chunks) = mpsc::channel();
        spawn_reader(Box::new(reader), tx).expect("spawn the input reader");
        TerminalInput::reading(chunks)
    }

    fn reading(chunks: Receiver<Chunk>) -> TerminalInput {
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
            // What was read while the loop was busy comes first: held bytes
            // time out only when nothing read follows them.
            match self.chunks.try_recv() {
                Ok(chunk) => {
                    if let Some(result) = self.take(chunk) {
                        return result;
                    }
                    continue;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => return Err(closed()),
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
                Ok(chunk) => {
                    if let Some(result) = self.take(chunk) {
                        return result;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err(closed()),
            }
        }
    }

    /// Take in a chunk read; what to return at once, if anything.
    fn take(&mut self, chunk: Chunk) -> Option<io::Result<Option<Input>>> {
        match chunk {
            Chunk::Bytes(bytes, at) => {
                self.decoder.feed(&bytes, at);
                None
            }
            Chunk::Resize(w, h) => Some(Ok(Some(Input::Event(Event::Resize(w, h))))),
            Chunk::Closed(e) => Some(Err(e)),
        }
    }
}

fn closed() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "the terminal is closed")
}

fn spawn_reader(mut reader: Box<dyn Read + Send>, tx: Sender<Chunk>) -> io::Result<()> {
    std::thread::Builder::new()
        .name("moonhare-input".into())
        .spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                let chunk = match reader.read(&mut buf) {
                    Ok(0) => Chunk::Closed(closed()),
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

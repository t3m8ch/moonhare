//! Reading the terminal: replies to moonhare's queries (OSC 11, DEC 2031,
//! device attributes) come out as replies, never as keys, however they are
//! cut into reads and whenever they come; every other byte gives the events
//! crossterm would give, as soon as it would.

use std::time::{Duration, Instant};

use moonhare::terminal_input::{Decoder, Input, Reply, TerminalInput, ESC_GAP};
use moonhare::theme::Background;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

const LIGHT_ST: &[u8] = b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\";
const DARK_BEL: &[u8] = b"\x1b]11;rgb:1c1c/1c1c/1c1c\x07";

fn key(c: char) -> Input {
    Input::Event(Event::Key(KeyEvent::from(KeyCode::Char(c))))
}

fn alt(c: char) -> Input {
    Input::Event(Event::Key(KeyEvent::new(
        KeyCode::Char(c),
        KeyModifiers::ALT,
    )))
}

fn code(c: KeyCode) -> Input {
    Input::Event(Event::Key(KeyEvent::from(c)))
}

fn background(b: Background) -> Input {
    Input::Reply(Reply::Background(b))
}

/// Feed `chunks` (each read at its millisecond), letting held bytes time out
/// on the way, as the live loop does, and then wait until `end`.
fn decode_at(chunks: &[(u64, &[u8])], end: u64) -> Vec<Input> {
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let mut d = Decoder::default();
    let mut out = Vec::new();
    let expire_until = |d: &mut Decoder, now: Instant, out: &mut Vec<Input>| {
        while let Some(due) = d.deadline().filter(|due| *due <= now) {
            d.expire(due);
            out.extend(std::iter::from_fn(|| d.next()));
        }
    };
    for (ms, bytes) in chunks {
        expire_until(&mut d, at(*ms), &mut out);
        d.feed(bytes, at(*ms));
        out.extend(std::iter::from_fn(|| d.next()));
    }
    expire_until(&mut d, at(end), &mut out);
    assert_eq!(d.deadline(), None, "nothing is held for ever");
    out
}

/// Everything read at once.
fn decode(bytes: &[u8]) -> Vec<Input> {
    decode_at(&[(0, bytes)], 10_000)
}

/// `bytes` read in pieces cut at `cuts`, `gap` milliseconds apart.
fn decode_cut(bytes: &[u8], cuts: &[usize], gap: u64) -> Vec<Input> {
    let mut chunks = Vec::new();
    let mut from = 0;
    for (i, &to) in cuts.iter().chain([bytes.len()].iter()).enumerate() {
        chunks.push((i as u64 * gap, &bytes[from..to]));
        from = to;
    }
    decode_at(&chunks, 10_000)
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

// ------------------------------------------------------------ replies

#[test]
fn a_background_reply_among_keys_is_a_reply() {
    let bytes = cat(&[b"j", LIGHT_ST, b"k"]);
    assert_eq!(
        decode(&bytes),
        vec![key('j'), background(Background::Light), key('k')]
    );
    let bytes = cat(&[b"j", DARK_BEL, b"k"]);
    assert_eq!(
        decode(&bytes),
        vec![key('j'), background(Background::Dark), key('k')]
    );
}

#[test]
fn a_background_reply_cut_anywhere_is_still_a_reply() {
    let bytes = cat(&[b"j", LIGHT_ST, b"k"]);
    let want = vec![key('j'), background(Background::Light), key('k')];
    for cut in 1..bytes.len() {
        // Pieces of one write come microseconds apart …
        assert_eq!(decode_cut(&bytes, &[cut], 0), want, "cut at {cut}");
        // … and still a reply when the network holds the rest back.
        assert_eq!(decode_cut(&bytes, &[cut], 20), want, "cut at {cut}, 20 ms");
    }
    for cut in 6..bytes.len() {
        assert_eq!(
            decode_cut(&bytes, &[cut], 400),
            want,
            "cut at {cut}, 400 ms"
        );
    }
}

#[test]
fn a_reply_read_byte_by_byte_is_a_reply() {
    let bytes = cat(&[b"j", DARK_BEL, b"k"]);
    let cuts: Vec<usize> = (1..bytes.len()).collect();
    let want = vec![key('j'), background(Background::Dark), key('k')];
    assert_eq!(decode_cut(&bytes, &cuts, 0), want);
    assert_eq!(decode_cut(&bytes, &cuts, 5), want);
}

#[test]
fn a_late_reply_after_idle_time_is_a_reply() {
    let out = decode_at(&[(0, b"jj"), (2_000, LIGHT_ST), (2_100, b":q")], 5_000);
    assert_eq!(
        out,
        vec![
            key('j'),
            key('j'),
            background(Background::Light),
            key(':'),
            key('q')
        ]
    );
}

#[test]
fn the_theme_mode_notification_of_dec_2031_is_a_reply() {
    assert_eq!(
        decode(b"\x1b[?997;1nj\x1b[?997;2n"),
        vec![
            background(Background::Dark),
            key('j'),
            background(Background::Light)
        ]
    );
    let bytes = b"\x1b[?997;2n";
    for cut in 1..bytes.len() {
        assert_eq!(
            decode_cut(bytes, &[cut], 20),
            vec![background(Background::Light)],
            "cut at {cut}"
        );
    }
}

#[test]
fn other_replies_are_dropped_not_typed() {
    // Device attributes, a mode report, a background in a form not known.
    for bytes in [
        &b"\x1b[?62;22c"[..],
        b"\x1b[?2031;2$y",
        b"\x1b]11;#ffffff\x07",
        b"\x1b]11;rgb:zz/zz/zz\x1b\\",
    ] {
        let out = decode(&cat(&[b"j", bytes, b"k"]));
        assert_eq!(
            out,
            vec![key('j'), Input::Reply(Reply::Other), key('k')],
            "{bytes:?}"
        );
    }
}

#[test]
fn a_reply_cut_off_for_good_is_dropped_and_typing_goes_on() {
    let out = decode_at(&[(0, b"\x1b]11;rgb:ffff/ff"), (3_000, b"jk")], 5_000);
    assert_eq!(out, vec![key('j'), key('k')]);
}

#[test]
fn a_rgba_background_is_understood() {
    assert_eq!(
        decode(b"\x1b]11;rgba:ffff/ffff/ffff/ffff\x07"),
        vec![background(Background::Light)]
    );
    assert_eq!(
        decode(b"\x1b]11;rgb:0/0/0\x07"),
        vec![background(Background::Dark)]
    );
}

#[test]
fn esc_just_before_a_reply_is_still_esc() {
    assert_eq!(
        decode(&cat(&[b"\x1b", LIGHT_ST])),
        vec![code(KeyCode::Esc), background(Background::Light)]
    );
}

// ------------------------------------------------------------ keys

#[test]
fn esc_then_bracket_with_any_pause_is_keys() {
    // tmux joins Esc and `]` typed within `escape-time` into one read.
    assert_eq!(decode(b"\x1b]h"), vec![alt(']'), key('h')]);
    assert_eq!(
        decode_at(&[(0, b"\x1b]"), (10, b"h")], 1_000),
        vec![alt(']'), key('h')]
    );
    assert_eq!(
        decode_at(&[(0, b"\x1b]"), (300, b"h")], 1_000),
        vec![alt(']'), key('h')]
    );
    // Esc, then `]` on its own.
    for gap in [5, 20, 100, 700] {
        assert_eq!(
            decode_at(&[(0, b"\x1b"), (gap, b"]"), (2 * gap, b"h")], 5_000),
            vec![code(KeyCode::Esc), key(']'), key('h')],
            "{gap} ms"
        );
    }
    // A count after the bracket.
    assert_eq!(decode(b"\x1b]1j"), vec![alt(']'), key('1'), key('j')]);
    assert_eq!(
        decode(b"\x1b]11j"),
        vec![alt(']'), key('1'), key('1'), key('j')]
    );
}

#[test]
fn esc_and_alt_keys_are_what_crossterm_makes_of_them() {
    assert_eq!(decode(b"\x1b"), vec![code(KeyCode::Esc)]);
    assert_eq!(decode(b"\x1bj"), vec![alt('j')]);
    assert_eq!(
        decode_at(&[(0, b"\x1b"), (10, b"j")], 1_000),
        vec![code(KeyCode::Esc), key('j')]
    );
    // Each Esc may start a reply, so two in one read are two Escs.
    assert_eq!(
        decode(b"\x1b\x1b"),
        vec![code(KeyCode::Esc), code(KeyCode::Esc)]
    );
    assert_eq!(decode(b"j\x1b[A\x1b[1;3B"), {
        vec![
            key('j'),
            code(KeyCode::Up),
            Input::Event(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT))),
        ]
    });
}

#[test]
fn a_lone_esc_waits_only_a_moment() {
    let t0 = Instant::now();
    let mut d = Decoder::default();
    d.feed(b"\x1b", t0);
    assert_eq!(d.next(), None);
    assert_eq!(d.deadline(), Some(t0 + ESC_GAP));
    assert!(ESC_GAP <= Duration::from_millis(30));
    d.expire(t0 + ESC_GAP);
    assert_eq!(d.next(), Some(code(KeyCode::Esc)));
    // Keys that cannot start a reply are never held.
    d.feed(b"j\x1b[A\x1b[<0;3;4M:", t0);
    assert_eq!(d.deadline(), None);
}

#[test]
fn the_mouse_and_a_paste_come_through() {
    assert_eq!(
        decode(b"\x1b[<0;10;5M"),
        vec![Input::Event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 9,
            row: 4,
            modifiers: KeyModifiers::empty(),
        }))]
    );
    // A pasted reply is pasted text.
    let paste = cat(&[b"\x1b[200~a", LIGHT_ST, b"b\x1b[201~"]);
    let want = vec![Input::Event(Event::Paste(
        "a\x1b]11;rgb:ffff/ffff/ffff\x1b\\b".to_string(),
    ))];
    assert_eq!(decode(&paste), want);
    // Cut anywhere inside it (crossterm too reads a paste whose start is cut
    // right after its Esc as Esc and characters).
    let cuts: Vec<usize> = (6..paste.len()).collect();
    assert_eq!(decode_cut(&paste, &cuts, 1), want);
}

#[test]
fn keys_and_replies_mixed_in_every_way() {
    let bytes = cat(&[
        b"5j",
        LIGHT_ST,
        b"\x1b[<0;2;2M",
        b"\x1b[?997;1n",
        b"\x1b]h",
        b"\x1b[?62;22c",
        DARK_BEL,
        b"\x1b[200~x\x1b[201~",
        b"\x1bq",
    ]);
    let want = vec![
        key('5'),
        key('j'),
        background(Background::Light),
        Input::Event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: 1,
            modifiers: KeyModifiers::empty(),
        })),
        background(Background::Dark),
        alt(']'),
        key('h'),
        Input::Reply(Reply::Other),
        background(Background::Dark),
        Input::Event(Event::Paste("x".to_string())),
        alt('q'),
    ];
    assert_eq!(decode(&bytes), want);
    for cut in 1..bytes.len() {
        // A key sequence cut right after its Esc is Esc and then characters,
        // as crossterm has it: only replies are put back together.
        let after_esc = bytes[cut - 1] == 0x1b;
        let reply = bytes[cut..].starts_with(b"]11;") || bytes[cut..].starts_with(b"[?");
        if after_esc && !reply {
            continue;
        }
        assert_eq!(decode_cut(&bytes, &[cut], 0), want, "cut at {cut}");
    }
}

// ------------------------------------------------------------ live

#[test]
fn the_live_input_reads_a_pipe_and_resolves_a_lone_esc_without_more_input() {
    let (reader, mut writer) = std::io::pipe().unwrap();
    let mut input = TerminalInput::from_reader(reader);
    use std::io::Write;
    writer.write_all(b"j").unwrap();
    let wait = Duration::from_secs(5);
    assert_eq!(input.next(wait).unwrap(), Some(key('j')));
    assert_eq!(input.next(Duration::from_millis(50)).unwrap(), None);
    writer.write_all(LIGHT_ST).unwrap();
    assert_eq!(
        input.next(wait).unwrap(),
        Some(background(Background::Light))
    );
    writer.write_all(b"\x1b").unwrap();
    assert_eq!(input.next(wait).unwrap(), Some(code(KeyCode::Esc)));
    drop(writer);
    assert!(input.next(wait).is_err(), "the end of input is an error");
}

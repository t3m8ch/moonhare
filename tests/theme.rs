//! Themes: the dark and the light palette, telling the terminal background,
//! and switching with `:theme`.

mod support;

use moonhare::session::Session;
use moonhare::theme::{self, Background, Reply, Theme, DARK, LIGHT};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use support::{feature_branch, Harness};

// ---------------------------------------------------------------- contrast

/// The pairs of the theme whose contrast has to hold: a role, the colour of
/// the text, the colour under it and the least contrast.
fn pairs(t: &Theme) -> Vec<(String, Color, Color, f64)> {
    const TEXT: f64 = 4.5;
    const MUTED: f64 = 3.0;
    let s = &t.syntax;
    let code = [
        ("fg", t.fg),
        ("syntax.keyword", s.keyword),
        ("syntax.string", s.string),
        ("syntax.type", s.type_),
        ("syntax.error", s.error),
        ("syntax.delimiter", s.delimiter),
    ];
    let code_bgs = [
        ("terminal", Color::Reset),
        ("added", t.added_bg),
        ("removed", t.removed_bg),
        ("added fragment", t.added_fragment_bg),
        ("removed fragment", t.removed_fragment_bg),
    ];
    let list_bgs = [
        ("terminal", Color::Reset),
        ("cursor", t.cursor_bg),
        ("selection", t.selection_bg),
    ];
    let mut out = Vec::new();
    let mut add = |role: &str, fg: Color, bgs: &[(&str, Color)], min: f64| {
        for (name, bg) in bgs {
            out.push((format!("{role} on {name}"), fg, *bg, min));
        }
    };
    for (role, fg) in code {
        add(role, fg, &code_bgs, TEXT);
    }
    add("syntax.comment", s.comment, &code_bgs, MUTED);
    // The gutter: line numbers on the line's background.
    add("dim", t.dim, &code_bgs[..3], MUTED);
    add("dim", t.dim, &list_bgs, MUTED);
    add("dim", t.dim, &[("fold", t.fold_bg)], MUTED);
    add("fg", t.fg, &list_bgs[1..], TEXT);
    add("accent", t.accent, &code_bgs[..3], TEXT);
    add("accent", t.accent, &list_bgs, TEXT);
    add("accent", t.accent, &[("fold", t.fold_bg)], TEXT);
    for (role, fg) in [
        ("status.added", t.status_added),
        ("status.deleted", t.status_deleted),
        ("status.modified", t.status_modified),
        ("status.renamed", t.status_renamed),
        ("directory", t.directory),
    ] {
        add(role, fg, &list_bgs[..2], TEXT);
    }
    add("fg", t.fg, &[("bar", t.bar_bg)], TEXT);
    add("header", t.header_fg, &[("bar", t.bar_bg)], TEXT);
    add("on accent", t.on_accent, &[("accent", t.accent)], TEXT);
    add("on accent", t.on_accent, &[("comment", t.comment)], TEXT);
    add("comment", t.comment, &list_bgs[..1], TEXT);
    add("draft", t.draft, &list_bgs[..1], TEXT);
    add("error", t.error, &list_bgs[..1], TEXT);
    let md = &t.markdown;
    add("markdown.heading", md.heading, &list_bgs[..1], TEXT);
    add("markdown.code", md.code, &[("code", md.code_bg)], TEXT);
    add("fg", t.fg, &[("code block", md.block_bg)], TEXT);
    out
}

fn failures(t: &Theme) -> Vec<String> {
    pairs(t)
        .into_iter()
        .filter_map(|(role, fg, bg, min)| {
            let c = t.contrast(fg, bg);
            (c < min).then(|| format!("{role}: {c:.2} < {min}"))
        })
        .collect()
}

#[test]
fn every_role_of_the_light_theme_reads_on_its_background() {
    let failed = failures(&LIGHT);
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Light terminals are not all pure white: Solarized Light, a grey-white.
#[test]
fn the_light_theme_reads_on_off_white_terminals_too() {
    for bg in [(0xfd, 0xf6, 0xe3), (0xf5, 0xf5, 0xf5)] {
        let t = Theme {
            terminal_bg: bg,
            ..LIGHT
        };
        let failed = failures(&t);
        assert!(failed.is_empty(), "{bg:x?}: {failed:#?}");
    }
}

/// The dark theme keeps the look it has always had. Where that look falls
/// short of the bar, the shortfall is listed here, so it can only shrink.
#[test]
fn the_dark_theme_reads_on_its_background_but_for_the_known_old_spots() {
    let known = [
        "fg on added fragment",
        "syntax.keyword on added",
        "syntax.keyword on added fragment",
        "syntax.keyword on removed fragment",
        "syntax.string on added fragment",
        "syntax.type on added",
        "syntax.type on added fragment",
        "syntax.error on added",
        "syntax.error on added fragment",
        "syntax.error on removed fragment",
        "syntax.delimiter on added",
        "syntax.delimiter on added fragment",
        "syntax.delimiter on removed fragment",
        "syntax.comment on added",
        "syntax.comment on added fragment",
        "syntax.comment on removed fragment",
        "dim on added",
        "dim on cursor",
        "dim on selection",
        "accent on added",
        "accent on cursor",
        "accent on selection",
        "status.deleted on cursor",
        "status.renamed on cursor",
        "directory on cursor",
        "error on terminal",
    ];
    let failed = failures(&DARK);
    let role = |f: &String| f[..f.find(':').unwrap()].to_string();
    let unknown: Vec<_> = failed
        .iter()
        .filter(|f| !known.contains(&role(f).as_str()))
        .collect();
    assert!(unknown.is_empty(), "{unknown:#?}");
    let fixed: Vec<_> = known
        .iter()
        .filter(|k| !failed.iter().any(|f| role(f) == **k))
        .collect();
    assert!(
        fixed.is_empty(),
        "no longer short, drop from the list: {fixed:?}"
    );
}

#[test]
fn contrast_is_the_wcag_ratio() {
    let white = Color::Indexed(231);
    let black = Color::Indexed(16);
    assert!((LIGHT.contrast(black, white) - 21.0).abs() < 0.01);
    assert!((LIGHT.contrast(white, white) - 1.0).abs() < 0.01);
    // The terminal background is the theme's own.
    assert!((LIGHT.contrast(black, Color::Reset) - 21.0).abs() < 0.01);
    assert!((DARK.contrast(white, Color::Reset) - 21.0).abs() < 0.01);
}

// -------------------------------------------------------------- detection

#[test]
fn an_osc_11_reply_tells_light_from_dark() {
    use theme::background_from_osc11 as osc;
    assert_eq!(
        osc("\x1b]11;rgb:ffff/ffff/ffff\x1b\\"),
        Some(Background::Light)
    );
    assert_eq!(
        osc("\x1b]11;rgb:0000/0000/0000\x07"),
        Some(Background::Dark)
    );
    // Short components and a solarized-light background.
    assert_eq!(osc("\x1b]11;rgb:fd/f6/e3\x07"), Some(Background::Light));
    assert_eq!(
        osc("\x1b]11;rgb:1e1e/1e1e/2e2e\x07"),
        Some(Background::Dark)
    );
    assert_eq!(
        osc("\x1b]11;rgba:ffff/ffff/ffff/ffff\x07"),
        Some(Background::Light)
    );
    assert_eq!(osc(""), None);
    assert_eq!(osc("\x1b[?62;c"), None);
    assert_eq!(osc("\x1b]11;rgb:zz/00/00\x07"), None);
}

#[test]
fn colorfgbg_tells_light_from_dark() {
    use theme::background_from_colorfgbg as fgbg;
    assert_eq!(fgbg("15;0"), Some(Background::Dark));
    assert_eq!(fgbg("0;15"), Some(Background::Light));
    assert_eq!(fgbg("0;7"), Some(Background::Light));
    assert_eq!(fgbg("15;default;0"), Some(Background::Dark));
    assert_eq!(fgbg("12;8"), Some(Background::Dark));
    assert_eq!(fgbg("default;default"), None);
    assert_eq!(fgbg(""), None);
}

#[cfg(unix)]
#[test]
fn the_query_reads_the_reply_of_the_terminal() {
    use std::io::Write;
    let (reader, mut writer) = std::io::pipe().unwrap();
    writer
        .write_all(b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\\x1b[?62;22c")
        .unwrap();
    let mut asked = Vec::new();
    let got =
        theme::query_background_on(&reader, &mut asked, std::time::Duration::from_millis(500));
    assert_eq!(got, Reply::Told(Background::Light));
    let asked = String::from_utf8(asked).unwrap();
    assert!(asked.contains("\x1b]11;?"), "{asked:?}");
}

#[cfg(unix)]
#[test]
fn a_silent_terminal_holds_the_start_no_longer_than_the_timeout() {
    let (reader, _writer) = std::io::pipe().unwrap();
    let started = std::time::Instant::now();
    let got = theme::query_background_on(
        &reader,
        &mut Vec::new(),
        std::time::Duration::from_millis(50),
    );
    assert_eq!(got, Reply::TimedOut);
    assert!(started.elapsed() < std::time::Duration::from_millis(400));
}

#[cfg(unix)]
#[test]
fn a_terminal_without_osc_11_answers_only_the_attributes_and_is_not_waited_for() {
    use std::io::Write;
    let (reader, mut writer) = std::io::pipe().unwrap();
    writer.write_all(b"\x1b[?62;22c").unwrap();
    let started = std::time::Instant::now();
    let got =
        theme::query_background_on(&reader, &mut Vec::new(), std::time::Duration::from_secs(5));
    assert_eq!(got, Reply::Silent);
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn a_theme_is_parsed_from_its_name() {
    use theme::Choice;
    assert_eq!("light".parse::<Choice>(), Ok(Choice::Light));
    assert_eq!("dark".parse::<Choice>(), Ok(Choice::Dark));
    assert_eq!("auto".parse::<Choice>(), Ok(Choice::Auto));
    let err = "purple".parse::<Choice>().unwrap_err();
    assert!(err.contains("purple") && err.contains("light"), "{err}");
}

// ------------------------------------------------------------------ frames

/// The first cell of the row holding `needle`, and the cell where it starts.
fn cell(h: &mut Harness, needle: &str) -> (u16, u16) {
    h.frame()
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not on screen:\n{}", h.screen()))
}

fn open_light(fx: &support::Fixture) -> Harness {
    let mut session = Session::open(fx.path(), 160, 30).unwrap();
    session.set_terminal_background(Some(Background::Light));
    let mut h = Harness::from_session(session);
    h.settle();
    h
}

#[test]
fn on_a_light_terminal_the_screen_is_drawn_with_the_light_palette() {
    let fx = feature_branch();
    let mut h = open_light(&fx);
    // `src/a.rs`: an added file, every line on the added background.
    h.keys("]f");
    h.settle();
    let frame = h.frame();
    let (x, y) = frame
        .find("fn a()")
        .unwrap_or_else(|| panic!("{}", frame.text()));
    // The code of a new file is all changed fragments; the gutter has the
    // line's background.
    assert_eq!(frame.bg_at(x, y), LIGHT.added_fragment_bg);
    assert_eq!(frame.bg_at(x - 1, y), LIGHT.added_bg);
    assert_eq!(
        frame.fg_at(x - 2, y),
        LIGHT.accent,
        "the cursor is on line 1"
    );
    assert_eq!(frame.fg_at(x, y), LIGHT.syntax.keyword);
    let (x, y) = frame.find("add a").expect("commit row");
    assert_eq!(frame.fg_at(x, y), LIGHT.fg);
    let (x, y) = frame.find("README.md").expect("tree row");
    assert_eq!(frame.fg_at(x, y), LIGHT.fg);
    let (x, y) = frame.find("Commits").expect("box");
    assert!([LIGHT.dim, LIGHT.accent].contains(&frame.fg_at(x - 1, y)));
    // Auto: nothing in the status bar names the theme.
    assert!(!h.screen().contains("theme:"), "{}", h.screen());
}

#[test]
fn the_theme_command_switches_on_the_fly_and_shows_the_choice() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.settle();
    let (x, y) = cell(&mut h, "add a");
    assert_eq!(h.frame().fg_at(x, y), DARK.fg);

    h.keys(":theme light<CR>");
    assert_eq!(h.frame().fg_at(x, y), LIGHT.fg);
    assert!(h.screen().contains("theme:light"), "{}", h.screen());

    h.keys(":theme dark<CR>");
    assert_eq!(h.frame().fg_at(x, y), DARK.fg);
    assert!(h.screen().contains("theme:dark"), "{}", h.screen());

    h.keys(":theme auto<CR>");
    assert_eq!(h.frame().fg_at(x, y), DARK.fg);
    assert!(!h.screen().contains("theme:"), "{}", h.screen());
}

#[test]
fn an_unknown_theme_is_refused_with_a_message() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys(":theme purple<CR>");
    let screen = h.screen();
    assert!(
        screen.contains("Unknown theme \"purple\"") && screen.contains("light, dark or auto"),
        "{screen}"
    );
    h.keys(":theme<CR>");
    assert!(h.screen().contains("theme: dark (auto)"), "{}", h.screen());
}

#[test]
fn the_light_theme_survives_truecolor() {
    let fx = feature_branch();
    let mut h = open_light(&fx);
    h.set_truecolor(true);
    let frame = h.frame();
    let (x, y) = frame.find("add a").expect("commit row");
    let Color::Indexed(i) = LIGHT.fg else {
        panic!("the light fg is a palette colour")
    };
    let (r, g, b) = moonhare::color::indexed_to_rgb(i);
    assert_eq!(frame.fg_at(x, y), Color::Rgb(r, g, b));
}

// ------------------------------------------------------- a late reply

fn key(c: char, m: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), m))
}

fn plain(text: &str) -> Vec<Event> {
    text.chars().map(|c| key(c, KeyModifiers::NONE)).collect()
}

/// The keys crossterm makes of an OSC 11 reply that comes after the query
/// gave up: `ESC ]` is Alt-`]`, `ESC \` is Alt-`\`, BEL is Ctrl-g.
fn late_reply(end: Event) -> Vec<Event> {
    let mut keys = vec![key(']', KeyModifiers::ALT)];
    keys.extend(plain("11;rgb:ffff/ffff/ffff"));
    keys.push(end);
    keys
}

fn alt_backslash() -> Event {
    key('\\', KeyModifiers::ALT)
}

fn feed(h: &mut Harness, events: impl IntoIterator<Item = Event>) {
    for event in events {
        h.session().handle(event);
    }
    h.settle();
}

/// A session whose query for the background gave up: a reply may come late.
fn expecting(fx: &support::Fixture) -> Harness {
    let mut h = Harness::open(fx, 160, 30);
    h.session().expect_late_reply();
    h.settle();
    h
}

#[test]
fn a_reply_of_the_terminal_that_comes_late_is_not_taken_for_keys() {
    let fx = feature_branch();
    let mut h = expecting(&fx);
    let before = h.screen();
    let bel = key('g', KeyModifiers::CONTROL);
    for end in [alt_backslash(), bel] {
        feed(&mut h, late_reply(end));
        assert_eq!(h.screen(), before, "the reply moved the screen");
    }
    // Keys after it are keys again.
    h.keys(":theme light<CR>");
    assert!(h.screen().contains("theme:light"), "{}", h.screen());
}

#[test]
fn a_late_reply_in_pieces_a_moment_apart_is_still_dropped() {
    let fx = feature_branch();
    let mut h = expecting(&fx);
    let before = h.screen();
    let keys = late_reply(alt_backslash());
    let (first, rest) = keys.split_at(9);
    feed(&mut h, first.to_vec());
    h.wait(50);
    feed(&mut h, rest.to_vec());
    assert_eq!(h.screen(), before, "the reply moved the screen");
}

/// The screens `events` leave in a session that expects a late reply and in
/// one that does not, both opened before either takes a key (what is
/// Reviewed is kept on disk).
fn with_and_without(events: &[Event], pause: u64) -> (String, String) {
    let fx = feature_branch();
    let mut with = expecting(&fx);
    let mut without = Harness::open(&fx, 160, 30);
    without.settle();
    for h in [&mut with, &mut without] {
        feed(h, events.to_vec());
        if pause > 0 {
            h.wait(pause);
        }
    }
    (with.screen(), without.screen())
}

#[test]
fn esc_then_a_key_that_tmux_joins_into_alt_is_given_back() {
    // tmux sends Esc and a quick `]` as `ESC ]`: Alt-`]`.
    for typed in [
        vec![key(']', KeyModifiers::ALT), key('h', KeyModifiers::NONE)],
        [vec![key(']', KeyModifiers::ALT)], plain(":theme light")].concat(),
        [vec![key(']', KeyModifiers::ALT)], plain("11j")].concat(),
    ] {
        let (with, without) = with_and_without(&typed, 0);
        assert_eq!(with, without, "{typed:?}");
    }
}

#[test]
fn a_pause_inside_what_looked_like_a_reply_gives_the_keys_back() {
    let typed = [vec![key(']', KeyModifiers::ALT)], plain("11;")].concat();
    let fx = feature_branch();
    let mut h = expecting(&fx);
    feed(&mut h, typed.clone());
    assert!(
        h.next_tick_in().is_some(),
        "the keys are held, a wake-up due"
    );
    h.wait(200);
    assert_eq!(h.next_tick_in(), None, "the keys are given back");
    let (with, without) = with_and_without(&typed, 200);
    assert_eq!(with, without);
}

#[test]
fn nothing_is_dropped_unless_the_query_gave_up_or_after_a_while() {
    let reply = late_reply(alt_backslash());
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.settle();
    let before = h.screen();
    feed(&mut h, reply.clone());
    assert_ne!(h.screen(), before, "no query gave up: these are keys");

    let mut h = expecting(&fx);
    h.wait(3_100);
    let before = h.screen();
    feed(&mut h, reply);
    assert_ne!(h.screen(), before, "the window is over: these are keys");
}

// ------------------------------------------------- a real terminal (pty)

/// A pseudo-terminal in raw mode, as the query finds `/dev/tty`: the
/// terminal's end and the program's end.
#[cfg(unix)]
fn pty() -> (std::fs::File, std::fs::File) {
    use std::ffi::CStr;
    // `ptsname` answers in a static buffer: one pty at a time.
    static MAKING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _making = MAKING.lock().unwrap_or_else(|e| e.into_inner());
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::OpenOptionsExt;
    // SAFETY: plain libc calls on the descriptor they return; every result
    // is checked.
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(master >= 0, "posix_openpt");
        assert_eq!(libc::grantpt(master), 0, "grantpt");
        assert_eq!(libc::unlockpt(master), 0, "unlockpt");
        let name = libc::ptsname(master);
        assert!(!name.is_null(), "ptsname");
        let name = CStr::from_ptr(name).to_str().unwrap().to_owned();
        let master = std::fs::File::from_raw_fd(master);
        let program = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(name)
            .unwrap();
        let mut raw: libc::termios = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(program.as_raw_fd(), &mut raw), 0);
        libc::cfmakeraw(&mut raw);
        assert_eq!(libc::tcsetattr(program.as_raw_fd(), libc::TCSANOW, &raw), 0);
        (master, program)
    }
}

/// Run the query on the program's end of a pty, in a thread, so a query
/// that hangs fails the test instead of holding it.
#[cfg(unix)]
fn query_pty(program: std::fs::File, timeout: std::time::Duration) -> (Reply, std::time::Duration) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let mut output = program.try_clone().unwrap();
        let got = theme::query_background_on(&program, &mut output, timeout);
        let _ = tx.send((got, started.elapsed()));
    });
    rx.recv_timeout(timeout + std::time::Duration::from_secs(3))
        .expect("the query never returned")
}

#[cfg(unix)]
#[test]
fn a_silent_pty_holds_the_start_no_longer_than_the_timeout() {
    let (_terminal, program) = pty();
    let (got, took) = query_pty(program, std::time::Duration::from_millis(100));
    assert_eq!(got, Reply::TimedOut);
    assert!(took < std::time::Duration::from_millis(600), "{took:?}");
}

#[cfg(unix)]
#[test]
fn a_pty_that_answers_tells_its_background() {
    use std::io::Write;
    let (mut terminal, program) = pty();
    terminal
        .write_all(b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\\x1b[?62;22c")
        .unwrap();
    let (got, _) = query_pty(program, std::time::Duration::from_secs(2));
    assert_eq!(got, Reply::Told(Background::Light));
}

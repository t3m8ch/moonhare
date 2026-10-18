//! Themes: the dark and the light palette, the background `$COLORFGBG`
//! names, and switching with `:theme`.

mod support;

use moonhare::session::Session;
use moonhare::terminal_input::{self, Reply};
use moonhare::theme::{self, Background, Choice, Theme, DARK, LIGHT};
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

// ------------------------------------------------------------ COLORFGBG

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
    session.set_choice(Choice::Auto);
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
    assert!(h.screen().contains("theme:auto"), "{}", h.screen());
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

    // Dark is the default: the status bar does not name it.
    h.keys(":theme dark<CR>");
    assert_eq!(h.frame().fg_at(x, y), DARK.fg);
    assert!(!h.screen().contains("theme:"), "{}", h.screen());

    h.keys(":theme auto<CR>");
    assert_eq!(h.frame().fg_at(x, y), DARK.fg);
    assert!(h.screen().contains("theme:auto"), "{}", h.screen());
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
    assert!(h.screen().contains("theme: dark"), "{}", h.screen());
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

// ------------------------------------------------------- terminal replies

#[test]
fn a_background_reply_switches_the_auto_theme_whenever_it_comes() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.session().set_choice(Choice::Auto);
    h.settle();
    let (x, y) = cell(&mut h, "add a");
    assert_eq!(
        h.frame().fg_at(x, y),
        DARK.fg,
        "the first frame does not wait"
    );

    h.wait(2_000);
    h.session()
        .terminal_reply(&Reply::Background(Background::Light));
    assert_eq!(h.frame().fg_at(x, y), LIGHT.fg);
    // The terminal's colour scheme changed (DEC 2031).
    h.session()
        .terminal_reply(&Reply::Background(Background::Dark));
    assert_eq!(h.frame().fg_at(x, y), DARK.fg);
    h.session().terminal_reply(&Reply::Other);
    assert_eq!(h.frame().fg_at(x, y), DARK.fg);
}

#[test]
fn a_chosen_theme_outranks_the_terminal_background() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.settle();
    let (x, y) = cell(&mut h, "add a");
    h.keys(":theme dark<CR>");
    h.session()
        .terminal_reply(&Reply::Background(Background::Light));
    assert_eq!(h.frame().fg_at(x, y), DARK.fg);
    assert!(h.session().take_terminal_output().is_empty());
    // Back to auto: the background known so far, and asked anew.
    h.keys(":theme auto<CR>");
    assert_eq!(h.frame().fg_at(x, y), LIGHT.fg);
    assert_eq!(
        h.session().take_terminal_output(),
        vec![terminal_input::QUERY_BACKGROUND.to_string()]
    );
    h.session()
        .terminal_reply(&Reply::Background(Background::Dark));
    assert_eq!(h.frame().fg_at(x, y), DARK.fg);
}

#[test]
fn osc_11_colours_tell_light_from_dark() {
    use moonhare::theme::background_from_osc11 as bg;
    assert_eq!(bg("rgb:ffff/ffff/ffff"), Some(Background::Light));
    assert_eq!(bg("rgb:fdfd/f6f6/e3e3"), Some(Background::Light));
    assert_eq!(bg("rgb:1e/1e/2e"), Some(Background::Dark));
    assert_eq!(bg("rgb:0/0/0"), Some(Background::Dark));
    assert_eq!(bg("rgba:ffff/ffff/ffff/8000"), Some(Background::Light));
    for bad in [
        "",
        "rgb:ff/ff",
        "rgb:ff/ff/zz",
        "rgb:+f/f/f",
        "rgb:fffff/0/0",
        "#ffffff",
    ] {
        assert_eq!(bg(bad), None, "{bad:?}");
    }
}

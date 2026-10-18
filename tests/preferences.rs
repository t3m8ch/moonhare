//! Preferences: the theme and the diff layout the Reviewer chose, kept
//! between runs in every repository (ADR 0008). Every test gives the session
//! a directory of its own; none reads the Reviewer's real one.

mod support;

use std::path::{Path, PathBuf};

use moonhare::preferences::Preferences;
use moonhare::session::Session;
use moonhare::terminal_input::Reply;
use moonhare::theme::{self, Background, Choice};
use support::{feature_branch, Fixture, Harness};

/// A session on `fx` whose preferences live in `dir`.
fn open(fx: &Fixture, dir: &Path, width: u16) -> Harness {
    let mut session = Session::open(fx.path(), width, 30).unwrap();
    session.use_preferences(Preferences::open(dir));
    let mut h = Harness::from_session(session);
    h.settle();
    h
}

fn file(dir: &Path) -> PathBuf {
    dir.join("preferences")
}

fn status(h: &mut Harness) -> String {
    let screen = h.screen();
    let rows: Vec<&str> = screen.lines().collect();
    rows[rows.len() - 2].to_string()
}

// -------------------------------------------------------------------- theme

#[test]
fn without_preferences_the_theme_is_dark_whatever_the_terminal_says() {
    let fx = feature_branch();
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(&fx, dir.path(), 160);
    assert_eq!(h.session().theme().name, "dark");
    h.session()
        .terminal_reply(&Reply::Background(Background::Light));
    h.session().set_terminal_background(Some(Background::Light));
    assert_eq!(h.session().theme().name, "dark", "no auto-detection");
    assert!(
        !file(dir.path()).exists(),
        "nothing chosen, nothing written"
    );
    // A session without preferences at all is dark too.
    let mut bare = Harness::open(&fx, 160, 30);
    bare.session()
        .terminal_reply(&Reply::Background(Background::Light));
    assert_eq!(bare.session().theme().name, "dark");
}

#[test]
fn the_theme_command_is_remembered_by_the_next_session() {
    let fx = feature_branch();
    let other = feature_branch();
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(&fx, dir.path(), 160);
    h.keys(":theme light<CR>");
    assert_eq!(h.session().theme().name, "light");
    let saved = std::fs::read_to_string(file(dir.path())).unwrap();
    assert!(saved.contains("theme = light"), "{saved}");

    // Another repository, the same preferences.
    let mut next = open(&other, dir.path(), 160);
    assert_eq!(next.session().theme().name, "light");
    assert!(status(&mut next).contains("theme:light"));
}

#[test]
fn an_explicit_theme_beats_the_saved_one_and_does_not_overwrite_it() {
    let fx = feature_branch();
    let dir = tempfile::tempdir().unwrap();
    open(&fx, dir.path(), 160).keys(":theme light<CR>");
    let before = std::fs::read_to_string(file(dir.path())).unwrap();

    // What `MOONHARE_THEME=dark` gives main.
    let explicit = theme::explicit_choice(None, Some("dark")).unwrap();
    assert_eq!(explicit, Some(Choice::Dark));
    let mut h = open(&fx, dir.path(), 160);
    h.session().set_choice(explicit.unwrap());
    assert_eq!(h.session().theme().name, "dark");
    assert_eq!(std::fs::read_to_string(file(dir.path())).unwrap(), before);

    let mut again = open(&fx, dir.path(), 160);
    assert_eq!(again.session().theme().name, "light");
}

#[test]
fn the_flag_beats_the_environment_and_an_empty_one_is_no_choice() {
    use theme::explicit_choice;
    assert_eq!(
        explicit_choice(Some("light"), Some("dark")),
        Ok(Some(Choice::Light))
    );
    assert_eq!(explicit_choice(None, Some("")), Ok(None));
    assert_eq!(explicit_choice(None, None), Ok(None));
    assert!(explicit_choice(Some("purple"), None).is_err());
    assert_eq!(Choice::default(), Choice::Dark);
}

#[test]
fn theme_auto_is_remembered_and_follows_the_terminal() {
    let fx = feature_branch();
    let dir = tempfile::tempdir().unwrap();
    open(&fx, dir.path(), 160).keys(":theme auto<CR>");
    let mut h = open(&fx, dir.path(), 160);
    assert_eq!(h.session().theme().name, "dark", "background still unknown");
    h.session()
        .terminal_reply(&Reply::Background(Background::Light));
    assert_eq!(h.session().theme().name, "light");
}

// ------------------------------------------------------------------- layout

#[test]
fn s_is_remembered_and_the_next_wide_session_opens_inline() {
    let fx = feature_branch();
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(&fx, dir.path(), 160);
    assert!(status(&mut h).contains("layout:side-by-side (auto)"));
    h.keys("s");
    assert!(status(&mut h).contains("layout:inline (manual)"));

    let mut next = open(&fx, dir.path(), 160);
    let line = status(&mut next);
    assert!(line.contains("layout:inline (manual)"), "{line}");
}

#[test]
fn layout_auto_is_remembered_too() {
    let fx = feature_branch();
    let dir = tempfile::tempdir().unwrap();
    open(&fx, dir.path(), 160).keys("s");
    let mut h = open(&fx, dir.path(), 160);
    h.keys(":layout auto<CR>");
    assert!(status(&mut h).contains("layout:side-by-side (auto)"));

    let mut next = open(&fx, dir.path(), 160);
    assert!(status(&mut next).contains("layout:side-by-side (auto)"));
    let mut narrow = open(&fx, dir.path(), 100);
    assert!(status(&mut narrow).contains("layout:inline (auto)"));
}

#[test]
fn the_layout_command_names_a_layout_or_shows_it() {
    let fx = feature_branch();
    let dir = tempfile::tempdir().unwrap();
    let mut h = open(&fx, dir.path(), 160);
    h.keys(":layout inline<CR>");
    assert!(status(&mut h).contains("layout:inline (manual)"));
    h.keys(":layout side-by-side<CR>");
    assert!(status(&mut h).contains("layout:side-by-side (manual)"));
    h.keys(":layout sideways<CR>");
    let line = status(&mut h);
    assert!(line.contains("Unknown layout \"sideways\""), "{line}");
    let mut next = open(&fx, dir.path(), 100);
    assert!(status(&mut next).contains("layout:side-by-side (manual)"));
}

// --------------------------------------------------------------------- file

#[test]
fn a_damaged_or_foreign_file_is_ignored() {
    let fx = feature_branch();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(file(dir.path()), b"\xff\xfe theme = light\n\0\0").unwrap();
    let mut h = open(&fx, dir.path(), 160);
    assert_eq!(h.session().theme().name, "dark");

    std::fs::write(
        file(dir.path()),
        "theme = purple\nlayout = diagonal\nnonsense\n",
    )
    .unwrap();
    let mut h = open(&fx, dir.path(), 160);
    assert_eq!(h.session().theme().name, "dark");
    assert!(status(&mut h).contains("layout:side-by-side (auto)"));
}

#[test]
fn unknown_keys_survive_a_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(file(dir.path()), "# kept\nfuture = yes\ntheme = dark\n").unwrap();
    let mut prefs = Preferences::open(dir.path());
    prefs.set("theme", "light").unwrap();
    let text = std::fs::read_to_string(file(dir.path())).unwrap();
    assert!(text.contains("future = yes"), "{text}");
    assert!(text.contains("theme = light"), "{text}");
    assert!(!text.contains("theme = dark"), "{text}");
    assert_eq!(Preferences::open(dir.path()).get("future"), Some("yes"));
    // No temporary file is left beside it.
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("preferences")]);
}

#[test]
fn a_write_that_fails_is_a_message_not_a_panic() {
    let fx = feature_branch();
    let dir = tempfile::tempdir().unwrap();
    // The directory of the preferences is a file: nothing can be written.
    let blocked = dir.path().join("blocked");
    std::fs::write(&blocked, "").unwrap();
    let mut h = open(&fx, &blocked, 160);
    h.keys(":theme light<CR>");
    assert_eq!(h.session().theme().name, "light", "it applies anyway");
    let line = status(&mut h);
    assert!(line.contains("Cannot save preferences"), "{line}");
}

#[test]
fn the_directory_comes_from_xdg_state_home_else_from_home() {
    use moonhare::preferences::dir_from;
    assert_eq!(
        dir_from(Some("/x/state"), Some("/home/r"), false),
        Some(PathBuf::from("/x/state/moonhare"))
    );
    assert_eq!(
        dir_from(Some("/x/state"), Some("/Users/r"), true),
        Some(PathBuf::from("/x/state/moonhare"))
    );
    assert_eq!(
        dir_from(None, Some("/home/r"), false),
        Some(PathBuf::from("/home/r/.local/state/moonhare"))
    );
    // A relative or empty XDG_STATE_HOME is ignored, as the spec says.
    assert_eq!(
        dir_from(Some("rel"), Some("/home/r"), false),
        Some(PathBuf::from("/home/r/.local/state/moonhare"))
    );
    assert_eq!(
        dir_from(Some(""), Some("/Users/r"), true),
        Some(PathBuf::from(
            "/Users/r/Library/Application Support/moonhare"
        ))
    );
    assert_eq!(dir_from(None, None, false), None);
}

#[test]
fn the_help_says_where_preferences_are_kept() {
    let help = moonhare::hints::help_lines().join("\n");
    assert!(help.contains(".local/state/moonhare"), "{help}");
    assert!(help.contains(":layout"), "{help}");
}

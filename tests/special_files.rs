//! Created, deleted, renamed and binary files, and the note shown when the
//! diff is line-based instead of structural. Each case is built in a
//! temporary repository.

mod support;

use support::{Fixture, Harness};

fn row_with<'a>(screen: &'a str, needle: &str) -> Option<&'a str> {
    screen.lines().find(|l| l.contains(needle))
}

/// Narrow enough for the inline layout.
const INLINE: (u16, u16) = (100, 30);

#[test]
fn created_file_is_shown_entirely_as_added() {
    let fx = Fixture::new();
    fx.write("README.md", "# demo\n").commit("initial");
    fx.branch("feature");
    fx.write("new.rs", "fn one() {}\nfn two() {}\nfn three() {}\n")
        .commit("add new");

    let mut h = Harness::open(&fx, INLINE.0, INLINE.1);
    let screen = h.screen();
    for line in ["fn one() {}", "fn two() {}", "fn three() {}"] {
        let row = row_with(&screen, line).unwrap_or_else(|| panic!("{line} shown:\n{screen}"));
        assert!(row.contains('+'), "{line} is marked added:\n{screen}");
        assert!(
            !row.contains('-'),
            "{line} is not marked removed:\n{screen}"
        );
    }
}

#[test]
fn deleted_file_is_shown_entirely_as_removed() {
    let fx = Fixture::new();
    fx.write("gone.rs", "fn one() {}\nfn two() {}\n")
        .write("README.md", "# demo\n")
        .commit("initial");
    fx.branch("feature");
    fx.remove("gone.rs").commit("remove gone");

    let mut h = Harness::open(&fx, INLINE.0, INLINE.1);
    let screen = h.screen();
    for line in ["fn one() {}", "fn two() {}"] {
        let row = row_with(&screen, line).unwrap_or_else(|| panic!("{line} shown:\n{screen}"));
        assert!(row.contains('-'), "{line} is marked removed:\n{screen}");
        assert!(!row.contains('+'), "{line} is not marked added:\n{screen}");
    }
}

const BODY: &str = "fn alpha() {\n    1\n}\n\nfn beta() {\n    2\n}\n\nfn gamma() {\n    3\n}\n";

#[test]
fn pure_rename_shows_both_paths() {
    let fx = Fixture::new();
    fx.write("old_name.rs", BODY)
        .write("README.md", "# demo\n")
        .commit("initial");
    fx.branch("feature");
    fx.git(&["mv", "old_name.rs", "new_name.rs"]);
    fx.commit("rename");

    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    // One entry in the file list, not a deletion plus a creation.
    assert!(screen.contains("Files (1)"), "{screen}");
    let header = row_with(&screen, "renamed").unwrap_or_else(|| panic!("renamed:\n{screen}"));
    assert!(header.contains("old_name.rs"), "old path:\n{screen}");
    assert!(header.contains("new_name.rs"), "new path:\n{screen}");
}

#[test]
fn rename_with_an_edit_shows_the_edit() {
    let fx = Fixture::new();
    fx.write("old_name.rs", BODY)
        .write("README.md", "# demo\n")
        .commit("initial");
    fx.branch("feature");
    fx.git(&["mv", "old_name.rs", "new_name.rs"]);
    fx.write("new_name.rs", &BODY.replace("    2\n", "    22\n"))
        .commit("rename and edit");

    let mut h = Harness::open(&fx, INLINE.0, INLINE.1);
    let screen = h.screen();
    assert!(screen.contains("Files (1)"), "{screen}");
    let header = row_with(&screen, "renamed").unwrap_or_else(|| panic!("renamed:\n{screen}"));
    assert!(header.contains("old_name.rs") && header.contains("new_name.rs"));
    let removed = row_with(&screen, "    2").unwrap_or_else(|| panic!("old line:\n{screen}"));
    assert!(removed.contains('-'), "{screen}");
    let added = row_with(&screen, "    22").unwrap_or_else(|| panic!("new line:\n{screen}"));
    assert!(added.contains('+'), "{screen}");
    // The untouched part of the file is not shown as changed.
    let kept = row_with(&screen, "fn alpha").unwrap_or_else(|| panic!("context:\n{screen}"));
    assert!(!kept.contains('+') && !kept.contains('-'), "{screen}");
}

/// A branch that modifies one file at `path` from `before` to `after`.
fn modified(path: &str, before: &str, after: &str) -> Fixture {
    let fx = Fixture::new();
    fx.write(path, before).commit("initial");
    fx.branch("feature");
    fx.write(path, after).commit("change");
    fx
}

#[test]
fn unknown_language_fallback_names_the_language_as_the_reason() {
    let fx = modified("notes.zzz", "one\ntwo\n", "one\n2\n");
    let mut h = Harness::open(&fx, INLINE.0, INLINE.1);
    let screen = h.screen();
    assert!(
        screen.contains("line-based diff: unknown language"),
        "{screen}"
    );
}

#[test]
fn parse_error_fallback_names_the_language() {
    let fx = modified(
        "broken.rs",
        "fn a() {\n    1\n}\n",
        "fn a( {\n    1 +\n}}}\n",
    );
    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    assert!(
        screen.contains("line-based diff: ") && screen.contains("parse errors in Rust"),
        "{screen}"
    );
}

#[test]
fn oversized_file_fallback_names_the_size_limit() {
    let big = "let x = 1;\n".repeat(100_000);
    let fx = modified("big.rs", &big, &format!("{big}let y = 2;\n"));
    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    assert!(
        screen.contains("line-based diff: size limit exceeded"),
        "{screen}"
    );
}

#[test]
fn graph_limit_fallback_is_named_in_the_note() {
    let note = moonhare::special::fallback_note(&moonhare::diff::FallbackReason::GraphLimit);
    assert_eq!(
        note,
        "line-based diff: diff too complex, graph limit exceeded"
    );
}

#[test]
fn structural_diff_has_no_fallback_note() {
    let fx = modified("ok.rs", "fn a() {}\n", "fn b() {}\n");
    let mut h = Harness::open(&fx, 160, 30);
    assert!(!h.screen().contains("line-based"));
}

#[test]
fn binary_file_is_a_note_without_a_line_diff() {
    let fx = Fixture::new();
    fx.write("README.md", "# demo\n").commit("initial");
    fx.branch("feature");
    fx.write_bytes(
        "logo.png",
        &[0x89, b'P', b'N', b'G', 0, 1, 2, 3, b'a', b'\n', 0xff],
    )
    .commit("add logo");

    // A binary file is Mechanical, so it starts collapsed.
    let mut h = Harness::open(&fx, INLINE.0, INLINE.1);
    h.keys("<CR>");
    let screen = h.screen();
    assert!(screen.contains("Binary file"), "{screen}");
    assert!(
        !screen.contains("PNG"),
        "no line diff of the bytes:\n{screen}"
    );
}

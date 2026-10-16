use moonhare::diff::{diff_file, FallbackReason, FileKind, Highlight, Limits, Side};

const OLD_RS: &str = "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\nfn main() {\n    println!(\"{}\", add(1, 2));\n}\n";
const NEW_RS: &str = "fn add(a: i32, b: i32) -> i32 {\n    a + b + 1\n}\n\nfn main() {\n    println!(\"{}\", add(1, 2));\n}\n";

fn modified(path: &str, old: &str, new: &str) -> moonhare::diff::FileDiff {
    diff_file(
        path,
        Some(old.as_bytes()),
        Some(new.as_bytes()),
        Limits::default(),
    )
}

#[test]
fn rust_change_is_structural_with_aligned_rows_and_changed_fragments() {
    let d = modified("src/lib.rs", OLD_RS, NEW_RS);

    assert_eq!(d.kind, FileKind::Modified);
    assert_eq!(d.language.as_deref(), Some("Rust"));
    assert_eq!(d.fallback, None);

    // Both versions have 7 lines and every row pairs equal line numbers.
    assert_eq!(d.rows.len(), 7);
    assert!(d
        .rows
        .iter()
        .enumerate()
        .all(|(i, r)| r.old == Some(i) && r.new == Some(i)));

    // Only the `+ 1` on line 1 is novel, and only on the new side.
    let changed: Vec<String> = d.changed_fragments(Side::New).map(|f| f.text).collect();
    assert_eq!(changed, ["+", "1"]);
    assert!(d.changed_fragments(Side::Old).next().is_none());
    assert!(d.changed_fragments(Side::New).all(|f| f.line == 1));
}

#[test]
fn highlighting_covers_unchanged_tokens_too() {
    let d = modified("src/lib.rs", OLD_RS, NEW_RS);

    let fn_kw = d
        .tokens(Side::New)
        .iter()
        .find(|t| t.line == 0 && t.start_col == 0)
        .expect("token for `fn` on line 0");
    assert!(!fn_kw.changed);
    assert_eq!(fn_kw.highlight, Highlight::Keyword);
    assert_eq!(
        &d.new_lines[0][fn_kw.start_col as usize..fn_kw.end_col as usize],
        "fn"
    );

    // The string literal on line 5 is unchanged but still highlighted.
    assert!(d
        .tokens(Side::New)
        .iter()
        .any(|t| t.line == 5 && t.highlight == Highlight::String && !t.changed));
}

#[test]
fn python_change_is_structural() {
    let old = "def greet(name):\n    return 'hi ' + name\n";
    let new = "def greet(name):\n    return 'hello ' + name\n";
    let d = modified("greet.py", old, new);

    assert_eq!(d.language.as_deref(), Some("Python"));
    assert_eq!(d.fallback, None);
    assert!(d.changed_fragments(Side::New).any(|f| f.line == 1));
    assert!(d.changed_fragments(Side::Old).any(|f| f.line == 1));
}

#[test]
fn reformatting_without_semantic_change_has_no_changes() {
    let old = "fn main() { let x = 1; let y = x + 2; println!(\"{}\", y); }\n";
    let new = "fn main() {\n    let x = 1;\n    let y = x + 2;\n    println!(\"{}\", y);\n}\n";
    let d = modified("a.rs", old, new);

    assert_eq!(d.fallback, None);
    assert!(d.changed_fragments(Side::Old).next().is_none());
    assert!(d.changed_fragments(Side::New).next().is_none());
}

#[test]
fn unknown_extension_falls_back_to_line_diff_with_reason() {
    let d = modified("notes.zzz", "one\ntwo\nthree\n", "one\n2\nthree\n");

    assert_eq!(d.language, None);
    assert_eq!(d.fallback, Some(FallbackReason::UnknownLanguage));
    assert_eq!(d.rows.len(), 3);
    assert!(d.changed_fragments(Side::New).all(|f| f.line == 1));
    assert!(d.changed_fragments(Side::New).next().is_some());
}

#[test]
fn file_over_the_byte_limit_falls_back_with_reason() {
    let limits = Limits {
        byte_limit: 16,
        ..Limits::default()
    };
    let d = diff_file(
        "src/lib.rs",
        Some(OLD_RS.as_bytes()),
        Some(NEW_RS.as_bytes()),
        limits,
    );

    assert_eq!(d.language.as_deref(), Some("Rust"));
    assert!(matches!(d.fallback, Some(FallbackReason::ByteLimit { .. })));
    assert!(d.changed_fragments(Side::New).any(|f| f.line == 1));
}

#[test]
fn graph_limit_falls_back_with_reason() {
    let limits = Limits {
        graph_limit: 1,
        ..Limits::default()
    };
    let d = diff_file(
        "src/lib.rs",
        Some(OLD_RS.as_bytes()),
        Some(NEW_RS.as_bytes()),
        limits,
    );

    assert_eq!(d.fallback, Some(FallbackReason::GraphLimit));
}

#[test]
fn parse_errors_fall_back_with_reason() {
    let d = modified("broken.rs", "fn main() {}\n", "fn main( {{{ let = ;\n");

    assert!(matches!(
        d.fallback,
        Some(FallbackReason::ParseErrors { .. })
    ));
    assert!(d.changed_fragments(Side::New).next().is_some());
}

#[test]
fn created_file_is_all_new() {
    let d = diff_file(
        "src/new.rs",
        None,
        Some(NEW_RS.as_bytes()),
        Limits::default(),
    );

    assert_eq!(d.kind, FileKind::Created);
    assert!(d.old_lines.is_empty());
    assert_eq!(d.rows.len(), 7);
    assert!(d.rows.iter().all(|r| r.old.is_none() && r.new.is_some()));
}

#[test]
fn deleted_file_is_all_old() {
    let d = diff_file(
        "src/old.rs",
        Some(OLD_RS.as_bytes()),
        None,
        Limits::default(),
    );

    assert_eq!(d.kind, FileKind::Deleted);
    assert!(d.new_lines.is_empty());
    assert_eq!(d.rows.len(), 7);
    assert!(d.rows.iter().all(|r| r.old.is_some() && r.new.is_none()));
}

#[test]
fn binary_file_is_distinguished_and_has_no_rows() {
    let d = diff_file(
        "logo.png",
        Some(b"\x89PNG\0\x01"),
        Some(b"\x89PNG\0\x02"),
        Limits::default(),
    );

    assert_eq!(d.kind, FileKind::Binary);
    assert!(d.rows.is_empty());
}

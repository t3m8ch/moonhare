//! Mechanical files: lock files, snapshots, binary and generated files are
//! collapsed in the diff and listed last, yet counted in progress and marked
//! only by hand. Each case is built in a temporary repository.

mod support;

use support::{Fixture, Harness};

/// Index of the first sidebar row that lists the file `path`: the tree
/// shows its name under its directory.
fn row_index(screen: &str, path: &str) -> usize {
    let needle = path.rsplit('/').next().unwrap();
    screen
        .lines()
        .map(|l| l.chars().take(32).collect::<String>())
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no row with {needle}:\n{screen}"))
}

/// `main` holds a README; `feature` adds a lock file (which sorts before the
/// source file by path) and a source file.
fn lock_and_source() -> Fixture {
    let fx = Fixture::new();
    fx.write("README.md", "# demo\n").commit("initial");
    fx.branch("feature");
    fx.write("Cargo.lock", "# lock\nversion = 3\n")
        .write("src/lib.rs", "pub fn one() {}\n")
        .commit("add lock and source");
    fx
}

#[test]
fn a_lock_file_is_listed_after_the_ordinary_files() {
    let fx = lock_and_source();
    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    assert!(
        row_index(&screen, "src/lib.rs") < row_index(&screen, "Cargo.lock"),
        "{screen}"
    );
}

#[test]
fn a_lock_file_is_collapsed_in_the_diff_with_the_reason() {
    let fx = lock_and_source();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]f");
    let screen = h.screen();
    assert!(!screen.contains("version = 3"), "{screen}");
    assert!(screen.contains("lock file"), "{screen}");
}

/// A branch that adds `path` (with `content`) and a source file, and lands
/// the cursor on the mechanical file, which is listed last.
fn one_mechanical(path: &str, content: &[u8]) -> Fixture {
    let fx = Fixture::new();
    fx.write("README.md", "# demo\n").commit("initial");
    fx.branch("feature");
    fx.write_bytes(path, content)
        .write("src/lib.rs", "pub fn one() {}\n")
        .commit("add files");
    fx
}

fn assert_mechanical(path: &str, content: &[u8], marker: &str, label: &str) {
    let fx = one_mechanical(path, content);
    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    assert!(
        row_index(&screen, "src/lib.rs") < row_index(&screen, path),
        "{path} is listed last:\n{screen}"
    );
    h.keys("]f");
    let screen = h.screen();
    assert!(!screen.contains(marker), "{path} is collapsed:\n{screen}");
    assert!(screen.contains(label), "{path} says {label}:\n{screen}");
}

#[test]
fn other_known_lock_files_are_mechanical() {
    for name in [
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "poetry.lock",
        "go.sum",
        "deps/anything.lock",
    ] {
        assert_mechanical(name, b"locked-entry\n", "locked-entry", "lock file");
    }
}

#[test]
fn snapshot_files_are_mechanical() {
    for name in [
        "tests/__snapshots__/ui.test.js.snap",
        "src/snapshots/app__view.snap",
        "tests/render.snap",
    ] {
        assert_mechanical(name, b"snapshot-body\n", "snapshot-body", "snapshot");
    }
}

#[test]
fn binary_files_are_mechanical() {
    assert_mechanical(
        "assets/logo.png",
        &[0x89, b'P', b'N', b'G', 0, 1, 2, 3],
        "Binary file not shown",
        "binary",
    );
}

#[test]
fn files_marked_generated_by_attributes_are_mechanical() {
    let fx = Fixture::new();
    fx.write("README.md", "# demo\n").commit("initial");
    fx.branch("feature");
    fx.write(
        ".gitattributes",
        "gen/** linguist-generated\ngen/keep.rs -linguist-generated\n",
    )
    .write("gen/api.rs", "generated-body\n")
    .write("gen/keep.rs", "handwritten-body\n")
    .write("src/lib.rs", "pub fn one() {}\n")
    .commit("add generated");
    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    // The negated file stays ordinary, so it comes before the generated one.
    assert!(
        row_index(&screen, "gen/keep.rs") < row_index(&screen, "gen/api.rs"),
        "{screen}"
    );
    // The tree: .gitattributes, gen/ with keep.rs and then api.rs, src/lib.rs.
    // The generated file is last in its directory.
    h.keys("]f");
    assert!(h.screen().contains("handwritten-body"), "{}", h.screen());
    h.keys("]f");
    let screen = h.screen();
    assert!(!screen.contains("generated-body"), "{screen}");
    assert!(screen.contains("generated"), "{screen}");
}

#[test]
fn mechanical_files_count_in_progress_and_are_marked_only_by_hand() {
    let fx = lock_and_source();
    let mut h = Harness::open(&fx, 160, 30);
    // Looking at the collapsed file marks nothing.
    h.keys("]f<CR>");
    let screen = h.screen();
    assert!(screen.contains("reviewed 0/2"), "{screen}");
    assert!(!screen.contains('✓'), "{screen}");

    h.keys("m");
    let screen = h.screen();
    assert!(screen.contains("reviewed 1/2"), "{screen}");
    assert!(screen.contains("whole Range 1/2"), "{screen}");
    // The sidebar row, not the file bar above the diff.
    let row: String = screen
        .lines()
        .map(|l| l.chars().take(32).collect::<String>())
        .find(|l| l.contains("Cargo.lock"))
        .unwrap();
    assert!(row.contains('✓'), "{screen}");
}

#[test]
fn enter_expands_a_collapsed_file_and_moving_away_collapses_it_again() {
    let fx = lock_and_source();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]f<CR>");
    assert!(h.screen().contains("version = 3"), "{}", h.screen());
    h.keys("[f]f");
    assert!(!h.screen().contains("version = 3"), "{}", h.screen());
}

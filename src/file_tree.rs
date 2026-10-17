//! The directory tree of the file panel: the files of one Group (or of the
//! whole Range) laid out as directories and files, for the sidebar and the
//! file picker alike.
//!
//! A directory whose only child is a directory is drawn as one node, joined
//! with `/` as GitHub does (`src/features/course`). When such a chain does not
//! fit the width it is split into nested nodes instead of being cut; `…` is
//! left for a single segment too long for its row.
//!
//! The order is the reading order of the input: a directory stands where its
//! first file is, and the files of a directory keep their order.

use std::collections::HashMap;

/// Columns each level of the tree is indented by.
pub const INDENT: usize = 2;
/// Columns of a directory row before its name: the fold mark and a space.
pub const DIR_PREFIX: usize = 2;
/// Columns of a file row before its name: the reviewed mark, a space, the
/// status letter and a space (`✓ M name`).
pub const FILE_PREFIX: usize = 4;

/// One row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// A directory node: one or more path segments.
    Dir {
        depth: usize,
        /// Columns the row is indented by.
        indent: usize,
        /// The segments of the node, joined with `/`, fitted to the row.
        label: String,
        /// The full path of the directory the node ends at: what folding
        /// remembers.
        path: String,
        /// The input indices of every file under the node, in tree order.
        files: Vec<usize>,
    },
    File {
        depth: usize,
        indent: usize,
        /// The file name, fitted to the row.
        name: String,
        /// Index of the file in the input.
        index: usize,
    },
}

impl Line {
    pub fn depth(&self) -> usize {
        match self {
            Line::Dir { depth, .. } | Line::File { depth, .. } => *depth,
        }
    }
}

/// The columns `done/total` takes at the end of a directory row, with the
/// space in front of it.
pub fn progress_width(total: usize) -> usize {
    2 * total.to_string().len() + 2
}

#[derive(Default)]
struct Node {
    name: String,
    children: Vec<Child>,
    dirs: HashMap<String, usize>,
}

#[derive(Clone, Copy)]
enum Child {
    Dir(usize),
    File(usize),
}

/// The tree of `paths` (new-side paths, `/`-separated) for rows of `width`
/// columns, in reading order.
pub fn build(paths: &[&str], width: usize) -> Vec<Line> {
    let nodes = trie(paths);
    let mut out = Vec::new();
    emit(&nodes, 0, 0, "", paths, width, &mut out);
    out
}

/// The input indices in the order the tree shows the files.
pub fn order(paths: &[&str]) -> Vec<usize> {
    let nodes = trie(paths);
    let mut out = Vec::new();
    files_under(&nodes, 0, &mut out);
    out
}

/// The rows left when the directories `collapsed` says are folded hide
/// what is under them.
pub fn visible<'a>(lines: &'a [Line], collapsed: impl Fn(&str) -> bool) -> Vec<&'a Line> {
    let mut out = Vec::new();
    let mut hidden_below: Option<usize> = None;
    for line in lines {
        if let Some(depth) = hidden_below {
            if line.depth() > depth {
                continue;
            }
            hidden_below = None;
        }
        if let Line::Dir { path, depth, .. } = line {
            if collapsed(path) {
                hidden_below = Some(*depth);
            }
        }
        out.push(line);
    }
    out
}

/// Every directory that holds `path`, outermost first.
pub fn ancestors(path: &str) -> Vec<&str> {
    path.match_indices('/').map(|(i, _)| &path[..i]).collect()
}

fn trie(paths: &[&str]) -> Vec<Node> {
    let mut nodes = vec![Node::default()];
    for (index, path) in paths.iter().enumerate() {
        let mut segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        segments.pop();
        let mut at = 0;
        for segment in segments {
            at = match nodes[at].dirs.get(segment) {
                Some(&next) => next,
                None => {
                    let next = nodes.len();
                    nodes.push(Node {
                        name: segment.to_string(),
                        ..Node::default()
                    });
                    nodes[at].dirs.insert(segment.to_string(), next);
                    nodes[at].children.push(Child::Dir(next));
                    next
                }
            };
        }
        nodes[at].children.push(Child::File(index));
    }
    nodes
}

fn files_under(nodes: &[Node], node: usize, out: &mut Vec<usize>) {
    for child in &nodes[node].children {
        match *child {
            Child::File(i) => out.push(i),
            Child::Dir(d) => files_under(nodes, d, out),
        }
    }
}

fn indent_of(depth: usize, width: usize) -> usize {
    // A deep tree in a narrow row keeps half of it for the names.
    (depth * INDENT).min(width / 2)
}

fn emit(
    nodes: &[Node],
    node: usize,
    depth: usize,
    parent: &str,
    paths: &[&str],
    width: usize,
    out: &mut Vec<Line>,
) {
    for child in &nodes[node].children {
        match *child {
            Child::File(index) => {
                let indent = indent_of(depth, width);
                let name = paths[index].rsplit('/').next().unwrap_or(paths[index]);
                let room = width.saturating_sub(indent + FILE_PREFIX);
                out.push(Line::File {
                    depth,
                    indent,
                    name: fit_tail(name, room),
                    index,
                });
            }
            Child::Dir(first) => {
                // The chain of directories with a single directory child.
                let mut segments = vec![nodes[first].name.as_str()];
                let mut end = first;
                while let [Child::Dir(only)] = nodes[end].children[..] {
                    end = only;
                    segments.push(nodes[end].name.as_str());
                }
                let mut files = Vec::new();
                files_under(nodes, end, &mut files);
                let reserve = DIR_PREFIX + progress_width(files.len());
                let mut path = parent.to_string();
                let mut level = depth;
                let mut rest = &segments[..];
                while !rest.is_empty() {
                    let indent = indent_of(level, width);
                    let room = width.saturating_sub(indent + reserve);
                    let mut take = 1;
                    while take < rest.len() && rest[..=take].join("/").chars().count() <= room {
                        take += 1;
                    }
                    let label = rest[..take].join("/");
                    for segment in &rest[..take] {
                        if !path.is_empty() {
                            path.push('/');
                        }
                        path.push_str(segment);
                    }
                    out.push(Line::Dir {
                        depth: level,
                        indent,
                        label: fit_head(&label, room),
                        path: path.clone(),
                        files: files.clone(),
                    });
                    rest = &rest[take..];
                    level += 1;
                }
                emit(nodes, end, level, &path, paths, width, out);
            }
        }
    }
}

/// `s` cut to `width` columns with `…` at the end.
fn fit_head(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let head: String = s.chars().take(width - 1).collect();
    format!("{head}…")
}

/// `s` cut to `width` columns with `…` at the start: the extension stays.
fn fit_tail(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let tail: String = s.chars().skip(n - (width - 1)).collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tree as text: two spaces a level, directories end with `/`.
    fn draw(paths: &[&str], width: usize) -> Vec<String> {
        build(paths, width)
            .iter()
            .map(|line| match line {
                Line::Dir { indent, label, .. } => format!("{}{label}/", " ".repeat(*indent)),
                Line::File { indent, name, .. } => format!("{}{name}", " ".repeat(*indent)),
            })
            .collect()
    }

    const WIDE: usize = 80;

    #[test]
    fn a_chain_of_single_directories_is_one_node() {
        let paths = [
            "src/features/course/features/access/api/reviews.ts",
            "src/features/course/features/access/api/index.ts",
        ];
        assert_eq!(
            draw(&paths, WIDE),
            [
                "src/features/course/features/access/api/",
                "  reviews.ts",
                "  index.ts",
            ]
        );
    }

    #[test]
    fn a_chain_stops_where_a_directory_has_more_than_one_child() {
        let paths = ["src/a/x.rs", "src/b/y.rs", "src/c.rs"];
        assert_eq!(
            draw(&paths, WIDE),
            ["src/", "  a/", "    x.rs", "  b/", "    y.rs", "  c.rs"]
        );
    }

    #[test]
    fn a_directory_holding_files_and_one_directory_is_not_joined() {
        let paths = ["lib/one/deep.rs", "lib/top.rs"];
        assert_eq!(
            draw(&paths, WIDE),
            ["lib/", "  one/", "    deep.rs", "  top.rs"]
        );
    }

    #[test]
    fn a_chain_too_wide_for_the_row_is_split_into_nested_nodes() {
        let paths = ["src/features/course/features/access/api/reviews.ts"];
        // 26 columns: 2 for the fold mark, 4 for " 0/1", 20 left for the name.
        let lines = draw(&paths, 26);
        assert_eq!(
            lines,
            [
                "src/features/course/",
                "  features/access/",
                "    api/",
                "      reviews.ts",
            ]
        );
        for line in &lines {
            assert!(!line.contains('…'), "{line}");
        }
    }

    #[test]
    fn the_split_parts_remember_their_full_paths() {
        let lines = build(&["aaa/bbb/ccc/f.rs"], 13);
        let paths: Vec<&str> = lines
            .iter()
            .filter_map(|l| match l {
                Line::Dir { path, .. } => Some(path.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(paths, ["aaa/bbb", "aaa/bbb/ccc"]);
    }

    #[test]
    fn only_a_segment_too_long_for_its_row_is_cut() {
        let lines = draw(&["averyveryverylongdirectoryname/x.rs"], 20);
        // 14 columns are left for the name: 20, less the fold mark and " 0/1".
        assert_eq!(lines[0], "averyveryvery…/");
    }

    #[test]
    fn a_file_in_the_root_is_a_single_row() {
        assert_eq!(
            build(&["README.md"], WIDE),
            [Line::File {
                depth: 0,
                indent: 0,
                name: "README.md".into(),
                index: 0
            }]
        );
    }

    #[test]
    fn a_file_and_a_directory_sharing_a_name_prefix_stay_apart() {
        let paths = [
            "src/app.rs",
            "src/app/mod.rs",
            "src/application/x.rs",
            "src/app",
        ];
        assert_eq!(
            draw(&paths, WIDE),
            [
                "src/",
                "  app.rs",
                "  app/",
                "    mod.rs",
                "  application/",
                "    x.rs",
                "  app",
            ]
        );
    }

    #[test]
    fn the_reading_order_is_kept_a_directory_stands_at_its_first_file() {
        let paths = ["b/second.rs", "README.md", "a/first.rs", "b/third.rs"];
        assert_eq!(
            draw(&paths, WIDE),
            [
                "b/",
                "  second.rs",
                "  third.rs",
                "README.md",
                "a/",
                "  first.rs"
            ]
        );
        assert_eq!(order(&paths), [0, 3, 1, 2]);
    }

    #[test]
    fn a_renamed_file_is_placed_by_its_new_path() {
        // The session passes the new side of a rename.
        assert_eq!(
            draw(&["new/place/moved.rs", "old/kept.rs"], WIDE),
            ["new/place/", "  moved.rs", "old/", "  kept.rs"]
        );
    }

    #[test]
    fn a_directory_lists_the_files_under_it_for_its_progress() {
        let lines = build(&["src/a.rs", "doc.md", "src/x/b.rs"], WIDE);
        let Line::Dir { files, .. } = &lines[0] else {
            panic!("{lines:?}")
        };
        assert_eq!(files, &[0, 2]);
    }

    #[test]
    fn a_folded_directory_hides_what_is_under_it() {
        let lines = build(&["src/a/x.rs", "src/b.rs", "top.rs"], WIDE);
        let names = |shown: Vec<&Line>| -> Vec<String> {
            shown
                .iter()
                .map(|l| match l {
                    Line::Dir { label, .. } => format!("{label}/"),
                    Line::File { name, .. } => name.clone(),
                })
                .collect()
        };
        assert_eq!(
            names(visible(&lines, |p| p == "src/a")),
            ["src/", "a/", "b.rs", "top.rs"]
        );
        assert_eq!(names(visible(&lines, |p| p == "src")), ["src/", "top.rs"]);
    }

    #[test]
    fn a_file_name_too_long_keeps_its_end() {
        let lines = draw(&["a_really_long_file_name.rs"], 14);
        assert_eq!(lines, ["…e_name.rs"]);
    }

    #[test]
    fn the_ancestors_of_a_path_are_its_directories() {
        assert_eq!(ancestors("a/b/c.rs"), ["a", "a/b"]);
        assert!(ancestors("c.rs").is_empty());
    }
}

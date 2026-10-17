//! Prompts: copying Comments to the clipboard through OSC 52, the layout of
//! the text, Comment states and Rounds.

mod support;

use support::{feature_branch, Harness};

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn unbase64(text: &str) -> String {
    let mut bytes = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes().filter(|&c| c != b'=') {
        let v = B64.iter().position(|&x| x == c).expect("base64 digit") as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    String::from_utf8(bytes).expect("utf-8 prompt")
}

/// The texts the terminal was asked to put into the clipboard so far.
fn copied(h: &mut Harness) -> Vec<String> {
    h.session()
        .take_terminal_output()
        .iter()
        .map(|seq| {
            let body = seq
                .strip_prefix("\x1b]52;c;")
                .and_then(|s| s.strip_suffix('\x07'))
                .unwrap_or_else(|| panic!("not a clipboard sequence: {seq:?}"));
            unbase64(body)
        })
        .collect()
}

#[test]
fn capital_y_copies_the_open_comments_as_a_prompt_through_osc_52() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("jjcifix this<Esc><CR>");
    h.keys("Y");
    let sent = copied(&mut h);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].contains("fix this"), "{}", sent[0]);
    assert!(sent[0].contains("feature"), "{}", sent[0]);
}

#[test]
fn space_p_p_copies_all_open_comments_like_y() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("Cifresh one<Esc><CR>");
    h.keys("<Space>pp");
    let sent = copied(&mut h);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].contains("fresh one"), "{}", sent[0]);
    assert!(h.screen().contains("Comment · sent"), "{}", h.screen());
}

#[test]
fn space_p_f_copies_only_the_comments_of_the_current_file() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("]fCia rs note<Esc><CR>[f");
    h.keys("jjciline in readme<Esc><CR>Cifile readme<Esc><CR>");
    h.keys(":comment<CR>iwhole range<Esc><CR>");
    h.keys("<Space>pf");
    let sent = copied(&mut h);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].contains("line in readme"), "{}", sent[0]);
    assert!(sent[0].contains("file readme"), "{}", sent[0]);
    assert!(!sent[0].contains("a rs note"), "{}", sent[0]);
    assert!(!sent[0].contains("whole range"), "{}", sent[0]);

    // The other Comments are still open, so `Y` takes exactly those.
    h.keys("Y");
    let rest = copied(&mut h);
    assert!(rest[0].contains("a rs note"), "{}", rest[0]);
    assert!(rest[0].contains("whole range"), "{}", rest[0]);
    assert!(!rest[0].contains("line in readme"), "{}", rest[0]);
}

#[test]
fn space_p_c_copies_the_comment_under_the_cursor_only() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("cifirst note<Esc><CR>jjcithird note<Esc><CR>");
    h.keys("<Space>pc");
    let sent = copied(&mut h);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].contains("third note"), "{}", sent[0]);
    assert!(!sent[0].contains("first note"), "{}", sent[0]);
    // Only the copied one changed state.
    let screen = h.screen();
    assert!(screen.contains("Comment · sent · new 3"), "{screen}");
    assert!(screen.contains("Comment · open · new 1"), "{screen}");

    // Away from every Comment there is nothing to copy.
    h.keys("k");
    h.keys("<Space>pc");
    // (line 2 has no Comment)
    assert!(copied(&mut h).is_empty());
    assert!(h.screen().contains("No Comment here"), "{}", h.screen());
}

#[test]
fn space_p_s_shows_the_prompt_in_an_overlay_without_sending_anything() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("Cifresh one<Esc><CR>");
    h.keys("<Space>ps");
    let screen = h.screen();
    assert!(screen.contains("You are the author"), "{screen}");
    assert!(screen.contains("fresh one"), "{screen}");
    assert!(copied(&mut h).is_empty());

    // Esc closes the overlay; the Comment is still open.
    h.keys("<Esc>");
    let screen = h.screen();
    assert!(!screen.contains("You are the author"), "{screen}");
    assert!(screen.contains("Comment · open"), "{screen}");
}

#[test]
fn prompt_show_is_the_command_form_of_the_overlay() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("Cifresh one<Esc><CR>:prompt show<CR>");
    assert!(
        h.screen().contains("You are the author"),
        "{}",
        h.screen()
    );
    assert!(copied(&mut h).is_empty());
}

#[test]
fn prompt_save_writes_the_prompt_to_a_file_and_counts_as_a_copy() {
    let fx = feature_branch();
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("prompt.md");
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("Cifresh one<Esc><CR>");
    h.keys(&format!(":prompt save {}<CR>", path.display()));
    let saved = std::fs::read_to_string(&path).expect("the prompt is saved");
    assert!(saved.contains("fresh one"), "{saved}");
    assert!(saved.starts_with("You are the author"), "{saved}");
    let screen = h.screen();
    assert!(screen.contains("Comment · sent"), "{screen}");
    assert!(screen.contains("prompt.md"), "{screen}");
    // The file is the only way out: nothing goes to the clipboard.
    assert!(copied(&mut h).is_empty());
}

#[test]
fn prompt_save_without_a_path_stays_out_of_the_working_copy() {
    let fx = feature_branch();
    let before = fx.state();
    let mut h = Harness::open(&fx, 160, 40);
    h.keys("Cifresh one<Esc><CR>:prompt save<CR>");
    let screen = h.screen();
    let saved = screen
        .split_whitespace()
        .find(|w| w.ends_with(".md") && w.starts_with('/'))
        .unwrap_or_else(|| panic!("no path in the message:\n{screen}"));
    assert!(std::fs::read_to_string(saved)
        .unwrap()
        .contains("fresh one"));
    assert_eq!(fx.state(), before);
}

#[test]
fn copying_the_same_comment_again_sends_a_different_text_so_mosh_forwards_it() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("Cifresh one<Esc><CR>");
    h.keys("<Space>pc");
    let first = copied(&mut h).remove(0);
    h.keys("<Space>pc");
    let second = copied(&mut h).remove(0);
    assert_ne!(first, second);
    assert!(first.contains("Prompt #1"), "{first}");
    assert!(second.contains("Prompt #2"), "{second}");
}

/// Index of the first screen row containing `needle`.
fn row_of(screen: &str, needle: &str) -> usize {
    screen
        .lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no row with {needle:?}:\n{screen}"))
}

/// Copy a Prompt in a session of its own, so that a later session finds it
/// in the Review.
fn copy_one_prompt(fx: &support::Fixture) {
    let mut h = Harness::open(fx, 160, 30);
    h.keys("Cifresh one<Esc><CR>Y");
    assert_eq!(copied(&mut h).len(), 1);
}

#[test]
fn commits_after_a_prompt_sit_under_a_round_header_in_the_list() {
    let fx = feature_branch();
    copy_one_prompt(&fx);
    fx.write("src/c.rs", "fn c() {}\n").commit("answer c");

    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    let (r1, a, change) = (
        row_of(&screen, "Round 1"),
        row_of(&screen, "add a"),
        row_of(&screen, "change a"),
    );
    let (r2, answer) = (
        row_of(&screen, "Round 2, after Prompt #1"),
        row_of(&screen, "answer c"),
    );
    assert!(r1 < a && a < change && change < r2 && r2 < answer, "{screen}");
}

#[test]
fn without_a_prompt_the_commit_list_has_no_round_headers() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    assert!(!h.screen().contains("Round"), "{}", h.screen());
}

#[test]
fn rounds_without_commits_get_no_header_and_their_prompts_join_the_next_one() {
    let fx = feature_branch();
    copy_one_prompt(&fx);
    // A second Prompt before the agent answered the first one.
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("Cisecond one<Esc><CR>Y");
    assert_eq!(copied(&mut h).len(), 1);
    drop(h);
    fx.write("src/c.rs", "fn c() {}\n").commit("answer c");

    let mut h = Harness::open(&fx, 160, 30);
    let screen = h.screen();
    assert_eq!(screen.matches("Round").count(), 2, "{screen}");
    assert!(screen.contains("Round 2, after #1, #2"), "{screen}");
}

#[test]
fn the_status_line_names_the_current_round() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    assert!(h.screen().contains("round:1"), "{}", h.screen());
    h.keys("Cifresh one<Esc><CR>Y");
    // The Prompt closed Round 1, even though no commit answered it yet.
    h.keys("<Esc>");
    assert!(h.screen().contains("round:2"), "{}", h.screen());
}

#[test]
fn only_open_comments_are_copied_and_they_become_sent() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    h.keys("jjciclosed one<Esc><CR>x");
    h.keys("Cifresh one<Esc><CR>");
    h.keys("Y");
    let sent = copied(&mut h);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains("fresh one"), "{}", sent[0]);
    assert!(!sent[0].contains("closed one"), "{}", sent[0]);
    let screen = h.screen();
    assert!(screen.contains("Comment · sent · file"), "{screen}");
    assert!(screen.contains("Comment · closed"), "{screen}");
    assert!(!screen.contains("Comment · open"), "{screen}");

    // Nothing is open any more: no second copy, and the Reviewer is told.
    h.keys("Y");
    assert!(copied(&mut h).is_empty());
    assert!(h.screen().contains("No open Comments"), "{}", h.screen());
}

#[test]
fn the_prompt_goes_from_general_to_file_to_line_comments_with_path_lines_and_code() {
    let fx = feature_branch();
    let mut h = Harness::open(&fx, 160, 30);
    // README.md is "# demo", "", "more": comment the third line, then the
    // file, then the whole Range, in the opposite order of the prompt.
    h.keys("jjciline note<Esc><CR>");
    h.keys("Cifile note<Esc><CR>");
    h.keys(":comment<CR>igeneral note<Esc><CR>");
    h.keys("Y");
    let prompt = copied(&mut h).remove(0);
    let at = |needle: &str| {
        prompt
            .find(needle)
            .unwrap_or_else(|| panic!("no {needle:?} in:\n{prompt}"))
    };
    assert!(at("general note") < at("file note"), "{prompt}");
    assert!(at("file note") < at("line note"), "{prompt}");
    assert!(at("file note") > at("README.md"), "{prompt}");
    assert!(at("README.md:3") < at("line note"), "{prompt}");
    assert!(at("```\nmore\n```") < at("line note"), "{prompt}");
    // The preamble is English and names the branch and the commits.
    assert!(prompt.starts_with("You are the author"), "{prompt}");
    assert!(prompt.contains("`feature`"), "{prompt}");
    assert!(prompt.contains("3 commits"), "{prompt}");
}

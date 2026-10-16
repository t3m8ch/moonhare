//! PROTOTYPE, THROWAWAY. Do not promote to production code.
//!
//! Question: "What should moonhare's main screen look like, and how do the Vim-style
//! controls from Q21 feel?"
//!
//! Plan: Three structurally different layouts of moonhare's main screen, switchable at
//! runtime with F1/F2/F3 or `:variant a|b|c`, with a one-line PROTOTYPE bar naming the
//! current variant.
//!   A "IDE-like"     commits above groups/files on the left, diff on the right
//!   B "Diff-first"   full-width diff, thin strip on top, pickers for commits/files (gc/gf)
//!   C "Review queue" one file at a time through the unreviewed queue (n/p, m = done+next)
//!
//! Everything is hardcoded fake data (a pretend agent-written "add retry with backoff"
//! Range). No git, no difftastic, no Analyst, no clipboard, nothing is persisted.
//!
//! Run:       cargo run --release
//!            cargo run --release -- --variant c
//! Dump:      cargo run --release -- --dump a 120x40 [--keys "gcjj<Esc>"]
//!            (renders one frame to plain text with ratatui's TestBackend, no TTY needed)

#![allow(dead_code)]

mod app;
mod data;
mod ui;
mod va;
mod vb;
mod vc;

use std::io;

use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Terminal;

use app::{App, Variant};

fn parse_variant(s: &str) -> Variant {
    match s {
        "b" | "B" => Variant::B,
        "c" | "C" => Variant::C,
        _ => Variant::A,
    }
}

/// vim-ish key script for --dump: plain chars plus <Esc> <CR> <Tab> <BS> <C-x> <F1> <lt>
fn parse_keys(s: &str) -> Vec<KeyEvent> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            if let Some(end) = chars[i..].iter().position(|c| *c == '>') {
                let name: String = chars[i + 1..i + end].iter().collect();
                i += end + 1;
                let ev = match name.as_str() {
                    "Esc" => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                    "CR" => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                    "Tab" => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                    "BS" => KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
                    "lt" => KeyEvent::new(KeyCode::Char('<'), KeyModifiers::NONE),
                    "F1" => KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE),
                    "F2" => KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE),
                    "F3" => KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE),
                    n if n.starts_with("C-") => KeyEvent::new(KeyCode::Char(n.chars().nth(2).unwrap()), KeyModifiers::CONTROL),
                    _ => continue,
                };
                out.push(ev);
                continue;
            }
        }
        out.push(KeyEvent::new(KeyCode::Char(chars[i]), KeyModifiers::NONE));
        i += 1;
    }
    out
}

fn dump(variant: Variant, w: u16, h: u16, keys: &str) -> io::Result<()> {
    let mut app = App::new(variant);
    for k in parse_keys(keys) {
        app.on_key(k);
    }
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| ui::draw(f, &mut app)).unwrap();
    term.draw(|f| ui::draw(f, &mut app)).unwrap(); // second pass: scroll offsets settle
    let buf = term.backend().buffer();
    for y in 0..h {
        let mut line = String::new();
        for x in 0..w {
            line.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
        }
        println!("{}", line.trim_end());
    }
    Ok(())
}

fn run(variant: Variant) -> io::Result<()> {
    let mut app = App::new(variant);
    let mut terminal = ratatui::init();
    let res = (|| -> io::Result<()> {
        loop {
            terminal.draw(|f| ui::draw(f, &mut app))?;
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    app.on_key(k);
                }
            }
            if app.quit {
                return Ok(());
            }
        }
    })();
    ratatui::restore();
    res
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let val = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    if let Some(i) = args.iter().position(|a| a == "--dump") {
        let v = parse_variant(args.get(i + 1).map(|s| s.as_str()).unwrap_or("a"));
        let size = args.get(i + 2).map(|s| s.as_str()).unwrap_or("120x40");
        let (w, h) = size.split_once('x').map(|(a, b)| (a.parse().unwrap_or(120), b.parse().unwrap_or(40))).unwrap_or((120, 40));
        return dump(v, w, h, &val("--keys").unwrap_or_default());
    }
    let v = parse_variant(&val("--variant").unwrap_or_else(|| "a".into()));
    run(v)
}

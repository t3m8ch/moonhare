//! PROTOTYPE 4, THROWAWAY. Do not promote to production code.
//!
//! Question: "Does the merged adaptive layout with leader keys, mouse and
//! text-selection-to-clipboard feel right in ghostty and in mosh+zellij?"
//!
//! Plan: ONE adaptive layout (sidebar + diff, or a 2-line strip + full-width diff when the
//! sidebar is hidden with <Space>e or the terminal is narrow), Space as the leader key with
//! a which-key popup, Ctrl-w / Ctrl-hjkl pane moves, mouse clicks and wheel, drag-to-select
//! that copies code only through a REAL OSC 52 write, Comments on a line / file / Range.
//! Prototype 3 (`../tui-layout`) stays untouched as the primary source for its variants.
//!
//! Everything is hardcoded fake data (a pretend agent-written "add retry with backoff"
//! Range). No git, no difftastic, no Analyst, nothing is persisted. The only real side
//! effect is the OSC 52 clipboard write, on purpose, so copying can be tried in ghostty
//! and over mosh+zellij.
//!
//! Run:   cargo run --release
//! Dump:  cargo run --release -- --dump 120x40 [--keys "<Space>e"] [--state] [--sel-cells]
//!        (one frame as plain text via ratatui's TestBackend, no TTY needed)
//!        key script: plain chars, <Esc> <CR> <Tab> <BS> <Space> <lt> <C-w> <Left> ...,
//!        <Wait> (let the which-key delay pass), <Resize:WxH>,
//!        <Mouse:down:X,Y> <Mouse:drag:X,Y> <Mouse:up:X,Y> <Mouse:wheel-up:X,Y> <Mouse:wheel-down:X,Y>
//! Fuzz:  cargo run -- --fuzz 300 [--seed 7]    (debug build: overflow checks on)

#![allow(dead_code)]

mod app;
mod data;
mod mouse;
mod ui;

use std::io::{self, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::Color;
use ratatui::Terminal;

use app::App;

#[derive(Clone, Debug)]
enum Input {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Wait,
}

fn parse_xy(s: &str) -> Option<(u16, u16)> {
    let (a, b) = s.split_once(|c| c == ',' || c == 'x')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

fn parse_inputs(s: &str) -> Vec<Input> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let key = |c: KeyCode, m: KeyModifiers| Input::Key(KeyEvent::new(c, m));
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            if let Some(end) = chars[i..].iter().position(|c| *c == '>') {
                let name: String = chars[i + 1..i + end].iter().collect();
                i += end + 1;
                let ev = match name.as_str() {
                    "Esc" => key(KeyCode::Esc, KeyModifiers::NONE),
                    "CR" => key(KeyCode::Enter, KeyModifiers::NONE),
                    "Tab" => key(KeyCode::Tab, KeyModifiers::NONE),
                    "BS" => key(KeyCode::Backspace, KeyModifiers::NONE),
                    "Space" => key(KeyCode::Char(' '), KeyModifiers::NONE),
                    "lt" => key(KeyCode::Char('<'), KeyModifiers::NONE),
                    "Left" => key(KeyCode::Left, KeyModifiers::NONE),
                    "Right" => key(KeyCode::Right, KeyModifiers::NONE),
                    "Up" => key(KeyCode::Up, KeyModifiers::NONE),
                    "Down" => key(KeyCode::Down, KeyModifiers::NONE),
                    "Wait" => Input::Wait,
                    n if n.starts_with("C-") => match n.chars().nth(2) {
                        Some(c) => key(KeyCode::Char(c), KeyModifiers::CONTROL),
                        None => continue,
                    },
                    n if n.starts_with("Resize:") => match parse_xy(&n[7..]) {
                        Some((w, h)) => Input::Resize(w, h),
                        None => continue,
                    },
                    n if n.starts_with("Mouse:") => {
                        let parts: Vec<&str> = n[6..].splitn(2, ':').collect();
                        let (Some(kind), Some((x, y))) = (parts.first(), parts.get(1).and_then(|p| parse_xy(p))) else { continue };
                        let kind = match *kind {
                            "down" => MouseEventKind::Down(MouseButton::Left),
                            "drag" => MouseEventKind::Drag(MouseButton::Left),
                            "up" => MouseEventKind::Up(MouseButton::Left),
                            "wheel-up" => MouseEventKind::ScrollUp,
                            "wheel-down" => MouseEventKind::ScrollDown,
                            _ => continue,
                        };
                        Input::Mouse(MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE })
                    }
                    _ => continue,
                };
                out.push(ev);
                continue;
            }
        }
        out.push(key(KeyCode::Char(chars[i]), KeyModifiers::NONE));
        i += 1;
    }
    out
}

fn describe(i: &Input) -> String {
    match i {
        Input::Key(k) => format!("key {:?}{}", k.code, if k.modifiers.is_empty() { String::new() } else { format!("+{:?}", k.modifiers) }),
        Input::Mouse(m) => format!("mouse {:?} @{},{}", m.kind, m.column, m.row),
        Input::Resize(w, h) => format!("resize {w}x{h}"),
        Input::Wait => "wait".into(),
    }
}

/// Apply one input and redraw, exactly like the real loop does after every event.
fn apply(app: &mut App, inp: &Input, term: &mut Terminal<TestBackend>) {
    match inp {
        Input::Key(k) => app.on_key(*k),
        Input::Mouse(m) => app.on_mouse(*m),
        Input::Resize(w, h) => {
            app.on_resize(*w, *h);
            term.backend_mut().resize(*w, *h);
        }
        Input::Wait => {
            app.tick(Instant::now() + Duration::from_secs(1));
        }
    }
    term.draw(|f| ui::draw(f, app)).unwrap();
}

fn dump(w: u16, h: u16, keys: &str, state: bool, sel_cells: bool) -> io::Result<()> {
    let mut app = App::new((w, h));
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| ui::draw(f, &mut app)).unwrap();
    for inp in parse_inputs(keys) {
        apply(&mut app, &inp, &mut term);
    }
    let buf = term.backend().buffer().clone();
    let (bw, bh) = (buf.area.width, buf.area.height);
    for y in 0..bh {
        let mut line = String::new();
        for x in 0..bw {
            line.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
        }
        println!("{}", line.trim_end());
    }
    if sel_cells {
        println!("--- cells with the selection background (row: x-range \"text\")");
        for y in 0..bh {
            let mut x = 0;
            while x < bw {
                if buf.cell((x, y)).map_or(false, |c| c.bg == ui::SEL_BG) {
                    let s = x;
                    let mut t = String::new();
                    while x < bw && buf.cell((x, y)).map_or(false, |c| c.bg == ui::SEL_BG) {
                        t.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
                        x += 1;
                    }
                    println!("row {y}: x={s}..{} {t:?}", x - 1);
                } else {
                    x += 1;
                }
            }
        }
    }
    if !app.clip_log.is_empty() {
        println!("--- clipboard: {} OSC 52 write(s)", app.clip_log.len());
        if let Some(raw) = app.osc_out.first() {
            let shown: String = raw.chars().take(48).collect::<String>().replace('\x1b', "␛");
            println!("first raw sequence: {shown}… ({} bytes)", raw.len());
        }
        for (n, t) in app.clip_log.iter().enumerate() {
            println!("[{}] {} chars:", n + 1, t.chars().count());
            for l in t.lines() {
                println!("  | {l}");
            }
        }
    }
    if state {
        println!("--- state: {}", app.state_line());
    }
    Ok(())
}

// ---------------------------------------------------------------- fuzz

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

fn random_input(r: &mut Rng, size: (u16, u16)) -> Input {
    const CHARS: &str = "jkhlgGvmMcCYyonN0asi[]fuwdpeSq /:?!xtrz1234ABcb.-";
    let m = |c: KeyCode, mods: KeyModifiers| Input::Key(KeyEvent::new(c, mods));
    match r.below(100) {
        0..=54 => {
            let cs: Vec<char> = CHARS.chars().collect();
            m(KeyCode::Char(cs[r.below(cs.len() as u64) as usize]), KeyModifiers::NONE)
        }
        55..=59 => m(KeyCode::Esc, KeyModifiers::NONE),
        60..=62 => m(KeyCode::Enter, KeyModifiers::NONE),
        63..=64 => m(KeyCode::Tab, KeyModifiers::NONE),
        65 => m(KeyCode::Backspace, KeyModifiers::NONE),
        66..=69 => {
            let c = ['w', 'd', 'u', 'h', 'j', 'k', 'l'][r.below(7) as usize];
            m(KeyCode::Char(c), KeyModifiers::CONTROL)
        }
        70..=77 => {
            let kind = match r.below(5) {
                0 => MouseEventKind::Down(MouseButton::Left),
                1 | 2 => MouseEventKind::Drag(MouseButton::Left),
                3 => MouseEventKind::Up(MouseButton::Left),
                _ => {
                    if r.below(2) == 0 {
                        MouseEventKind::ScrollUp
                    } else {
                        MouseEventKind::ScrollDown
                    }
                }
            };
            Input::Mouse(MouseEvent { kind, column: r.below(size.0 as u64 + 6) as u16, row: r.below(size.1 as u64 + 6) as u16, modifiers: KeyModifiers::NONE })
        }
        78..=88 => {
            let presets = [(160, 50), (120, 40), (80, 24), (50, 12), (60, 15), (99, 30), (100, 30), (200, 60), (1, 1), (30, 8)];
            if r.below(3) == 0 {
                let (w, h) = presets[r.below(presets.len() as u64) as usize];
                Input::Resize(w, h)
            } else {
                Input::Resize(1 + r.below(200) as u16, 1 + r.below(60) as u16)
            }
        }
        89..=93 => Input::Wait,
        _ => {
            let cs: Vec<char> = "ijklm<>{}".chars().collect();
            m(KeyCode::Char(cs[r.below(cs.len() as u64) as usize]), KeyModifiers::NONE)
        }
    }
}

fn fuzz(steps: usize, seed: u64) {
    let runs = 12;
    let mut failures = 0;
    for run in 0..runs {
        let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(run as u64 + 1) | 1);
        let size = (1 + rng.below(200) as u16, 1 + rng.below(60) as u16);
        let mut trace: Vec<String> = Vec::new();
        let res = catch_unwind(AssertUnwindSafe(|| {
            let mut app = App::new(size);
            let mut term = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
            term.draw(|f| ui::draw(f, &mut app)).unwrap();
            let mut cur = size;
            for _ in 0..steps {
                let inp = random_input(&mut rng, cur);
                if let Input::Resize(w, h) = inp {
                    cur = (w, h);
                }
                trace.push(describe(&inp));
                apply(&mut app, &inp, &mut term);
                app.quit = false;
                app.osc_out.clear();
            }
        }));
        if res.is_err() {
            failures += 1;
            println!("fuzz run {run} (seed {seed}, start {}x{}) PANICKED after {} inputs; last inputs:", size.0, size.1, trace.len());
            for t in trace.iter().rev().take(12).rev() {
                println!("  {t}");
            }
        }
    }
    println!("fuzz: {runs} runs x {steps} inputs, {failures} panicked");
    if failures > 0 {
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------- real terminal

fn flush_osc(app: &mut App) -> io::Result<()> {
    if app.osc_out.is_empty() {
        return Ok(());
    }
    let mut out = io::stdout();
    for s in app.osc_out.drain(..) {
        out.write_all(s.as_bytes())?;
    }
    out.flush()
}

fn run() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let size = terminal.size()?;
    let mut app = App::new((size.width, size.height));
    ratatui::crossterm::execute!(io::stdout(), EnableMouseCapture)?;
    let res = (|| -> io::Result<()> {
        let mut dirty = true;
        loop {
            if dirty {
                terminal.draw(|f| ui::draw(f, &mut app))?;
                flush_osc(&mut app)?;
                dirty = false;
            }
            if app.mouse_dirty {
                app.mouse_dirty = false;
                if app.mouse_on {
                    ratatui::crossterm::execute!(io::stdout(), EnableMouseCapture)?;
                } else {
                    ratatui::crossterm::execute!(io::stdout(), DisableMouseCapture)?;
                }
            }
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(k) if k.kind == KeyEventKind::Press => app.on_key(k),
                    Event::Mouse(m) => app.on_mouse(m),
                    Event::Resize(w, h) => app.on_resize(w, h),
                    _ => {}
                }
                dirty = true;
            } else if app.tick(Instant::now()) {
                dirty = true;
            }
            if app.quit {
                return Ok(());
            }
        }
    })();
    let _ = ratatui::crossterm::execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    res
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let val = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    if let Some(i) = args.iter().position(|a| a == "--fuzz") {
        let steps = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(300);
        let seed = val("--seed").and_then(|s| s.parse().ok()).unwrap_or(1);
        fuzz(steps, seed);
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--dump") {
        let size = args.get(i + 1).map(|s| s.as_str()).unwrap_or("120x40");
        let (w, h) = parse_xy(size).unwrap_or((120, 40));
        return dump(w, h, &val("--keys").unwrap_or_default(), has("--state"), has("--sel-cells"));
    }
    run()
}

#[allow(unused)]
fn _color_probe() -> Color {
    ui::SEL_BG
}

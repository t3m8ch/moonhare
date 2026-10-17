use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use moonhare::claude::ClaudeAnalyst;
use moonhare::git::Target;
use moonhare::github::GitHub;
use moonhare::session::Session;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;

fn main() -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("moonhare: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (width, height) = ratatui::crossterm::terminal::size().unwrap_or((80, 24));
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let language = match take_flag_value(&mut args, "--lang") {
        Ok(language) => language,
        Err(()) => {
            eprintln!(
                "usage: moonhare [--lang <code>] [<branch> | <base>..<head> | --pr <number>]"
            );
            return ExitCode::FAILURE;
        }
    };
    let forge = GitHub::new();
    let opened = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--pr", n] => match n.trim_start_matches('#').parse::<u64>() {
            Ok(n) => Session::open_pull_request(&cwd, &forge, n, width, height),
            Err(_) => Err(moonhare::session::Error(format!(
                "not a pull request number: {n}"
            ))),
        },
        [] | [_] if !args.first().is_some_and(|a| a.starts_with("--")) => {
            let target = Target::from_arg(args.first().map(String::as_str));
            Session::open_target_with_forge(&cwd, &target, &forge, width, height)
        }
        _ => {
            eprintln!("usage: moonhare [<branch> | <base>..<head> | --pr <number>]");
            return ExitCode::FAILURE;
        }
    };
    let mut session = match opened {
        Ok(s) => s,
        Err(e) => {
            eprintln!("moonhare: {e}");
            return ExitCode::FAILURE;
        }
    };

    let colorterm = std::env::var("COLORTERM").ok();
    let forced = std::env::var("MOONHARE_TRUECOLOR").is_ok_and(|v| v == "1");
    session
        .set_truecolor(forced || moonhare::color::truecolor_from_colorterm(colorterm.as_deref()));
    if let Some(language) = language {
        session.set_language(&language);
    }
    session.set_analyst(std::sync::Arc::new(ClaudeAnalyst::new()));

    let mut terminal = ratatui::init();
    let _ = execute!(io::stdout(), EnableMouseCapture);
    let result = run(&mut terminal, &mut session);
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("moonhare: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Remove `--flag <value>` from `args` and return the value. `Err` when the
/// flag has no value.
fn take_flag_value(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, ()> {
    let Some(at) = args.iter().position(|a| a == flag) else {
        return Ok(None);
    };
    if at + 1 >= args.len() {
        return Err(());
    }
    let value = args.remove(at + 1);
    args.remove(at);
    Ok(Some(value))
}

/// How often to look for the Analyst's answers while it works.
const ANALYST_POLL: Duration = Duration::from_millis(200);

fn run(terminal: &mut ratatui::DefaultTerminal, session: &mut Session) -> io::Result<()> {
    while !session.should_quit() {
        terminal.draw(|f| session.render(f.buffer_mut()))?;
        // Sleep until input, until a pause makes the screen change, or (while
        // the Analyst works) until it may have answered; never redraw an idle
        // screen.
        let wake = match (session.next_tick_in(), session.analyst_busy()) {
            (Some(due), true) => Some(due.min(ANALYST_POLL)),
            (Some(due), false) => Some(due),
            (None, true) => Some(ANALYST_POLL),
            (None, false) => None,
        };
        match wake {
            Some(wake) => {
                let waited = Instant::now();
                if event::poll(wake)? {
                    session.handle(event::read()?);
                } else {
                    session.elapse(wake.max(waited.elapsed()));
                }
            }
            None => session.handle(event::read()?),
        }
        session.tick();
        if let Some(text) = session.take_clipboard() {
            let mut out = io::stdout();
            let _ = out.write_all(moonhare::clipboard::osc52(&text).as_bytes());
            let _ = out.flush();
        }
        // Prompts come as ready OSC 52 sequences.
        let sequences = session.take_terminal_output();
        if !sequences.is_empty() {
            let mut out = io::stdout();
            for sequence in sequences {
                out.write_all(sequence.as_bytes())?;
            }
            out.flush()?;
        }
    }
    Ok(())
}

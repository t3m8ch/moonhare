use std::io;
use std::process::ExitCode;
use std::time::Duration;

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

fn run(terminal: &mut ratatui::DefaultTerminal, session: &mut Session) -> io::Result<()> {
    while !session.should_quit() {
        terminal.draw(|f| session.render(f.buffer_mut()))?;
        // Wake up now and then to pick up what the Analyst has finished.
        if event::poll(Duration::from_millis(200))? {
            session.handle(event::read()?);
        }
        session.tick();
    }
    Ok(())
}

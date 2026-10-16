use std::io;
use std::process::ExitCode;
use std::time::{Duration, Instant};

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
    let args: Vec<String> = std::env::args().skip(1).collect();
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

fn run(terminal: &mut ratatui::DefaultTerminal, session: &mut Session) -> io::Result<()> {
    while !session.should_quit() {
        terminal.draw(|f| session.render(f.buffer_mut()))?;
        let waited = Instant::now();
        if event::poll(Duration::from_millis(50))? {
            session.handle(event::read()?);
        } else {
            session.tick(waited.elapsed());
        }
    }
    Ok(())
}

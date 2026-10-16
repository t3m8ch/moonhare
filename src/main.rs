use std::io;
use std::process::ExitCode;

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
    let mut session = match Session::open(&cwd, width, height) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("moonhare: {e}");
            return ExitCode::FAILURE;
        }
    };

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
        session.handle(event::read()?);
    }
    Ok(())
}

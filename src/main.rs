use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use moonhare::claude::ClaudeAnalyst;
use moonhare::git::Target;
use moonhare::github::GitHub;
use moonhare::opening::Opening;
use moonhare::session::Session;
use moonhare::terminal_input::{self, Input, Reply, TerminalInput};
use moonhare::theme::{self, Background, Choice};
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
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
    let usage = || {
        eprintln!("usage: moonhare [--lang <code>] [--effort <low|medium|high|xhigh|max>] [--model <name>] [--theme <light|dark|auto>] [<branch> | <base>..<head> | --pr <number>]")
    };
    let (Ok(language), Ok(effort), Ok(model), Ok(theme)) = (
        take_flag_value(&mut args, "--lang"),
        take_flag_value(&mut args, "--effort"),
        take_flag_value(&mut args, "--model"),
        take_flag_value(&mut args, "--theme"),
    ) else {
        usage();
        return ExitCode::FAILURE;
    };
    let theme = theme.or_else(|| {
        std::env::var("MOONHARE_THEME")
            .ok()
            .filter(|t| !t.is_empty())
    });
    let choice = match theme.as_deref().map(str::parse::<Choice>).transpose() {
        Ok(choice) => choice.unwrap_or_default(),
        Err(e) => {
            eprintln!("moonhare: {e}");
            return ExitCode::FAILURE;
        }
    };
    // `auto` follows the background `$COLORFGBG` names (dark without it)
    // until the terminal tells its own.
    let colorfgbg = std::env::var("COLORFGBG").ok();
    let mut background = colorfgbg
        .as_deref()
        .and_then(theme::background_from_colorfgbg);
    let forge: std::sync::Arc<GitHub> = std::sync::Arc::new(GitHub::new());
    let mut started = None;
    let opened = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--pr", n] => match n.trim_start_matches('#').parse::<u64>() {
            Ok(n) => match Opening::pull_request(&cwd, forge, n, width, height) {
                Ok(opening) => {
                    // The Forge may take a while: show a loading screen that
                    // can be left, not an empty terminal.
                    let (mut t, mut input) = match start_terminal() {
                        Ok(started) => started,
                        Err(e) => {
                            eprintln!("moonhare: {e}");
                            return ExitCode::FAILURE;
                        }
                    };
                    // The Forge thread catches its panics; ratatui's hook must not see them.
                    moonhare::session::silence_worker_panics();
                    let result = wait_for_opening(&mut t, &mut input, &mut background, opening);
                    started = Some((t, input));
                    match result {
                        Ok(Some(session)) => Ok(session),
                        Ok(None) => {
                            restore();
                            return ExitCode::SUCCESS;
                        }
                        Err(e) => Err(moonhare::session::Error(e.to_string())),
                    }
                }
                Err(e) => Err(e),
            },
            Err(_) => Err(moonhare::session::Error(format!(
                "not a pull request number: {n}"
            ))),
        },
        [] | [_] if !args.first().is_some_and(|a| a.starts_with("--")) => {
            let target = Target::from_arg(args.first().map(String::as_str));
            Session::open_target_with_forge(&cwd, &target, forge, width, height)
        }
        _ => {
            usage();
            return ExitCode::FAILURE;
        }
    };
    let mut session = match opened {
        Ok(s) => s,
        Err(e) => {
            if started.is_some() {
                restore();
            }
            eprintln!("moonhare: {e}");
            return ExitCode::FAILURE;
        }
    };

    let colorterm = std::env::var("COLORTERM").ok();
    let forced = std::env::var("MOONHARE_TRUECOLOR").is_ok_and(|v| v == "1");
    session
        .set_truecolor(forced || moonhare::color::truecolor_from_colorterm(colorterm.as_deref()));
    for result in [
        effort.as_deref().map(|e| session.set_effort(e)),
        model.as_deref().map(|m| session.set_model(m)),
    ]
    .into_iter()
    .flatten()
    {
        if let Err(e) = result {
            if started.is_some() {
                restore();
            }
            eprintln!("moonhare: {e}");
            return ExitCode::FAILURE;
        }
    }
    if let Some(language) = language {
        session.set_language(&language);
    }
    session.set_terminal_background(background);
    session.set_choice(choice);
    session.set_analyst(std::sync::Arc::new(ClaudeAnalyst::new()));

    let (mut terminal, mut input) = match started.map_or_else(start_terminal, Ok) {
        Ok(started) => started,
        Err(e) => {
            eprintln!("moonhare: {e}");
            return ExitCode::FAILURE;
        }
    };
    // The diff workers catch their panics; the hooks above must not see them.
    moonhare::session::silence_worker_panics();
    let _ = execute!(io::stdout(), EnableMouseCapture);
    let result = run(&mut terminal, &mut input, &mut session);
    restore();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("moonhare: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Enter the full screen and read the terminal. Ask it for its background
/// and to report when its colour scheme changes: the replies come as input,
/// whenever they do, and the first frame does not wait for them.
fn start_terminal() -> io::Result<(ratatui::DefaultTerminal, TerminalInput)> {
    let terminal = ratatui::init();
    // ratatui's hook restores the screen; the rest is ours to undo.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        undo_terminal_modes();
        hook(info);
    }));
    let input = match TerminalInput::start() {
        Ok(input) => input,
        Err(e) => {
            restore();
            return Err(e);
        }
    };
    let mut out = io::stdout();
    let _ = out.write_all(terminal_input::QUERY_BACKGROUND.as_bytes());
    let _ = out.write_all(terminal_input::REPORT_THEME.as_bytes());
    let _ = out.flush();
    Ok((terminal, input))
}

/// Release the mouse and stop the reports of the colour scheme.
fn undo_terminal_modes() {
    let mut out = io::stdout();
    let _ = execute!(out, DisableMouseCapture);
    let _ = out.write_all(terminal_input::STOP_THEME_REPORTS.as_bytes());
    let _ = out.flush();
}

/// Leave the terminal as it was found.
fn restore() {
    undo_terminal_modes();
    ratatui::restore();
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

/// Show the loading screen until the pull request is open. `None` when the
/// Reviewer left meanwhile.
fn wait_for_opening(
    terminal: &mut ratatui::DefaultTerminal,
    input: &mut TerminalInput,
    background: &mut Option<Background>,
    mut opening: Opening,
) -> Result<Option<Session>, moonhare::session::Error> {
    let step = Duration::from_millis(100);
    let draw = |t: &mut ratatui::DefaultTerminal, o: &Opening| {
        let _ = t.draw(|f| o.render(f.buffer_mut()));
    };
    loop {
        if let Some(result) = opening.poll() {
            return result.map(Some);
        }
        draw(terminal, &opening);
        let started = Instant::now();
        match input.next(step) {
            Ok(Some(Input::Event(ev))) => opening.handle(ev),
            Ok(Some(Input::Reply(Reply::Background(b)))) => *background = Some(b),
            Ok(_) => {}
            Err(_) => std::thread::sleep(step),
        }
        opening.elapse(started.elapsed());
        if opening.cancelled() {
            return Ok(None);
        }
    }
}

/// How often to look for the Analyst's answers while it works.
const ANALYST_POLL: Duration = Duration::from_millis(200);
/// How often to look for a diff the selected file waits for.
const DIFF_POLL: Duration = Duration::from_millis(40);

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    input: &mut TerminalInput,
    session: &mut Session,
) -> io::Result<()> {
    let mut redraw = true;
    let mut last = Instant::now();
    let mut captured = true;
    while !session.should_quit() {
        if redraw {
            terminal.draw(|f| session.render(f.buffer_mut()))?;
        }
        // Sleep until input, until a pause makes the screen change, or (while
        // the Analyst works) until it may have answered, and in any case until
        // it is time to look for new commits in the branch. An idle screen is
        // redrawn only when that look found something.
        let poll = session.next_poll_in();
        let wake = match (
            session.next_tick_in(),
            session.analyst_busy() || session.forge_busy(),
        ) {
            (Some(due), true) => due.min(ANALYST_POLL),
            (Some(due), false) => due,
            (None, true) => ANALYST_POLL,
            (None, false) => poll,
        }
        .min(poll);
        // A diff, or a picture of a diagram drawn in the background, shows
        // as soon as it is there.
        let wake = if session.diff_pending() || session.drawing_busy() {
            wake.min(DIFF_POLL)
        } else {
            wake
        };
        redraw = match input.next(wake)? {
            // Typing must not starve the watch on the branch: the time spent
            // here counts too, before the event, which comes at that time.
            Some(Input::Event(event)) => {
                session.input(last.elapsed(), event);
                true
            }
            // A reply is no key: time passed without input, and the theme
            // may follow the background it tells.
            Some(Input::Reply(reply)) => {
                session.elapse(last.elapsed());
                session.terminal_reply(&reply);
                true
            }
            None => session.elapse(wake.max(last.elapsed())),
        };
        last = Instant::now();
        let busy = session.analyst_busy()
            || session.forge_busy()
            || session.diff_pending()
            || session.drawing_busy();
        session.tick();
        redraw |= busy
            || session.analyst_busy()
            || session.forge_busy()
            || session.diff_pending()
            || session.drawing_busy();
        // `:set nomouse` hands the mouse back to the terminal.
        if session.mouse_enabled() != captured {
            captured = session.mouse_enabled();
            if captured {
                execute!(io::stdout(), EnableMouseCapture)?;
            } else {
                execute!(io::stdout(), DisableMouseCapture)?;
            }
        }
        // Everything copied comes as ready OSC 52 sequences.
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

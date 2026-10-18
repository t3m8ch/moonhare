use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use moonhare::claude::ClaudeAnalyst;
use moonhare::git::Target;
use moonhare::github::GitHub;
use moonhare::opening::Opening;
use moonhare::session::Session;
use moonhare::theme::{self, Choice};
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
    // The query for the background holds the terminal in raw mode while the
    // session opens: a panic in that time must not leave it there.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = ratatui::crossterm::terminal::disable_raw_mode();
        hook(info);
    }));
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
    // Ask the terminal for its background while the session opens: `auto`
    // follows it, and so does a later `:theme auto`.
    let mut background = BackgroundProbe::new();
    let forge: std::sync::Arc<GitHub> = std::sync::Arc::new(GitHub::new());
    let mut terminal = None;
    let opened = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--pr", n] => match n.trim_start_matches('#').parse::<u64>() {
            Ok(n) => match Opening::pull_request(&cwd, forge, n, width, height) {
                Ok(opening) => {
                    background.wait();
                    // The Forge may take a while: show a loading screen that
                    // can be left, not an empty terminal.
                    let mut t = ratatui::init();
                    // The Forge thread catches its panics; ratatui's hook must not see them.
                    moonhare::session::silence_worker_panics();
                    let result = wait_for_opening(&mut t, opening);
                    terminal = Some(t);
                    match result {
                        Ok(Some(session)) => Ok(session),
                        Ok(None) => {
                            ratatui::restore();
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
            background.wait();
            usage();
            return ExitCode::FAILURE;
        }
    };
    let detected = background.wait();
    let mut session = match opened {
        Ok(s) => s,
        Err(e) => {
            if terminal.is_some() {
                ratatui::restore();
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
            if terminal.is_some() {
                ratatui::restore();
            }
            eprintln!("moonhare: {e}");
            return ExitCode::FAILURE;
        }
    }
    if let Some(language) = language {
        session.set_language(&language);
    }
    session.set_terminal_background(detected);
    session.set_choice(choice);
    session.set_analyst(std::sync::Arc::new(ClaudeAnalyst::new()));

    let mut terminal = terminal.unwrap_or_else(ratatui::init);
    // ratatui's hook restores the screen; the mouse capture is ours to release.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableMouseCapture);
        hook(info);
    }));
    // The diff workers catch their panics; the hooks above must not see them.
    moonhare::session::silence_worker_panics();
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

/// How long the terminal may take to tell its background.
const BACKGROUND_TIMEOUT: Duration = Duration::from_millis(200);

/// The terminal background, told by a thread that asks while the session
/// opens.
struct BackgroundProbe {
    asking: Option<std::thread::JoinHandle<Option<theme::Background>>>,
    told: Option<theme::Background>,
}

impl BackgroundProbe {
    /// Start asking.
    fn new() -> BackgroundProbe {
        let asking = std::thread::spawn(|| {
            let colorfgbg = std::env::var("COLORFGBG").ok();
            theme::detect(query, colorfgbg.as_deref())
        });
        BackgroundProbe {
            asking: Some(asking),
            told: None,
        }
    }

    /// The background, once the thread is done; it holds the terminal in raw
    /// mode while it asks, so wait before drawing or printing.
    fn wait(&mut self) -> Option<theme::Background> {
        if let Some(asking) = self.asking.take() {
            self.told = asking.join().ok().flatten();
        }
        self.told
    }
}

#[cfg(unix)]
fn query() -> Option<theme::Background> {
    use std::io::IsTerminal;
    if !io::stdout().is_terminal() {
        return None;
    }
    theme::query_background(BACKGROUND_TIMEOUT)
}

#[cfg(not(unix))]
fn query() -> Option<theme::Background> {
    None
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
        if event::poll(step).unwrap_or(false) {
            if let Ok(ev) = event::read() {
                opening.handle(ev);
            }
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

fn run(terminal: &mut ratatui::DefaultTerminal, session: &mut Session) -> io::Result<()> {
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
        let wake = if session.diff_pending() {
            wake.min(DIFF_POLL)
        } else {
            wake
        };
        redraw = if event::poll(wake)? {
            session.handle(event::read()?);
            // Typing must not starve the watch on the branch: the time spent
            // here counts too.
            session.watch(last.elapsed());
            true
        } else {
            session.elapse(wake.max(last.elapsed()))
        };
        last = Instant::now();
        let busy = session.analyst_busy() || session.forge_busy() || session.diff_pending();
        session.tick();
        redraw |= busy || session.analyst_busy() || session.forge_busy() || session.diff_pending();
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

//! moonhare in a real terminal (a pty): it asks for the background without
//! waiting for the reply, follows the reply and the colour-scheme reports of
//! DEC mode 2031 whenever they come, and stops the reports when it quits.
//! The preferences of every run are kept in a temporary `XDG_STATE_HOME`,
//! never in the Reviewer's own.

#![cfg(unix)]

mod support;

use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use support::feature_branch;

/// `openpty` is not thread-safe everywhere (macOS names the pty in a static
/// buffer): one at a time.
static OPENPTY: Mutex<()> = Mutex::new(());

fn openpty(cols: u16, rows: u16) -> (OwnedFd, OwnedFd) {
    let _one = OPENPTY.lock().unwrap_or_else(|e| e.into_inner());
    let mut master = 0;
    let mut slave = 0;
    let mut size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let ok = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            // `*const` on Linux, `*mut` on macOS.
            &raw mut size,
        )
    };
    assert_eq!(ok, 0, "openpty: {}", std::io::Error::last_os_error());
    unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) }
}

/// moonhare running in a pty, and everything it has written so far.
struct Terminal {
    child: Child,
    master: std::fs::File,
    output: Arc<Mutex<Vec<u8>>>,
    _stubs: tempfile::TempDir,
    _state: Option<tempfile::TempDir>,
}

impl Terminal {
    fn spawn(fx: &support::Fixture) -> Terminal {
        Terminal::spawn_with(fx, &[])
    }

    /// With preferences of its own, which no other run sees.
    fn spawn_with(fx: &support::Fixture, args: &[&str]) -> Terminal {
        let state = tempfile::tempdir().unwrap();
        let mut t = Terminal::spawn_in(fx, state.path(), args, &[]);
        t._state = Some(state);
        t
    }

    /// With its preferences under `state` (as `XDG_STATE_HOME`) and `env` set.
    fn spawn_in(
        fx: &support::Fixture,
        state: &std::path::Path,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Terminal {
        // No real `claude` or `gh` is run.
        let stubs = tempfile::tempdir().unwrap();
        for name in ["claude", "gh"] {
            let path = stubs.path().join(name);
            std::fs::write(&path, "#!/bin/sh\nexit 1\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = format!(
            "{}:{}",
            stubs.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let (master, slave) = openpty(120, 30);
        let mut command = Command::new(env!("CARGO_BIN_EXE_moonhare"));
        command
            .args(args)
            .current_dir(fx.path())
            .env("PATH", path)
            .env("TERM", "xterm-256color")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("COLORFGBG")
            .env_remove("MOONHARE_THEME")
            .env_remove("MOONHARE_TRUECOLOR")
            .env_remove("COLORTERM")
            .env("XDG_STATE_HOME", state)
            .envs(env.iter().copied())
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave));
        // The pty is the child's controlling terminal, its `/dev/tty`.
        unsafe {
            use std::os::unix::process::CommandExt;
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().expect("moonhare starts");
        // `command` holds the parent's copies of the slave until dropped: once
        // they are closed, reading the master ends when moonhare exits.
        drop(command);
        let master = std::fs::File::from(master);
        let output = Arc::new(Mutex::new(Vec::new()));
        let mut reader = master.try_clone().unwrap();
        let sink = output.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        Terminal {
            child,
            master,
            output,
            _stubs: stubs,
            _state: None,
        }
    }

    fn len(&self) -> usize {
        self.output.lock().unwrap().len()
    }

    /// Wait until what moonhare wrote after `from` holds `needle`.
    fn wait_for(&self, from: usize, needle: &[u8]) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            {
                let out = self.output.lock().unwrap();
                if out[from.min(out.len())..]
                    .windows(needle.len())
                    .any(|w| w == needle)
                {
                    return;
                }
                if Instant::now() > deadline {
                    panic!(
                        "{:?} never came; moonhare wrote:\n{}",
                        String::from_utf8_lossy(needle),
                        String::from_utf8_lossy(&out[from.min(out.len())..])
                    );
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Type, or reply as the terminal, with `bytes`.
    fn send(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).unwrap();
        self.master.flush().unwrap();
    }

    fn wait_exit(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                panic!(
                    "moonhare did not quit; it wrote:\n{}",
                    String::from_utf8_lossy(&self.output.lock().unwrap())
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

/// The SGR of the theme's ordinary text: dark 252, light 235.
const DARK_FG: &[u8] = b"[38;5;252;";
const LIGHT_FG: &[u8] = b"[38;5;235;";

#[test]
fn the_theme_follows_the_terminal_whenever_it_replies_and_reports_stop_on_quit() {
    let fx = feature_branch();
    let mut t = Terminal::spawn_with(&fx, &["--theme", "auto"]);
    t.wait_for(0, b"\x1b]11;?\x1b\\");
    t.wait_for(0, b"\x1b[?2031h");
    // The first frame does not wait for a reply: dark, as nothing tells
    // otherwise.
    t.wait_for(0, b"add a");
    t.wait_for(0, DARK_FG);

    // A late reply, in two pieces far apart: light.
    let mark = t.len();
    t.send(b"\x1b]11;rgb:ffff/");
    std::thread::sleep(Duration::from_millis(300));
    t.send(b"ffff/ffff\x1b\\");
    t.wait_for(mark, LIGHT_FG);

    // The terminal switched to a dark scheme (DEC 2031).
    let mark = t.len();
    t.send(b"\x1b[?997;1n");
    t.wait_for(mark, DARK_FG);

    // No reply became keys: `:q` quits at once.
    t.send(b":q\r");
    assert!(t.wait_exit().success());
    let out = t.output.lock().unwrap().clone();
    let on = find(&out, b"\x1b[?2031h").unwrap();
    let off = find(&out, b"\x1b[?2031l").expect("the reports are stopped on quit");
    assert!(off > on);
}

#[test]
fn a_chosen_theme_ignores_the_reply_until_theme_auto() {
    let fx = feature_branch();
    let mut t = Terminal::spawn_with(&fx, &["--theme", "dark"]);
    t.wait_for(0, b"\x1b]11;?\x1b\\");
    t.wait_for(0, b"add a");
    t.wait_for(0, DARK_FG);
    t.send(b"\x1b]11;rgb:ffff/ffff/ffff\x07");
    std::thread::sleep(Duration::from_millis(300));
    let mark = t.len();
    // `:theme auto` follows the background told, and asks anew.
    t.send(b":theme auto\r");
    t.wait_for(mark, b"\x1b]11;?\x1b\\");
    t.wait_for(mark, LIGHT_FG);
    let out = t.output.lock().unwrap().clone();
    assert_eq!(find(&out[..mark], LIGHT_FG), None, "dark was chosen");
    t.send(b":q\r");
    assert!(t.wait_exit().success());
}

#[test]
fn with_nothing_chosen_the_theme_is_dark_even_on_a_light_terminal() {
    let fx = feature_branch();
    let mut t = Terminal::spawn(&fx);
    t.wait_for(0, b"add a");
    t.wait_for(0, DARK_FG);
    let mark = t.len();
    t.send(b"\x1b]11;rgb:ffff/ffff/ffff\x07");
    std::thread::sleep(Duration::from_millis(300));
    t.send(b":q\r");
    assert!(t.wait_exit().success());
    let out = t.output.lock().unwrap().clone();
    assert_eq!(find(&out[mark..], LIGHT_FG), None, "no auto-detection");
}

#[test]
fn the_theme_command_holds_in_the_next_run_and_moonhare_theme_outranks_it() {
    let fx = feature_branch();
    let state = tempfile::tempdir().unwrap();
    let file = state.path().join("moonhare/preferences");

    let mut t = Terminal::spawn_in(&fx, state.path(), &[], &[]);
    t.wait_for(0, b"add a");
    let mark = t.len();
    t.send(b":theme light\r");
    t.wait_for(mark, LIGHT_FG);
    t.send(b":q\r");
    assert!(t.wait_exit().success());
    let saved = std::fs::read_to_string(&file).unwrap();
    assert!(saved.contains("theme = light"), "{saved}");

    // The next run is light from its first frame.
    let mut t = Terminal::spawn_in(&fx, state.path(), &[], &[]);
    t.wait_for(0, b"add a");
    t.wait_for(0, LIGHT_FG);
    t.send(b":q\r");
    assert!(t.wait_exit().success());

    // `MOONHARE_THEME=dark` holds for its run and leaves the file alone.
    let mut t = Terminal::spawn_in(&fx, state.path(), &[], &[("MOONHARE_THEME", "dark")]);
    t.wait_for(0, b"add a");
    t.wait_for(0, DARK_FG);
    t.send(b":q\r");
    assert!(t.wait_exit().success());
    let out = t.output.lock().unwrap().clone();
    assert_eq!(find(&out, LIGHT_FG), None);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), saved);
}

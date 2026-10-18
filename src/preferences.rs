//! Preferences: what the Reviewer chose once and wants in every repository
//! and every run (the theme, the diff layout), kept in one small file
//! outside any clone (ADR 0008).
//!
//! The file is `key = value` lines. Lines that do not read as such, and keys
//! moonhare does not know, are kept as they are when it is rewritten; a file
//! that does not read at all counts as empty. A write goes to a temporary file
//! beside it, which is then renamed over it.

use std::path::{Path, PathBuf};

/// The name of the file in its directory.
const FILE: &str = "preferences";

/// Larger files are not ours and are not read.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// The preferences of the Reviewer, as read from their file.
#[derive(Debug, Clone, Default)]
pub struct Preferences {
    /// `None`: kept in memory only, never written (tests, no home).
    file: Option<PathBuf>,
    lines: Vec<String>,
}

/// The directory of the preferences: `$XDG_STATE_HOME/moonhare` when the
/// variable holds an absolute path, else `~/Library/Application
/// Support/moonhare` on macOS and `~/.local/state/moonhare` elsewhere.
/// `None` without a home.
pub fn dir_from(xdg_state_home: Option<&str>, home: Option<&str>, macos: bool) -> Option<PathBuf> {
    if let Some(xdg) = xdg_state_home.filter(|x| Path::new(x).is_absolute()) {
        return Some(Path::new(xdg).join("moonhare"));
    }
    let home = Path::new(home.filter(|h| !h.is_empty())?);
    Some(if macos {
        home.join("Library/Application Support/moonhare")
    } else {
        home.join(".local/state/moonhare")
    })
}

/// [`dir_from`] the environment of this process.
pub fn default_dir() -> Option<PathBuf> {
    let var = |name| std::env::var(name).ok();
    dir_from(
        var("XDG_STATE_HOME").as_deref(),
        var("HOME").as_deref(),
        cfg!(target_os = "macos"),
    )
}

impl Preferences {
    /// The preferences kept in `dir`; empty when there are none or the file
    /// does not read.
    pub fn open(dir: &Path) -> Preferences {
        let file = dir.join(FILE);
        Preferences {
            lines: read_lines(&file),
            file: Some(file),
        }
    }

    /// Where they are kept; `None` when only in memory.
    pub fn path(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// The value of `key`, if set.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines
            .iter()
            .rev()
            .find_map(|line| entry(line).filter(|(k, _)| *k == key).map(|(_, v)| v))
    }

    /// Set `key` to `value` and save. The file is read anew first, so what
    /// another moonhare saved meanwhile is kept.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        if let Some(file) = &self.file {
            self.lines = read_lines(file);
        }
        let line = format!("{key} = {value}");
        let mut found = false;
        self.lines.retain_mut(|l| {
            if entry(l).is_some_and(|(k, _)| k == key) {
                if found {
                    return false;
                }
                found = true;
                *l = line.clone();
            }
            true
        });
        if !found {
            self.lines.push(line);
        }
        let Some(file) = &self.file else {
            return Ok(());
        };
        write(file, &self.lines)
            .map_err(|e| format!("Cannot save preferences to {}: {e}", file.display()))
    }
}

/// `key = value` of a line, trimmed; `None` for any other line.
fn entry(line: &str) -> Option<(&str, &str)> {
    if line.trim_start().starts_with('#') {
        return None;
    }
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    (!key.is_empty()).then(|| (key, value.trim()))
}

fn read_lines(file: &Path) -> Vec<String> {
    let read = || -> Option<Vec<String>> {
        if std::fs::metadata(file).ok()?.len() > MAX_FILE_BYTES {
            return None;
        }
        let text = String::from_utf8(std::fs::read(file).ok()?).ok()?;
        if text.contains('\0') {
            return None;
        }
        Some(text.lines().map(str::to_string).collect())
    };
    read().unwrap_or_default()
}

fn write(file: &Path, lines: &[String]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = file.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    for line in lines {
        writeln!(tmp, "{line}")?;
    }
    tmp.as_file().sync_all()?;
    tmp.persist(file).map_err(|e| e.error)?;
    Ok(())
}

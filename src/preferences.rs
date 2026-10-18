//! Preferences: what the Reviewer chose once and wants in every repository
//! and every run (the theme, the diff layout), kept in one small file
//! outside any clone (ADR 0008).
//!
//! The file is `key = value` lines. Lines that do not read as such, and keys
//! moonhare does not know, are kept as they are when it is rewritten; a file
//! that does not read at all counts as empty and is never overwritten. A
//! write goes to a temporary file beside it (beside the target of a symbolic
//! link), which is then renamed over it.

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
            lines: read_lines(&file).unwrap_or_default(),
            file: Some(file),
        }
    }

    /// The value of `key`, if set.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines
            .iter()
            .rev()
            .find_map(|line| entry(line).filter(|(k, _)| *k == key).map(|(_, v)| v))
    }

    /// Set `key` to `value` and save. The file is read anew first, so what
    /// another moonhare saved meanwhile is kept. A file that cannot be read,
    /// or reads as damaged, is left as it is: `Err`, and the value holds in
    /// memory only.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        if let Some(file) = self.file.clone() {
            match read_lines(&file) {
                Ok(lines) => self.lines = lines,
                Err(e) => {
                    self.set_in_memory(key, value);
                    return Err(format!(
                        "Cannot save preferences: {} {e}, left as it is",
                        file.display()
                    ));
                }
            }
        }
        self.set_in_memory(key, value);
        let Some(file) = &self.file else {
            return Ok(());
        };
        write(file, &self.lines)
            .map_err(|e| format!("Cannot save preferences to {}: {e}", file.display()))
    }

    fn set_in_memory(&mut self, key: &str, value: &str) {
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

/// The lines of `file`; none when it is missing. `Err` (the end of a
/// sentence naming the file) when it is there but cannot be read, or is too
/// large, not UTF-8 or holds NUL: damaged, or not ours.
fn read_lines(file: &Path) -> Result<Vec<String>, String> {
    use std::io::ErrorKind;
    let bytes = match std::fs::metadata(file) {
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(Vec::new())
        }
        Err(e) => return Err(format!("cannot be read ({e})")),
        Ok(meta) if meta.len() > MAX_FILE_BYTES => return Err("is too large".into()),
        Ok(_) => std::fs::read(file).map_err(|e| format!("cannot be read ({e})"))?,
    };
    match String::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') => Ok(text.lines().map(str::to_string).collect()),
        _ => Err("is not text".into()),
    }
}

/// Where `file` really is: a symbolic link (as dotfile managers make) is
/// followed, even to a file not there yet, so that the rename replaces the
/// target and not the link.
fn resolve(file: &Path) -> PathBuf {
    let mut path = file.to_path_buf();
    // A loop of links ends somewhere; the write then fails on it.
    for _ in 0..40 {
        let Ok(target) = std::fs::read_link(&path) else {
            break;
        };
        path = match path.parent() {
            Some(dir) => dir.join(target),
            None => target,
        };
    }
    path
}

fn write(file: &Path, lines: &[String]) -> std::io::Result<()> {
    use std::io::Write;
    let file = &resolve(file);
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

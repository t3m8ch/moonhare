//! A stand-in `gh` executable for the GitHub adapter.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

/// A stand-in `gh`: logs its arguments, prints canned stdout and stderr and
/// exits with a canned status.
pub struct FakeGh {
    dir: tempfile::TempDir,
}

impl FakeGh {
    pub fn replying(stdout: &str) -> Self {
        Self::with(stdout, "", 0)
    }

    pub fn with(stdout: &str, stderr: &str, status: i32) -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("stdout"), stdout).unwrap();
        fs::write(dir.path().join("stderr"), stderr).unwrap();
        let script = format!(
            "#!/bin/sh\n[ \"$1\" = --probe ] && exit 0\nd='{}'\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> \"$d/args\"; done\n\
             printf '<<end>>\\n' >> \"$d/args\"\ncat \"$d/stdout\"\ncat \"$d/stderr\" >&2\nexit {status}\n",
            dir.path().display()
        );
        let path = dir.path().join("gh");
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        // Another test thread may fork while the script is still open for
        // writing, which makes exec fail with ETXTBSY. Run it once to be sure
        // it is settled.
        let probe = || std::process::Command::new(&path).arg("--probe").status();
        while probe().is_err() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        FakeGh { dir }
    }

    pub fn program(&self) -> PathBuf {
        self.dir.path().join("gh")
    }

    /// One entry per invocation, arguments joined by spaces.
    pub fn calls(&self) -> Vec<String> {
        let log = fs::read_to_string(self.dir.path().join("args")).unwrap_or_default();
        log.split("<<end>>\n")
            .filter(|c| !c.is_empty())
            .map(|c| c.lines().collect::<Vec<_>>().join(" "))
            .collect()
    }
}

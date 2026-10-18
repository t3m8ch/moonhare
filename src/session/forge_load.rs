//! The pull request of the branch, asked of the Forge in the background so
//! that the first frame does not wait for the network.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::forge::ForgeError;

/// The name of the Forge's threads: they catch their own panics, so the
/// panic hooks must not see them (see [`super::silence_worker_panics`]).
pub(crate) const WORKER: &str = "moonhare-forge";

/// Run `work` on a thread named [`WORKER`].
pub(crate) fn spawn_worker(work: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new()
        .name(WORKER.to_string())
        .spawn(work);
}

type Answer = Result<Option<PullRequest>, ForgeError>;

pub(super) struct ForgeLoad {
    /// When the request was sent, while it has no answer.
    since: Option<Duration>,
    sender: Sender<Answer>,
    receiver: Receiver<Answer>,
}

impl Default for ForgeLoad {
    fn default() -> Self {
        let (sender, receiver) = channel();
        ForgeLoad {
            since: None,
            sender,
            receiver,
        }
    }
}

impl Session {
    pub(super) fn request_pull_request(&mut self, forge: Arc<dyn Forge>, branch: String) {
        self.forge_load.since = Some(self.clock());
        let sender = self.forge_load.sender.clone();
        spawn_worker(move || {
            let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                forge.pull_request_for_branch(&branch)
            }))
            .unwrap_or_else(|_| Err(ForgeError::Failed("the Forge request crashed".into())));
            let _ = sender.send(answer);
        });
    }

    /// Whether the Forge still owes the pull request.
    pub fn forge_busy(&self) -> bool {
        self.forge_load.since.is_some()
    }

    pub(super) fn forge_tick(&mut self) {
        if let Ok(answer) = self.forge_load.receiver.try_recv() {
            self.take_pull_request(answer);
        }
    }

    /// Block until the Forge has answered.
    pub fn wait_for_forge(&mut self) {
        if self.forge_busy() {
            if let Ok(answer) = self.forge_load.receiver.recv() {
                self.take_pull_request(answer);
            }
        }
    }

    fn take_pull_request(&mut self, answer: Answer) {
        self.forge_load.since = None;
        match answer {
            Ok(Some(pr)) => self.pull_request = Some(pr),
            Ok(None) => self.forge_note = Some("the branch has no pull request".into()),
            // No Forge to ask is not a failure: the Review goes on without a
            // pull request, and `:pr` tells why.
            Err(ForgeError::Unavailable(why)) => self.forge_note = Some(why),
            Err(e) => {
                self.forge_note = Some(e.to_string());
                self.warn(format!("⚠ {e}"));
            }
        }
        if let Some(analyst) = self.analysis.analyst.clone() {
            self.start_analysis(analyst);
        }
    }

    /// `:pr`: the pull request of the Range, or why there is none.
    pub(super) fn pr_command(&mut self) {
        self.message = Some(match (&self.pull_request, &self.forge_note) {
            (Some(pr), _) => format!("PR #{}: {} (into {})", pr.number, pr.title, pr.base_branch),
            (None, _) if self.forge_busy() => "PR: loading…".to_string(),
            (None, Some(why)) => format!("PR: — {why}"),
            (None, None) => "PR: — not looked up: the Range is not a branch".to_string(),
        });
    }

    /// The status line with a note in front of it while the pull request is
    /// awaited.
    pub(super) fn forge_status(&self, rest: &str) -> String {
        match self.forge_load.since {
            Some(since) => format!(
                "{} PR: loading… {}s  {rest}",
                super::analysis::spinner(self.clock()),
                self.clock().saturating_sub(since).as_secs()
            ),
            None => rest.to_string(),
        }
    }
}

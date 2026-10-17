//! The pull request of the branch, asked of the Forge in the background so
//! that the first frame does not wait for the network.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::forge::ForgeError;

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
        std::thread::spawn(move || {
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
            Ok(pr) => self.pull_request = pr,
            Err(e) => self.warn(format!("⚠ {e}")),
        }
        if let Some(analyst) = self.analysis.analyst.clone() {
            self.start_analysis(analyst);
        }
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

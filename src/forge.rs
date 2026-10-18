//! The Forge: the code-hosting service behind the repository.
//!
//! Nothing here knows about a particular service. A new Forge is a new
//! implementation of [`Forge`]. Every operation only reads: nothing is ever
//! written to the Forge.

use std::fmt;
use std::path::Path;

/// A pull request as the Forge describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub body: String,
    /// Name of the branch the pull request targets.
    pub base_branch: String,
}

/// Why the Forge could not answer. The tool carries on without it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeError {
    /// There is no Forge to ask: no remote on it, its client is not
    /// installed or not logged in. Not a failure: a pull request is optional,
    /// so the Review goes on without one and says why only when asked.
    Unavailable(String),
    /// The Forge could not be reached.
    Network(String),
    /// The Forge answered that there is no such pull request.
    NotFound(String),
    /// Anything else, such as an unexpected answer.
    Failed(String),
}

impl fmt::Display for ForgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ForgeError::Unavailable(m)
            | ForgeError::Network(m)
            | ForgeError::NotFound(m)
            | ForgeError::Failed(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ForgeError {}

pub trait Forge: Send + Sync {
    /// The pull request whose head is `branch`, if there is one.
    fn pull_request_for_branch(&self, branch: &str) -> Result<Option<PullRequest>, ForgeError>;

    /// The pull request with this number.
    fn pull_request(&self, number: u64) -> Result<PullRequest, ForgeError>;

    /// Bring the head of pull request `number` into the clone at `repo` under
    /// a ref of moonhare's own, without creating a branch or touching the
    /// working copy. Returns the name of that ref.
    fn fetch_pull_request_head(&self, repo: &Path, number: u64) -> Result<String, ForgeError>;
}

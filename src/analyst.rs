//! The Analyst: the LLM that moonhare runs to analyze a Range for the Reviewer.
//!
//! Nothing here knows which agent does the work. A new Analyst is a new
//! implementation of [`Analyst`]; the first one is [`crate::claude`].

use std::fmt;
use std::path::PathBuf;

use crate::forge::PullRequest;

/// What the Analyst is asked to summarize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryRequest {
    /// The clone holding the Range.
    pub repo: PathBuf,
    pub base: String,
    pub head: String,
    /// Language of the answer, as a code: `ru`, `en`.
    pub language: String,
    /// Title and description of the pull request, when the Forge gave them.
    pub pull_request: Option<PullRequest>,
}

/// The Analyst's account of what a Range changes and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub text: String,
}

/// Why the Analyst could not answer. The tool carries on without it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalystError {
    /// The agent is not installed.
    Unavailable(String),
    /// The Reviewer is not logged in to the agent.
    NotLoggedIn(String),
    /// Anything else: a crash, an answer that does not fit the schema.
    Failed(String),
}

impl fmt::Display for AnalystError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnalystError::Unavailable(m)
            | AnalystError::NotLoggedIn(m)
            | AnalystError::Failed(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for AnalystError {}

pub trait Analyst: Send + Sync {
    /// Summarize the Range. May take minutes; callers run it in the background.
    fn summarize(&self, request: &SummaryRequest) -> Result<Summary, AnalystError>;
}

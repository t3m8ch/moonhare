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

/// What the Reviewer wants explained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplainTarget {
    /// One hunk of a file: its path and the changed lines as shown, each
    /// prefixed with `-`, `+` or a space.
    Hunk { path: String, text: String },
    /// A whole file of the Range.
    File { path: String },
}

/// The Analyst is asked to explain a hunk or a file of a Range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainRequest {
    /// The clone holding the Range.
    pub repo: PathBuf,
    pub base: String,
    pub head: String,
    /// Language of the answer, as a code: `ru`, `en`.
    pub language: String,
    pub pull_request: Option<PullRequest>,
    pub target: ExplainTarget,
}

/// The Analyst's explanation of a hunk or a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
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

    /// Split the changed files of the Range into Groups. May take minutes;
    /// callers run it in the background.
    fn group(&self, _request: &GroupRequest) -> Result<Grouping, AnalystError> {
        Err(AnalystError::Failed(
            "this Analyst cannot group files".into(),
        ))
    }

    /// Explain a hunk or a file on demand. May take minutes; callers run it
    /// in the background. An Analyst that cannot explain keeps this default.
    fn explain(&self, _request: &ExplainRequest) -> Result<Explanation, AnalystError> {
        Err(AnalystError::Failed(
            "this Analyst cannot explain code".into(),
        ))
    }
}

/// What the Analyst is asked to split into Groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRequest {
    /// The clone holding the Range.
    pub repo: PathBuf,
    pub base: String,
    pub head: String,
    /// Language of the names and descriptions, as a code: `ru`, `en`.
    pub language: String,
    /// Title and description of the pull request, when the Forge gave them.
    pub pull_request: Option<PullRequest>,
    /// Every file the Range changes.
    pub files: Vec<String>,
}

/// One Group as the Analyst names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    /// One line on what the files have in common.
    pub description: String,
    /// The changes need no close reading.
    pub mechanical: bool,
    pub files: Vec<String>,
}

/// The Analyst's Groups, in the recommended reading order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grouping {
    pub groups: Vec<Group>,
}

//! The working tree's changes as the Changes panel shows them: what is staged,
//! what is not, and how many lines each file adds and removes.

use serde::Serialize;
use thiserror::Error;

/// One changed file. `path` is relative to the repository, which is also what
/// staging it takes back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    pub add: usize,
    pub del: usize,
}

/// A file with both staged and unstaged edits is in both lists, with each
/// side's own counts — as `git status` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingChanges {
    pub staged: Vec<ChangedFile>,
    pub unstaged: Vec<ChangedFile>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GitChangesError {
    #[error("the open folder is not in a git repository")]
    NotARepository,
    #[error("not a path inside the repository: {0}")]
    InvalidPath(String),
    #[error("the commit message is empty")]
    EmptyMessage,
    #[error("nothing is staged to commit")]
    NothingStaged,
    #[error("git user.name and user.email are not set; set them with git config")]
    MissingIdentity,
    #[error("git: {0}")]
    Git(String),
}

//! One folder of the open workspace as the Files tab draws it: what is in it,
//! and where git sees a change.

use serde::Serialize;
use thiserror::Error;

/// How git sees a file: in the working tree or the index, whichever says
/// more. A deleted file is not on disk to list; the Changes tab has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileStatus {
    Modified,
    /// New and staged.
    Added,
    /// New and not yet known to git.
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeEntry {
    pub name: String,
    /// Relative to the open folder, `/`-separated — what the next listing takes.
    pub path: String,
    pub is_dir: bool,
    /// A file's own; `None` when it is unchanged, and always for a folder.
    pub status: Option<FileStatus>,
    /// A folder with a changed file somewhere under it.
    pub changed: bool,
}

/// Folders first, then files, each by name ignoring case — as a file
/// explorer lists them. At most `MAX_ENTRIES`; `more` counts the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderListing {
    pub entries: Vec<TreeEntry>,
    pub more: usize,
}

/// A folder of generated files can hold tens of thousands; nobody scrolls
/// those, and drawing them stalls the panel.
pub const MAX_ENTRIES: usize = 500;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FileTreeError {
    #[error("not a folder inside the open one: {0}")]
    InvalidPath(String),
    #[error("could not list {0}: {1}")]
    Io(String, String),
}

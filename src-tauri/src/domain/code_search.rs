//! What a search of the open folder's code answers with. The ranking is
//! `services::code_search`; these are the shapes the model and the window
//! read.

use serde::{Deserialize, Serialize};

/// Which kind of evidence put a match in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchSource {
    /// A declaration named as the query spelled it.
    Symbol,
    /// Near the query in meaning.
    Semantic,
    /// Shares words with the query.
    Lexical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeMatch {
    /// Relative to the folder, `/`-separated.
    pub path: String,
    /// 1-based, inclusive — what `readFile` takes.
    pub start_line: u32,
    pub end_line: u32,
    /// The enclosing declarations (`RepoIndexer.sync`) or headings.
    pub name: Option<String>,
    pub text: String,
    pub source: MatchSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMeta {
    /// The kinds of evidence that were consulted — `semantic` is missing
    /// when there was nothing embedded to consult.
    pub tiers_used: Vec<MatchSource>,
    /// The results are probably not what was asked for.
    pub weak: bool,
    /// What to do next, addressed to the model.
    pub hint: Option<String>,
}

/// One commit as search weighs it: what it said, and the files it touched,
/// relative to the searched folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitNote {
    pub message: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeSearchResult {
    pub matches: Vec<CodeMatch>,
    pub meta: SearchMeta,
}

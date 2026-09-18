//! The repository's own instructions to agents — `AGENTS.md` and its
//! equivalents — which the prompt carries on every turn (CA-9.3).
//!
//! New here: Alfa Atlas had no such file. Its "rules" were documentation
//! standards checked against a document, not text handed to the model.

use serde::Serialize;

/// Looked for at the root of the open folder, in this order. `AGENTS.md` is
/// the shared convention; `CLAUDE.md` is what many repositories have instead,
/// and often beside it. Both are read when both are there — the second
/// usually adds to the first rather than repeating it, and when it is a
/// symlink to it, it is read once.
///
/// ponytail: root only, and `@file` imports are not followed. A nested
/// `AGENTS.md` for a subdirectory comes when a monorepo asks for it.
pub const RULE_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md"];

/// Per file. The whole text goes out on every request, so a file that has
/// grown into a design document is cut rather than allowed to crowd out the
/// conversation; the model is told where it was cut and can read the rest.
pub const MAX_RULE_CHARS: usize = 20_000;

/// One file as the prompt carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleFile {
    /// Relative to the open folder: `AGENTS.md`.
    pub name: String,
    pub content: String,
    /// `content` is the first [`MAX_RULE_CHARS`] of a longer file.
    pub truncated: bool,
}

impl RuleFile {
    pub fn new(name: &str, text: &str) -> RuleFile {
        let cut = text.char_indices().nth(MAX_RULE_CHARS).map(|(at, _)| at);
        RuleFile {
            name: name.to_string(),
            content: cut.map_or(text, |at| &text[..at]).to_string(),
            truncated: cut.is_some(),
        }
    }
}

/// One row of the rules tab. `error` when the file is there but could not be
/// used — not text, or a link out of the folder — and then it has no switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleListItem {
    pub name: String,
    /// Canonical, and the key the switch is stored under.
    pub path: String,
    pub enabled: bool,
    pub content: String,
    pub truncated: bool,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_file_is_kept_whole() {
        let rule = RuleFile::new("AGENTS.md", "Run cargo test.");
        assert_eq!(rule.content, "Run cargo test.");
        assert!(!rule.truncated);
    }

    /// Counted in characters and cut on one, or a Cyrillic file would panic
    /// on a byte boundary.
    #[test]
    fn a_long_file_is_cut_at_the_limit_on_a_character() {
        let exact = "я".repeat(MAX_RULE_CHARS);
        assert!(!RuleFile::new("AGENTS.md", &exact).truncated);

        let rule = RuleFile::new("AGENTS.md", &format!("{exact}ё"));
        assert!(rule.truncated);
        assert_eq!(rule.content, exact);
    }
}

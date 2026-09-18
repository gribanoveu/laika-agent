//! Agent Skills (https://agentskills.io/specification): what a `SKILL.md` is,
//! and which ones are valid.
//!
//! Ported from Alfa Atlas `domain/agent_skills.rs`. What did not come across:
//!
//! - **The bundled catalog and `SkillSource`.** Atlas shipped five skills for
//!   writing documentation; none of them is about code, and a source enum with
//!   one variant distinguishes nothing. It comes back with the first skill
//!   worth shipping.
//! - **`search` and its ranking.** Atlas never showed the model its catalog —
//!   a skill existed only if a substring search found it, and upstream kept a
//!   test listing every phrase each skill had to answer to, because no
//!   stemming meant "задачу" and "задача" were different words. Here the
//!   catalog is short (the user's own skills) and goes into the prompt as
//!   names and descriptions, so the model picks by reading, which is what it
//!   is good at. `prompt::skills_block` says what happens when it is not short.
//! - **`requires-project`.** It hid a skill while no project was open; every
//!   turn here runs in an open folder.
//!
//! No I/O — the folder under the app directory is `infra::skills_store`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const NAME_MAX_CHARS: usize = 64;
pub const DESCRIPTION_MAX_CHARS: usize = 1024;

/// What the model is shown before it loads a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SkillError {
    #[error("SKILL.md is missing YAML frontmatter")]
    MissingFrontmatter,
    #[error("invalid SKILL.md frontmatter: {0}")]
    InvalidFrontmatter(String),
    #[error("skill name {0:?} is invalid: {1}")]
    InvalidName(String, String),
    #[error("skill name {0:?} does not match directory name {1:?}")]
    NameMismatch(String, String),
    #[error("description must be 1–{DESCRIPTION_MAX_CHARS} characters")]
    InvalidDescription,
    #[error("no skill named {0:?} — use a name from the Skills list")]
    NotFound(String),
    #[error("path escapes the skill folder: {0}")]
    PathEscape(String),
    #[error("{0}")]
    Io(String),
}

/// The frontmatter fields this app reads. The spec's optional ones
/// (`license`, `compatibility`, `metadata`, `allowed-tools`) are accepted by
/// being ignored — serde skips unknown keys — so a skill written for another
/// agent loads here unchanged. `allowed-tools` in particular is not honoured:
/// the approval gate decides what runs, not the skill asking.
#[derive(Deserialize)]
struct Frontmatter {
    name: String,
    description: String,
}

/// A valid `SKILL.md`: its meta and the markdown after the frontmatter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSkill {
    pub meta: SkillMeta,
    pub body: String,
}

/// Spec `name`: 1–64 chars of `[a-z0-9-]`, no leading, trailing or doubled
/// hyphen. Also what keeps a name safe to join onto the skills folder.
pub fn validate_skill_name(name: &str) -> Result<(), SkillError> {
    let invalid = |why: &str| Err(SkillError::InvalidName(name.to_string(), why.to_string()));
    if name.is_empty() || name.chars().count() > NAME_MAX_CHARS {
        return invalid(&format!("must be 1–{NAME_MAX_CHARS} characters"));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return invalid("must not start or end with a hyphen");
    }
    if name.contains("--") {
        return invalid("must not contain consecutive hyphens");
    }
    if !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return invalid("only lowercase letters, digits, and hyphens are allowed");
    }
    Ok(())
}

/// Parse `SKILL.md` found in the folder `dir_name`. The spec requires the two
/// names to agree; a skill whose folder was renamed would otherwise be listed
/// under one name and loaded under another.
pub fn parse_skill_md(contents: &str, dir_name: &str) -> Result<ParsedSkill, SkillError> {
    let (yaml, body) = split_frontmatter(contents).ok_or(SkillError::MissingFrontmatter)?;
    let front: Frontmatter =
        yaml_serde::from_str(yaml).map_err(|e| SkillError::InvalidFrontmatter(e.to_string()))?;
    validate_skill_name(&front.name)?;
    let description = front.description.trim();
    if description.is_empty() || description.chars().count() > DESCRIPTION_MAX_CHARS {
        return Err(SkillError::InvalidDescription);
    }
    if front.name != dir_name {
        return Err(SkillError::NameMismatch(front.name, dir_name.to_string()));
    }
    Ok(ParsedSkill {
        meta: SkillMeta { name: front.name, description: description.to_string() },
        body: body.to_string(),
    })
}

fn split_frontmatter(contents: &str) -> Option<(&str, &str)> {
    let text = contents.trim_start_matches('\u{feff}');
    let rest = text.strip_prefix("---")?;
    let rest = rest.strip_prefix('\r').unwrap_or(rest).strip_prefix('\n')?;
    let close = rest.find("\n---")?;
    let yaml = &rest[..close];
    let after = &rest[close + "\n---".len()..];
    let body = after.strip_prefix("\r\n").or_else(|| after.strip_prefix('\n')).unwrap_or(after);
    Some((yaml, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(name: &str, description: &str, body: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n{body}")
    }

    #[test]
    fn minimal_frontmatter_parses_and_the_body_is_what_follows_it() {
        let parsed = parse_skill_md(&md("code-review", "Reviews PRs when asked.", "# Hi\n"), "code-review").unwrap();
        assert_eq!(parsed.meta.name, "code-review");
        assert_eq!(parsed.meta.description, "Reviews PRs when asked.");
        assert_eq!(parsed.body, "# Hi\n");
    }

    /// Written on Windows, or saved by an editor that adds a BOM.
    #[test]
    fn crlf_and_a_byte_order_mark_are_read_too() {
        let raw = "\u{feff}---\r\nname: code-review\r\ndescription: Reviews PRs.\r\n---\r\n# Hi\r\n";
        let parsed = parse_skill_md(raw, "code-review").unwrap();
        assert_eq!(parsed.meta.description, "Reviews PRs.");
        assert_eq!(parsed.body, "# Hi\r\n");
    }

    /// Why this is YAML and not a line splitter: skills written for other
    /// agents fold long descriptions, and a folded one read as a literal `>`
    /// would be a skill described by a single character.
    #[test]
    fn a_folded_description_and_the_specs_other_fields_are_accepted() {
        let raw = "---\nname: pdf-tools\ndescription: >\n  Extract text from PDFs\n  when handling them.\nlicense: Apache-2.0\nallowed-tools: Bash\nmetadata:\n  author: example\n---\n# Body\n";
        let parsed = parse_skill_md(raw, "pdf-tools").unwrap();
        assert_eq!(parsed.meta.description, "Extract text from PDFs when handling them.");
        assert_eq!(parsed.body, "# Body\n");
    }

    #[test]
    fn a_file_without_frontmatter_is_rejected() {
        assert_eq!(parse_skill_md("# just markdown\n", "x").unwrap_err(), SkillError::MissingFrontmatter);
        assert_eq!(parse_skill_md("---\nname: x\n", "x").unwrap_err(), SkillError::MissingFrontmatter);
    }

    #[test]
    fn the_name_must_match_its_folder() {
        let err = parse_skill_md(&md("code-review", "Reviews PRs.", ""), "other").unwrap_err();
        assert_eq!(err, SkillError::NameMismatch("code-review".into(), "other".into()));
    }

    #[test]
    fn an_empty_or_overlong_description_is_rejected() {
        let empty = "---\nname: code-review\ndescription: \"  \"\n---\nbody\n";
        assert_eq!(parse_skill_md(empty, "code-review").unwrap_err(), SkillError::InvalidDescription);
        let long = md("code-review", &"a".repeat(DESCRIPTION_MAX_CHARS + 1), "");
        assert_eq!(parse_skill_md(&long, "code-review").unwrap_err(), SkillError::InvalidDescription);
    }

    #[test]
    fn missing_fields_are_a_frontmatter_error() {
        let raw = "---\nname: code-review\n---\nbody\n";
        assert!(matches!(parse_skill_md(raw, "code-review"), Err(SkillError::InvalidFrontmatter(_))));
    }

    /// The name is joined onto the skills folder, so this is also what keeps
    /// `../x` from being a name.
    #[test]
    fn names_follow_the_spec() {
        for bad in ["", "PDF", "-pdf", "pdf-", "pdf--x", "../x", "a/b", "a.b", &"a".repeat(NAME_MAX_CHARS + 1)] {
            assert!(validate_skill_name(bad).is_err(), "{bad:?} was accepted");
        }
        for good in ["code-review", "a", "x2", &"a".repeat(NAME_MAX_CHARS)] {
            assert!(validate_skill_name(good).is_ok(), "{good:?} was rejected");
        }
    }
}

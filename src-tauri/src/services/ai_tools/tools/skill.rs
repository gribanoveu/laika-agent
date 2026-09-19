//! `skill` — the instructions of one of the user's skills, fetched when the
//! task calls for it rather than sent on every request.
//!
//! The model already has the catalog (names and descriptions, in the system
//! prompt), so there is nothing to search: it names a skill, gets its
//! `SKILL.md` body and the list of files beside it, and asks for one of those
//! by `path` only if the body points to it. Ported from Alfa Atlas
//! `tools/skill.rs`, whose `search`/`load`/`read` ops became "name" and
//! "name + path" — see `domain::skills` for why search went.

use crate::domain::llm::LlmToolDefinition;
use crate::domain::skills::SkillError;
use crate::domain::tools::{SkillArgs, ToolDeps, ToolError, ToolResult};
use crate::infra::skills_store;

/// Only a skill the turn's prompt listed. The folder may hold more — one the
/// user switched off, one added mid-turn — and a name the model guessed or
/// remembered from an earlier chat must not reach past the switch.
pub fn skill(args: &SkillArgs, deps: &ToolDeps) -> Result<ToolResult, ToolError> {
    if !deps.skills.iter().any(|s| s.name == args.name) {
        return Err(SkillError::NotFound(args.name.clone()).into());
    }
    match &args.path {
        None => {
            let (parsed, files) = skills_store::load(&args.name)?;
            Ok(ToolResult::Skill { name: parsed.meta.name, instructions: parsed.body, files })
        }
        Some(path) => Ok(ToolResult::SkillFile {
            name: args.name.clone(),
            path: path.clone(),
            content: skills_store::read(&args.name, path)?,
        }),
    }
}

impl From<SkillError> for ToolError {
    fn from(error: SkillError) -> Self {
        match error {
            SkillError::PathEscape(path) => ToolError::PathEscape(path),
            other => ToolError::Skill(other.to_string()),
        }
    }
}

pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "skill".to_string(),
        description: "Load the instructions of one of the user's skills, listed under \"Skills\" in the system prompt. \
When the task matches a skill's description, load it before starting the work, and follow what it says. \
Without path, returns the skill's instructions and the files it keeps beside them; with path, returns one of those files — ask for one only when the instructions point to it."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The skill's name, exactly as listed."
                },
                "path": {
                    "type": ["string", "null"],
                    "description": "A file from the skill's file list, relative to the skill folder."
                }
            },
            "required": ["name"]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::skills_store::test_support::write_skill;
    use crate::domain::skills::SkillMeta;
    use crate::testing::with_app_dir;

    /// Deps whose catalog lists `names`, as the turn's prompt would.
    fn listing(names: &[&str]) -> ToolDeps<'static> {
        ToolDeps {
            skills: names.iter().map(|n| SkillMeta { name: n.to_string(), description: "d".into() }).collect(),
            ..ToolDeps::default()
        }
    }

    fn args(name: &str, path: Option<&str>) -> SkillArgs {
        SkillArgs { name: name.to_string(), path: path.map(str::to_string) }
    }

    #[test]
    fn a_skill_comes_back_as_its_instructions_and_files() {
        with_app_dir("skill-tool-load", || {
            let root = write_skill("release", "Cuts a release.", "Bump the version.\n");
            std::fs::write(root.join("checklist.md"), "1. tag").unwrap();

            assert_eq!(
                skill(&args("release", None), &listing(&["release"])).unwrap(),
                ToolResult::Skill {
                    name: "release".into(),
                    instructions: "Bump the version.\n".into(),
                    files: vec!["checklist.md".into()],
                }
            );
            assert_eq!(
                skill(&args("release", Some("checklist.md")), &listing(&["release"])).unwrap(),
                ToolResult::SkillFile { name: "release".into(), path: "checklist.md".into(), content: "1. tag".into() }
            );
        });
    }

    /// Worded for the model: a guessed name should send it back to the list.
    #[test]
    fn an_unknown_skill_points_back_to_the_list() {
        with_app_dir("skill-tool-unknown", || {
            let err = skill(&args("nope", None), &listing(&["nope"])).unwrap_err().to_string();
            assert!(err.contains("Skills list"), "{err}");
        });
    }

    #[test]
    fn a_path_out_of_the_skill_is_a_path_escape() {
        with_app_dir("skill-tool-escape", || {
            write_skill("release", "Cuts a release.", "");
            assert!(matches!(
                skill(&args("release", Some("../x")), &listing(&["release"])),
                Err(ToolError::PathEscape(_))
            ));
        });
    }

    /// Switched off in the tab, so absent from the prompt — but still on
    /// disk, and a name is easy to guess.
    #[test]
    fn a_skill_the_prompt_did_not_list_is_not_loaded() {
        with_app_dir("skill-tool-unlisted", || {
            write_skill("release", "Cuts a release.", "Bump the version.\n");
            std::fs::write(crate::infra::skills_store::dir().unwrap().join("release/notes.md"), "x").unwrap();
            for path in [None, Some("notes.md")] {
                let err = skill(&args("release", path), &listing(&["review"])).unwrap_err().to_string();
                assert!(err.contains("Skills list"), "{path:?}: {err}");
            }
        });
    }
}

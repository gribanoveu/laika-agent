//! `skill` — the instructions of one of the skills — the open folder's or the
//! user's own — fetched when the task calls for it rather than sent on every
//! request.
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
/// Loaded from the folder the catalog found it in: a repository's skill from
/// the repository, the user's from theirs.
pub fn skill(args: &SkillArgs, deps: &ToolDeps) -> Result<ToolResult, ToolError> {
    let Some(listed) = deps.skills.iter().find(|s| s.meta.name == args.name) else {
        return Err(SkillError::NotFound(args.name.clone()).into());
    };
    match &args.path {
        None => {
            let (parsed, files) = skills_store::load(&listed.dir)?;
            Ok(ToolResult::Skill { name: parsed.meta.name, instructions: parsed.body, files, from: provenance(&listed.dir) })
        }
        Some(path) => Ok(ToolResult::SkillFile {
            name: args.name.clone(),
            path: path.clone(),
            content: skills_store::read(&listed.dir, path)?,
        }),
    }
}

/// A skill in one of the user's folders is theirs, shared by every project;
/// anything else the catalog found in the repository. Said, because a
/// user's skill may be about another stack than this one.
fn provenance(dir: &std::path::Path) -> String {
    const USER: [&str; 3] = ["Kibo's skills folder", "~/.agents/skills", "~/.claude/skills"];
    let user = skills_store::user_dirs().ok().and_then(|dirs| dirs.iter().position(|d| dir.starts_with(d)));
    match user {
        Some(i) => format!("{} — the user's own, shared by every project, not this repository's", USER[i]),
        None => {
            let tail: Vec<String> = dir.components().rev().take(3).map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            let shown: Vec<&str> = tail.iter().rev().map(String::as_str).collect();
            format!("{} — this repository's", shown.join("/"))
        }
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
        description: "Load the instructions of one of the skills listed under \"Skills\" in the system prompt. \
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
    use crate::infra::skills_store::test_support::{write_skill, write_skill_in};
    use crate::domain::skills::{Skill, SkillMeta};
    use crate::testing::{temp_dir, with_app_dir};

    /// Deps whose catalog lists `names` from the user's folder, as the turn's prompt would.
    fn listing(names: &[&str]) -> ToolDeps<'static> {
        let dir = skills_store::dir().unwrap();
        ToolDeps {
            skills: names
                .iter()
                .map(|n| Skill { meta: SkillMeta { name: n.to_string(), description: "d".into() }, dir: dir.join(n) })
                .collect(),
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
                    from: "Kibo's skills folder — the user's own, shared by every project, not this repository's".into(),
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

    /// The catalog says where the skill is: a repository's `release` is read
    /// from the repository even when the user has one of the same name.
    #[test]
    fn a_skill_is_loaded_from_the_folder_the_catalog_found_it_in() {
        with_app_dir("skill-tool-project", || {
            write_skill("release", "Mine.", "My steps.\n");
            let ws = temp_dir("skill-tool-project-ws");
            let theirs = write_skill_in(&skills_store::project_dirs(&ws)[0], "release", "Theirs.", "Their steps.\n");
            std::fs::write(theirs.join("notes.md"), "their notes").unwrap();
            let deps = ToolDeps {
                skills: vec![Skill { meta: SkillMeta { name: "release".into(), description: "Theirs.".into() }, dir: theirs }],
                ..ToolDeps::default()
            };

            assert!(matches!(
                skill(&args("release", None), &deps).unwrap(),
                ToolResult::Skill { instructions, from, .. }
                    if instructions == "Their steps.\n" && from == ".claude/skills/release — this repository's"
            ));
            assert!(matches!(
                skill(&args("release", Some("notes.md")), &deps).unwrap(),
                ToolResult::SkillFile { content, .. } if content == "their notes"
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
            // The one that is listed is on disk too: asking for another must
            // not come back with it.
            let review = write_skill("review", "Reviews a diff.", "Read the diff.\n");
            std::fs::write(review.join("notes.md"), "review notes").unwrap();
            for path in [None, Some("notes.md")] {
                let err = skill(&args("release", path), &listing(&["review"])).unwrap_err().to_string();
                assert!(err.contains("Skills list"), "{path:?}: {err}");
            }
        });
    }
}

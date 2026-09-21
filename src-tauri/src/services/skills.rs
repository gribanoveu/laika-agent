//! The skills as the rest of the app sees them: the open folder's and the
//! user's own, minus what the user switched off.
//!
//! Ported from Alfa Atlas `services/agent_skills.rs`, the half that is not the
//! `skill` tool: the catalog the router searched and the Settings list. The
//! bundled/user merge went with the bundled skills; the merge here is of the
//! repository's skills with the user's.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::domain::settings::OptOut;
use crate::domain::skills::{Skill, SkillError, SkillListItem, SkillSource};
use crate::infra::skills_store::{self, SkillEntry};
use crate::infra::settings_store;

/// One skill folder, from wherever it was found.
struct Found {
    source: SkillSource,
    entry: SkillEntry,
}

/// Every skill folder in the order a name is looked up: the open folder's
/// `.claude/skills` and `.agents/skills` — a repository's skill knows that
/// repository — then the user's, the app's own before other agents'.
fn found(workspace: Option<&Path>) -> Result<Vec<Found>, SkillError> {
    let mut all = Vec::new();
    if let Some(workspace) = workspace {
        for dir in skills_store::project_dirs(workspace) {
            for entry in skills_store::scan(&dir, Some(workspace))? {
                all.push(Found { source: SkillSource::Project, entry });
            }
        }
    }
    for dir in skills_store::user_dirs()? {
        for entry in skills_store::scan(&dir, None)? {
            all.push(Found { source: SkillSource::User, entry });
        }
    }
    Ok(all)
}

/// For each folder, the one the model gets for its name: valid, switched on,
/// and first. `None` for a folder whose name no one wins — broken, or off.
///
/// The switch is by name, so a name switched off is off in every folder and
/// every repository: turning off `writing-tests` once is enough.
fn winners(all: &[Found], settings: &OptOut) -> Vec<Option<usize>> {
    let mut first = std::collections::HashMap::new();
    for (i, f) in all.iter().enumerate() {
        if let Ok(parsed) = &f.entry.parsed {
            if settings.is_enabled(&parsed.meta.name) {
                first.entry(parsed.meta.name.clone()).or_insert(i);
            }
        }
    }
    all.iter()
        .map(|f| f.entry.parsed.as_ref().ok().and_then(|p| first.get(&p.meta.name).copied()))
        .collect()
}

/// What the model is offered: the winner of each name, in lookup order.
///
/// Unreadable settings count as "nothing switched off" rather than failing
/// the turn — the settings store already refuses to overwrite a file it
/// could not parse, so nothing is lost by reading past it here.
pub fn enabled_catalog(workspace: Option<&Path>) -> Result<Vec<Skill>, SkillError> {
    let settings = settings_store::load().unwrap_or_default().skills;
    let all = found(workspace)?;
    let wins = winners(&all, &settings);
    Ok(all
        .into_iter()
        .enumerate()
        .filter(|(i, _)| wins[*i] == Some(*i))
        .filter_map(|(_, f)| f.entry.parsed.ok().map(|parsed| Skill { meta: parsed.meta, dir: f.entry.root }))
        .collect())
}

/// The skills tab: every folder, the broken ones with their reason, and the
/// app's own directory, so an empty list can say where skills go.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsView {
    pub dir: PathBuf,
    pub skills: Vec<SkillListItem>,
}

pub fn list(workspace: Option<&Path>) -> Result<SkillsView, SkillError> {
    let settings = settings_store::load().unwrap_or_default().skills;
    let all = found(workspace)?;
    let wins = winners(&all, &settings);
    let paths: Vec<String> = all.iter().map(|f| f.entry.root.display().to_string()).collect();
    let skills = all
        .into_iter()
        .enumerate()
        .map(|(i, f)| {
            let path = paths[i].clone();
            match f.entry.parsed {
                Ok(parsed) => SkillListItem {
                    enabled: settings.is_enabled(&parsed.meta.name),
                    shadowed_by: wins[i].filter(|w| *w != i).map(|w| paths[w].clone()),
                    name: parsed.meta.name,
                    description: parsed.meta.description,
                    error: None,
                    source: f.source,
                    path,
                },
                Err(error) => SkillListItem {
                    name: f.entry.dir_name,
                    description: String::new(),
                    enabled: false,
                    error: Some(error.to_string()),
                    source: f.source,
                    path,
                    shadowed_by: None,
                },
            }
        })
        .collect();
    Ok(SkillsView { dir: skills_store::dir()?, skills })
}

/// By name, for every folder and repository at once. Unlike the catalog, this
/// one does fail on settings it cannot read: saving over them would replace
/// the user's whole configuration with the defaults.
pub fn set_enabled(name: &str, enabled: bool) -> Result<(), SkillError> {
    let io = |e: crate::domain::settings::SettingsError| SkillError::Io(e.to_string());
    let mut settings = settings_store::load().map_err(io)?;
    settings.skills.set_enabled(name, enabled);
    settings_store::save(&settings).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::skills_store::test_support::{write_skill, write_skill_in};
    use crate::testing::{temp_dir, with_app_dir};

    fn names(skills: &[Skill]) -> Vec<&str> {
        skills.iter().map(|s| s.meta.name.as_str()).collect()
    }

    /// A repository with `release` in `.claude/skills` and `lint` in `.agents/skills`.
    fn repository(label: &str) -> PathBuf {
        let ws = temp_dir(label);
        let [claude, agents] = skills_store::project_dirs(&ws);
        write_skill_in(&claude, "release", "The repository's release.", "");
        write_skill_in(&agents, "lint", "Lints.", "");
        ws
    }

    #[test]
    fn a_switched_off_skill_leaves_the_catalog_and_stays_in_the_list() {
        with_app_dir("skills-svc-toggle", || {
            write_skill("release", "Cuts a release.", "");
            write_skill("review", "Reviews a diff.", "");

            set_enabled("release", false).unwrap();
            assert_eq!(names(&enabled_catalog(None).unwrap()), ["review"]);
            let listed = list(None).unwrap().skills;
            assert_eq!(listed.len(), 2);
            assert!(!listed.iter().find(|s| s.name == "release").unwrap().enabled);
            assert!(listed.iter().find(|s| s.name == "review").unwrap().enabled);

            set_enabled("release", true).unwrap();
            assert_eq!(names(&enabled_catalog(None).unwrap()), ["release", "review"]);
        });
    }

    #[test]
    fn a_repositorys_skills_come_first_and_load_from_the_repository() {
        with_app_dir("skills-svc-project", || {
            let ws = repository("skills-svc-project-ws");
            write_skill("review", "Reviews a diff.", "");

            let catalog = enabled_catalog(Some(&ws)).unwrap();
            assert_eq!(names(&catalog), ["release", "lint", "review"]);
            assert_eq!(catalog[0].dir, skills_store::project_dirs(&ws)[0].join("release"));
            assert_eq!(catalog[1].dir, skills_store::project_dirs(&ws)[1].join("lint"));
            assert_eq!(catalog[2].dir, skills_store::dir().unwrap().join("review"));

            let listed = list(Some(&ws)).unwrap().skills;
            let sources: Vec<SkillSource> = listed.iter().map(|s| s.source).collect();
            assert_eq!(sources, [SkillSource::Project, SkillSource::Project, SkillSource::User]);
            assert_eq!(listed[0].path, skills_store::project_dirs(&ws)[0].join("release").display().to_string());
            // No folder open: only the user's.
            assert_eq!(names(&enabled_catalog(None).unwrap()), ["review"]);
        });
    }

    /// The model gets one skill per name: the repository's. The user's own
    /// says which one is used instead of it.
    #[test]
    fn the_same_name_is_the_repositorys() {
        with_app_dir("skills-svc-shadow", || {
            let ws = repository("skills-svc-shadow-ws");
            write_skill("release", "My release.", "");

            let catalog = enabled_catalog(Some(&ws)).unwrap();
            assert_eq!(names(&catalog), ["release", "lint"]);
            assert_eq!(catalog[0].meta.description, "The repository's release.");
            let listed = list(Some(&ws)).unwrap().skills;
            let theirs = skills_store::project_dirs(&ws)[0].join("release").display().to_string();
            let user = listed.iter().find(|s| s.source == SkillSource::User).unwrap();
            assert!(user.enabled);
            assert_eq!(user.shadowed_by.as_deref(), Some(theirs.as_str()));
            assert_eq!(listed[0].shadowed_by, None);
        });
    }

    /// Off by name: every folder's `release` and every repository's, and it
    /// stays off whichever folder is opened next.
    #[test]
    fn a_skill_switched_off_is_off_in_every_folder_and_repository() {
        with_app_dir("skills-svc-by-name", || {
            let one = repository("skills-svc-by-name-one");
            let two = repository("skills-svc-by-name-two");
            write_skill("release", "My release.", "");

            set_enabled("release", false).unwrap();
            assert_eq!(names(&enabled_catalog(Some(&one)).unwrap()), ["lint"]);
            assert_eq!(names(&enabled_catalog(Some(&two)).unwrap()), ["lint"]);
            assert!(enabled_catalog(None).unwrap().is_empty());
            let listed = list(Some(&two)).unwrap().skills;
            let releases: Vec<_> = listed.iter().filter(|s| s.name == "release").collect();
            assert_eq!(releases.len(), 2);
            assert!(releases.iter().all(|s| !s.enabled && s.shadowed_by.is_none()));
        });
    }

    /// Codex's and Claude Code's folders are read after the app's own, and an
    /// installer that put the same skill into both lists it once for the model.
    #[test]
    fn other_agents_skills_follow_the_apps_own() {
        with_app_dir("skills-svc-agents", || {
            let [mine, agents, claude] = skills_store::user_dirs().unwrap();
            write_skill_in(&claude, "streamdown", "Claude Code's copy.", "");
            write_skill_in(&agents, "streamdown", "Codex's copy.", "");
            write_skill_in(&agents, "find-skills", "Finds skills.", "");
            write_skill_in(&mine, "release", "Mine.", "");

            let catalog = enabled_catalog(None).unwrap();
            assert_eq!(names(&catalog), ["release", "find-skills", "streamdown"]);
            assert_eq!(catalog[2].dir, agents.join("streamdown"));
            let listed = list(None).unwrap().skills;
            assert_eq!(listed.len(), 4);
            let copy = listed.iter().find(|s| s.path == claude.join("streamdown").display().to_string()).unwrap();
            assert_eq!(copy.shadowed_by, Some(agents.join("streamdown").display().to_string()));
        });
    }

    #[test]
    fn a_broken_skill_is_listed_by_its_folder_with_the_reason() {
        with_app_dir("skills-svc-broken", || {
            let broken = skills_store::dir().unwrap().join("broken");
            std::fs::create_dir_all(&broken).unwrap();
            std::fs::write(broken.join("SKILL.md"), "no frontmatter").unwrap();

            let view = list(None).unwrap();
            assert_eq!(view.dir, skills_store::dir().unwrap());
            let item = &view.skills[0];
            assert_eq!(item.name, "broken");
            assert!(!item.enabled && item.shadowed_by.is_none());
            assert_eq!(item.error.as_deref(), Some("SKILL.md is missing YAML frontmatter"));
            assert_eq!(item.path, broken.display().to_string());
        });
    }

    /// A broken one never takes a name from a valid one after it.
    #[test]
    fn a_broken_skill_does_not_hide_one_of_the_same_name() {
        with_app_dir("skills-svc-broken-shadow", || {
            let ws = temp_dir("skills-svc-broken-shadow-ws");
            let broken = skills_store::project_dirs(&ws)[0].join("release");
            std::fs::create_dir_all(&broken).unwrap();
            std::fs::write(broken.join("SKILL.md"), "no frontmatter").unwrap();
            write_skill("release", "Mine.", "");

            assert_eq!(names(&enabled_catalog(Some(&ws)).unwrap()), ["release"]);
            assert!(list(Some(&ws)).unwrap().skills.iter().all(|s| s.shadowed_by.is_none()));
        });
    }

    /// The switch must not cost the user their provider configuration.
    #[test]
    fn switching_a_skill_keeps_the_rest_of_the_settings() {
        with_app_dir("skills-svc-keep", || {
            let mut settings = settings_store::load().unwrap();
            settings.llm.debug_logging = true;
            settings_store::save(&settings).unwrap();

            set_enabled("release", false).unwrap();
            let saved = settings_store::load().unwrap();
            assert!(saved.llm.debug_logging);
            assert_eq!(saved.skills.disabled, ["release"]);
        });
    }

    #[test]
    fn unreadable_settings_refuse_the_switch_rather_than_overwrite_them() {
        with_app_dir("skills-svc-damaged", || {
            crate::infra::app_dir::ensure().unwrap();
            std::fs::write(settings_store::path().unwrap(), "{ not json").unwrap();

            assert!(set_enabled("release", false).is_err());
            assert_eq!(std::fs::read_to_string(settings_store::path().unwrap()).unwrap(), "{ not json");
        });
    }
}

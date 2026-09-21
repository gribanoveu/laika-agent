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
    key: String,
}

/// Every skill folder in the order a name is looked up: the open folder's
/// `.claude/skills`, its `.agents/skills`, then the user's own — a
/// repository's skill knows that repository, so it comes first.
fn found(workspace: Option<&Path>) -> Result<Vec<Found>, SkillError> {
    let mut all = Vec::new();
    if let Some(workspace) = workspace {
        for dir in skills_store::project_dirs(workspace) {
            for entry in skills_store::scan(&dir, Some(workspace))? {
                // By folder, so the same name in two repositories is two switches.
                let key = entry.root.canonicalize().unwrap_or_else(|_| entry.root.clone()).display().to_string();
                all.push(Found { source: SkillSource::Project, entry, key });
            }
        }
    }
    for entry in skills_store::scan(&skills_store::dir()?, None)? {
        // By name, as before there were two sources: a user's switches survive.
        let key = entry.dir_name.clone();
        all.push(Found { source: SkillSource::User, entry, key });
    }
    Ok(all)
}

/// For each folder, whether the model gets it: valid, switched on, and the
/// first such of its name. One switched off steps aside — the user's own
/// `release` is what a repository's switched-off `release` leaves.
fn winners(all: &[Found], settings: &OptOut) -> Vec<bool> {
    let mut taken = std::collections::HashSet::new();
    all.iter()
        .map(|f| match &f.entry.parsed {
            Ok(parsed) if settings.is_enabled(&f.key) => taken.insert(parsed.meta.name.clone()),
            _ => false,
        })
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
        .zip(wins)
        .filter_map(|(f, wins)| wins.then_some(f))
        .filter_map(|f| f.entry.parsed.ok().map(|parsed| Skill { meta: parsed.meta, dir: f.entry.root }))
        .collect())
}

/// The skills tab: every folder, the broken ones with their reason, and the
/// user's directory itself, so an empty list can say where skills go.
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
    let skills = all
        .into_iter()
        .zip(wins)
        .map(|(f, wins)| {
            let path = f.entry.root.display().to_string();
            match f.entry.parsed {
                Ok(parsed) => {
                    let enabled = settings.is_enabled(&f.key);
                    SkillListItem {
                        enabled,
                        shadowed: enabled && !wins,
                        name: parsed.meta.name,
                        description: parsed.meta.description,
                        error: None,
                        source: f.source,
                        key: f.key,
                        path,
                    }
                }
                Err(error) => SkillListItem {
                    name: f.entry.dir_name,
                    description: String::new(),
                    enabled: false,
                    error: Some(error.to_string()),
                    source: f.source,
                    key: f.key,
                    path,
                    shadowed: false,
                },
            }
        })
        .collect();
    Ok(SkillsView { dir: skills_store::dir()?, skills })
}

/// By the row's `key`. Unlike the catalog, this one does fail on settings it
/// cannot read: saving over them would replace the user's whole
/// configuration with the defaults.
pub fn set_enabled(key: &str, enabled: bool) -> Result<(), SkillError> {
    let io = |e: crate::domain::settings::SettingsError| SkillError::Io(e.to_string());
    let mut settings = settings_store::load().map_err(io)?;
    settings.skills.set_enabled(key, enabled);
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
            assert_eq!(listed[2].key, "review");
            assert!(listed[0].key.ends_with("release") && listed[0].key.contains(".claude"), "{}", listed[0].key);
            // No folder open: only the user's.
            assert_eq!(names(&enabled_catalog(None).unwrap()), ["review"]);
        });
    }

    /// The model gets one skill per name: the repository's, and the user's
    /// own again once the repository's is switched off.
    #[test]
    fn the_same_name_is_the_repositorys_until_it_is_switched_off() {
        with_app_dir("skills-svc-shadow", || {
            let ws = repository("skills-svc-shadow-ws");
            let mine = write_skill("release", "My release.", "");

            let catalog = enabled_catalog(Some(&ws)).unwrap();
            assert_eq!(names(&catalog), ["release", "lint"]);
            assert_eq!(catalog[0].meta.description, "The repository's release.");
            let listed = list(Some(&ws)).unwrap().skills;
            let user = listed.iter().find(|s| s.source == SkillSource::User).unwrap();
            assert!(user.enabled && user.shadowed);
            assert!(!listed[0].shadowed);

            set_enabled(&listed[0].key, false).unwrap();
            let catalog = enabled_catalog(Some(&ws)).unwrap();
            assert_eq!(names(&catalog), ["lint", "release"]);
            assert_eq!(catalog[1].dir, mine);
            assert!(!list(Some(&ws)).unwrap().skills.iter().any(|s| s.shadowed));
        });
    }

    /// Switched off by folder: the same name in another repository stays on,
    /// and so does the user's own.
    #[test]
    fn switching_off_a_repositorys_skill_is_for_that_repository_only() {
        with_app_dir("skills-svc-per-repo", || {
            let one = repository("skills-svc-repo-one");
            let two = repository("skills-svc-repo-two");
            let key = list(Some(&one)).unwrap().skills[0].key.clone();
            set_enabled(&key, false).unwrap();

            assert_eq!(names(&enabled_catalog(Some(&one)).unwrap()), ["lint"]);
            assert_eq!(names(&enabled_catalog(Some(&two)).unwrap()), ["release", "lint"]);
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
            assert!(!item.enabled && !item.shadowed);
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
            assert!(!list(Some(&ws)).unwrap().skills.iter().any(|s| s.shadowed));
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

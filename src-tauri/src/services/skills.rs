//! The user's skills as the rest of the app sees them: the folder, minus
//! what the user switched off.
//!
//! Ported from Alfa Atlas `services/agent_skills.rs`, the half that is not the
//! `skill` tool: the catalog the router searched and the Settings list. There
//! is one source here, so the bundled/user merge went with the bundled skills.

use std::path::PathBuf;

use serde::Serialize;

use crate::domain::skills::{SkillError, SkillListItem, SkillMeta};
use crate::infra::{settings_store, skills_store};

/// What the model is offered: every skill that parses and is switched on.
///
/// Unreadable settings count as "nothing switched off" rather than failing
/// the turn — the settings store already refuses to overwrite a file it
/// could not parse, so nothing is lost by reading past it here.
pub fn enabled_catalog() -> Result<Vec<SkillMeta>, SkillError> {
    let settings = settings_store::load().unwrap_or_default().skills;
    Ok(skills_store::catalog()?.into_iter().filter(|s| settings.is_enabled(&s.name)).collect())
}

/// The skills tab: every folder, the broken ones with their reason, and the
/// directory itself, so an empty list can say where skills go.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsView {
    pub dir: PathBuf,
    pub skills: Vec<SkillListItem>,
}

pub fn list() -> Result<SkillsView, SkillError> {
    let settings = settings_store::load().unwrap_or_default().skills;
    let skills = skills_store::scan()?
        .into_iter()
        .map(|entry| match entry.parsed {
            Ok(parsed) => SkillListItem {
                enabled: settings.is_enabled(&parsed.meta.name),
                name: parsed.meta.name,
                description: parsed.meta.description,
                error: None,
            },
            Err(error) => SkillListItem {
                name: entry.dir_name,
                description: String::new(),
                enabled: false,
                error: Some(error.to_string()),
            },
        })
        .collect();
    Ok(SkillsView { dir: skills_store::dir()?, skills })
}

/// Unlike the catalog, this one does fail on settings it cannot read: saving
/// over them would replace the user's whole configuration with the defaults.
pub fn set_enabled(name: &str, enabled: bool) -> Result<(), SkillError> {
    let io = |e: crate::domain::settings::SettingsError| SkillError::Io(e.to_string());
    let mut settings = settings_store::load().map_err(io)?;
    settings.skills.set_enabled(name, enabled);
    settings_store::save(&settings).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::skills_store::test_support::write_skill;
    use crate::testing::with_app_dir;

    fn names(skills: &[SkillMeta]) -> Vec<&str> {
        skills.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn a_switched_off_skill_leaves_the_catalog_and_stays_in_the_list() {
        with_app_dir("skills-svc-toggle", || {
            write_skill("release", "Cuts a release.", "");
            write_skill("review", "Reviews a diff.", "");

            set_enabled("release", false).unwrap();
            assert_eq!(names(&enabled_catalog().unwrap()), ["review"]);
            let listed = list().unwrap().skills;
            assert_eq!(listed.len(), 2);
            assert!(!listed.iter().find(|s| s.name == "release").unwrap().enabled);
            assert!(listed.iter().find(|s| s.name == "review").unwrap().enabled);

            set_enabled("release", true).unwrap();
            assert_eq!(names(&enabled_catalog().unwrap()), ["release", "review"]);
        });
    }

    #[test]
    fn a_broken_skill_is_listed_by_its_folder_with_the_reason() {
        with_app_dir("skills-svc-broken", || {
            let broken = skills_store::dir().unwrap().join("broken");
            std::fs::create_dir_all(&broken).unwrap();
            std::fs::write(broken.join("SKILL.md"), "no frontmatter").unwrap();

            let view = list().unwrap();
            assert_eq!(view.dir, skills_store::dir().unwrap());
            let item = &view.skills[0];
            assert_eq!(item.name, "broken");
            assert!(!item.enabled);
            assert_eq!(item.error.as_deref(), Some("SKILL.md is missing YAML frontmatter"));
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

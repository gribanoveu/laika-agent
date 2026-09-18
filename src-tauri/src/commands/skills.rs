//! The skills tab: what is in the skills folder, and which are switched on.

use crate::services::skills::{self, SkillsView};

#[tauri::command]
pub fn skills_list() -> Result<SkillsView, String> {
    skills::list().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn skills_set_enabled(name: String, enabled: bool) -> Result<(), String> {
    skills::set_enabled(&name, enabled).map_err(|e| e.to_string())
}

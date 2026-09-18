//! The skills and rules tabs: what the model is given besides the
//! conversation, and which of it is switched on.

use crate::services::skills::{self, SkillsView};

#[tauri::command]
pub fn skills_list() -> Result<SkillsView, String> {
    skills::list().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn skills_set_enabled(name: String, enabled: bool) -> Result<(), String> {
    skills::set_enabled(&name, enabled).map_err(|e| e.to_string())
}

/// The open folder's instruction files; none while no folder is open.
#[tauri::command]
pub fn rules_list(state: tauri::State<'_, std::sync::Arc<super::chat::AgentState>>) -> Vec<crate::domain::project_rules::RuleListItem> {
    state.workspace().map(|root| crate::services::project_rules::list(&root)).unwrap_or_default()
}

#[tauri::command]
pub fn rules_set_enabled(path: String, enabled: bool) -> Result<(), String> {
    crate::services::project_rules::set_enabled(&path, enabled).map_err(|e| e.to_string())
}

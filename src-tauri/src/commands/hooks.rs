//! The Hooks tab and its configuration editor.

use serde::Serialize;

use crate::domain::hooks::{self, HookItem, HooksConfig};
use crate::infra::hooks as hooks_file;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksView {
    pub path: String,
    pub text: String,
    pub hooks: Vec<HookItem>,
}

fn view(config: &HooksConfig) -> Result<HooksView, String> {
    Ok(HooksView {
        path: hooks_file::path().map_err(|e| e.to_string())?.display().to_string(),
        text: hooks_file::read_text().map_err(|e| e.to_string())?,
        hooks: hooks::items(config),
    })
}

#[tauri::command]
pub fn hooks_config_get() -> Result<HooksView, String> {
    view(&hooks_file::load().map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn hooks_config_save(text: String) -> Result<HooksView, String> {
    view(&hooks_file::save_text(&text).map_err(|e| e.to_string())?)
}

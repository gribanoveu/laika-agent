//! The MCP tab and its configuration editor.

use serde::Serialize;

use crate::domain::mcp::{self, McpConfig, McpServerItem};
use crate::infra::mcp_config;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpView {
    /// Where the file is, so the editor can say.
    pub path: String,
    /// The file as it stands, for the editor.
    pub text: String,
    pub servers: Vec<McpServerItem>,
}

fn view(config: Option<McpConfig>) -> Result<McpView, String> {
    let config = match config {
        Some(config) => config,
        None => mcp_config::load().map_err(|e| e.to_string())?,
    };
    Ok(McpView {
        path: mcp_config::path().map_err(|e| e.to_string())?.display().to_string(),
        text: mcp_config::read_text().map_err(|e| e.to_string())?,
        servers: mcp::items(&config),
    })
}

#[tauri::command]
pub fn mcp_config_get() -> Result<McpView, String> {
    view(None)
}

#[tauri::command]
pub fn mcp_config_save(text: String) -> Result<McpView, String> {
    view(Some(mcp_config::save_text(&text).map_err(|e| e.to_string())?))
}

#[tauri::command]
pub fn mcp_server_set_enabled(name: String, enabled: bool) -> Result<McpView, String> {
    view(Some(mcp_config::set_enabled(&name, enabled).map_err(|e| e.to_string())?))
}

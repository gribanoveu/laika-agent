//! The Files tab's folder tree: one folder at a time, as it is unfolded.

use std::sync::Arc;

use tauri::State;

use super::chat::AgentState;
use crate::domain::file_tree::FolderListing;
use crate::infra::file_tree;

/// `dir` is relative to the open folder; empty for the folder itself. Off the
/// IPC loop: a folder's walk and git's status of it take a moment on a big one.
#[tauri::command]
pub async fn workspace_list(dir: String, state: State<'_, Arc<AgentState>>) -> Result<FolderListing, String> {
    let root = state.workspace()?;
    tauri::async_runtime::spawn_blocking(move || file_tree::list(&root, &dir))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

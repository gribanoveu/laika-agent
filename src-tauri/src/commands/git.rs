//! The Changes tab: the open folder's staged and unstaged files, staging, and
//! committing. Off the IPC loop — a status of a large repository takes a while.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use super::chat::AgentState;
use crate::domain::git_changes::{GitChangesError, WorkingChanges};
use crate::infra::git_changes;

async fn in_repo<T: Send + 'static>(
    state: &AgentState,
    op: impl FnOnce(PathBuf) -> Result<T, GitChangesError> + Send + 'static,
) -> Result<T, String> {
    let root = state.workspace()?;
    tauri::async_runtime::spawn_blocking(move || op(root))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn git_changes(state: State<'_, Arc<AgentState>>) -> Result<WorkingChanges, String> {
    in_repo(&state, |root| git_changes::changes(&root)).await
}

#[tauri::command]
pub async fn git_stage(paths: Vec<String>, state: State<'_, Arc<AgentState>>) -> Result<(), String> {
    in_repo(&state, move |root| git_changes::stage(&root, &paths)).await
}

#[tauri::command]
pub async fn git_unstage(paths: Vec<String>, state: State<'_, Arc<AgentState>>) -> Result<(), String> {
    in_repo(&state, move |root| git_changes::unstage(&root, &paths)).await
}

/// The new commit's short id.
#[tauri::command]
pub async fn git_commit(message: String, state: State<'_, Arc<AgentState>>) -> Result<String, String> {
    in_repo(&state, move |root| git_changes::commit(&root, &message)).await
}

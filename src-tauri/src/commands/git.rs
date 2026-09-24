//! The Changes tab: the open folder's staged and unstaged files, staging, and
//! committing. Off the IPC loop — a status of a large repository takes a while.
//!
//! The tab is told when to read again rather than asking on a timer. An edit
//! to the tree already arrives as the index's `syncStarted`; what that watcher
//! leaves out is `.git`, so a `git add` or a commit made in a terminal is
//! reported here, on [`GIT_EVENT`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, State};

use super::chat::AgentState;
use crate::domain::git_changes::{ChangeTotals, GitChangesError, GitHistory, WorkingChanges};
use crate::infra::file_watcher::FileWatcher;
use crate::infra::git_changes;

/// The open folder's repository changed its index, HEAD or refs.
pub const GIT_EVENT: &str = "workspace-git:changed";

/// The folder it is about, the string `workspace_open` returned: an event from
/// the folder just left must not refresh the one opened after it.
#[derive(Clone, Serialize)]
struct GitChanged {
    root: String,
}

/// The watch on the open folder's `.git`. Replaced when another folder opens.
#[derive(Default)]
pub struct GitWatch(Mutex<Option<FileWatcher>>);

/// Watches the repository `root` is in, in place of the previous folder's.
/// Outside a repository, or where it cannot be watched, nothing is reported —
/// the tab still reads again after its own actions and on tree edits.
pub fn watch_git<R: Runtime>(app: &AppHandle<R>, watch: &GitWatch, root: &Path, shown: String) {
    let mut current = watch.0.lock().unwrap_or_else(PoisonError::into_inner);
    *current = None;
    let Some(git_dir) = git_changes::git_dir(root) else { return };
    let app = app.clone();
    let started = FileWatcher::start(&git_dir, move || {
        let _ = app.emit(GIT_EVENT, GitChanged { root: shown.clone() });
    });
    match started {
        Ok(watcher) => *current = Some(watcher),
        Err(e) => eprintln!("{} is not watched, so git changes made elsewhere are not shown: {e}", git_dir.display()),
    }
}

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

/// `None` outside a repository: an ordinary folder has nothing to count, and
/// the header shows nothing rather than an error.
#[tauri::command]
pub async fn git_totals(state: State<'_, Arc<AgentState>>) -> Result<Option<ChangeTotals>, String> {
    in_repo(&state, |root| match git_changes::totals(&root) {
        Err(GitChangesError::NotARepository) => Ok(None),
        other => other.map(Some),
    })
    .await
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

/// Where HEAD is and the newest `limit` commits, for the History tab.
#[tauri::command]
pub async fn git_history(limit: usize, state: State<'_, Arc<AgentState>>) -> Result<GitHistory, String> {
    in_repo(&state, move |root| git_changes::history(&root, limit)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;
    use std::sync::mpsc;
    use std::time::Duration;
    use tauri::Listener;

    #[test]
    fn staging_outside_the_tab_is_reported_with_the_folder() {
        let dir = temp_dir("git-watch");
        git2::Repository::init(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();

        let app = tauri::test::mock_app();
        let (tx, rx) = mpsc::channel();
        app.handle().listen(GIT_EVENT, move |event| {
            let _ = tx.send(event.payload().to_string());
        });
        let watch = GitWatch::default();
        watch_git(app.handle(), &watch, &dir, "shown".into());

        git_changes::stage(&dir, &["a.txt".into()]).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), r#"{"root":"shown"}"#);
    }

    #[test]
    fn opening_a_folder_outside_a_repository_stops_the_previous_watch() {
        let app = tauri::test::mock_app();
        let watch = GitWatch::default();
        let repo = temp_dir("git-watch-repo");
        git2::Repository::init(&repo).unwrap();
        watch_git(app.handle(), &watch, &repo, "repo".into());
        assert!(watch.0.lock().unwrap().is_some());

        watch_git(app.handle(), &watch, &temp_dir("git-watch-plain"), "plain".into());
        assert!(watch.0.lock().unwrap().is_none());
    }

    /// `src/lib/chat.ts` listens on this name.
    #[test]
    fn the_channel_name_is_pinned() {
        assert_eq!(GIT_EVENT, "workspace-git:changed");
    }
}

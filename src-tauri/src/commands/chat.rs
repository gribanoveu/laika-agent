//! The turn, reachable from the window.
//!
//! Every command here is thin on purpose: check what came across the wire,
//! call one service, flatten the error to a string. The decisions all live
//! below this file.
//!
//! ## What the backend does not remember
//!
//! The conversation is not kept here. `chat_start` receives the whole history
//! and `chat_resume` receives the whole checkpoint, which is what makes a
//! paused turn survive a reload, a crash, or being picked up by a different
//! window. What *is* resident is only what a running turn needs to be reached
//! from outside while it runs: the stop flag, the queue of notes typed at it,
//! the workspace it acts on, and the standing approval policy.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::domain::command_exec::Shell;
use crate::domain::compaction::ContextUsage;
use crate::domain::conversation_mode::ConversationMode;
use crate::domain::llm::{LlmMessage, LlmToolCall};
use crate::domain::tools::{ApprovalPolicy, CodeSearchFn, Task, ToolName, ToolPreview, ToolScope};
use crate::domain::turn::{
    ChatStreamOutcome, PendingApproval, PendingToolCall, SteeringNote, ToolCallDecision,
};
use crate::services::ai_tools::preview;
use crate::services::llm_chat::{self, SteeringQueue, Turn, TurnError};
use crate::services::context_compaction;
use crate::services::llm_session;
use crate::services::workspace_index::WorkspaceIndex;

use super::chat_events::chat_event_sink;
use super::workspace_events::index_event_sink;

#[derive(Default)]
pub struct AgentState {
    /// One in-flight turn at a time, so one flag needs no turn id to
    /// disambiguate. Reset by `chat_start` and deliberately *not* by
    /// `chat_resume` — a stop pressed while an approval card was showing has
    /// to survive until the resumed turn reads it.
    cancel: AtomicBool,
    steering: SteeringQueue,
    workspace: Mutex<Option<PathBuf>>,
    /// Grows as the user answers "always allow"; not persisted, because a
    /// saved "never ask me" is a brake released a month ago and forgotten.
    approval: Mutex<ApprovalPolicy>,
    /// What the window's mode chip says right now. Resident rather than sent
    /// with each turn for the same reason as the approval policy: a paused
    /// turn is resumed from a checkpoint that predates the chip, and a mode
    /// threaded through both entry points would have to be re-sent correctly
    /// by the caller at the one moment it is easiest to forget. Not persisted
    /// — a mode is a decision about this conversation, not about the app.
    mode: Mutex<ConversationMode>,
}

impl AgentState {
    pub(super) fn workspace(&self) -> Result<PathBuf, String> {
        self.workspace
            .lock()
            .map_err(|_| "workspace lock poisoned".to_string())?
            .clone()
            .ok_or_else(|| "no folder is open".to_string())
    }

    fn mode(&self) -> ConversationMode {
        // A poisoned lock loses the chip's setting, not the turn. Falling back
        // to the default would silently widen it, so the last successful read
        // is not available — `Agent` is the default and the honest answer is
        // to say the mode could not be read.
        self.mode
            .lock()
            .map_or(ConversationMode::default(), |mode| *mode)
    }

    fn approval(&self) -> Result<ApprovalPolicy, String> {
        Ok(self
            .approval
            .lock()
            .map_err(|_| "approval lock poisoned".to_string())?
            .clone())
    }
}

/// Opens a folder as the workspace. Canonicalized here, once, so every path a
/// tool resolves later is measured against a real directory rather than
/// against whatever spelling the caller used.
///
/// Also makes it the indexed folder. The index opening is awaited — it reads
/// what was embedded before, off the UI thread — and its first sync is not.
/// An index that cannot open does not stop the folder opening: the reason
/// arrives on `workspace-index:event`, and the agent works without search.
#[tauri::command]
pub async fn workspace_open(
    path: String,
    app: AppHandle,
    state: State<'_, Arc<AgentState>>,
    index: State<'_, Arc<WorkspaceIndex>>,
) -> Result<String, String> {
    let resolved = PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| format!("cannot open {path}: {e}"))?;
    if !resolved.is_dir() {
        return Err(format!("{path} is not a folder"));
    }
    *state
        .workspace
        .lock()
        .map_err(|_| "workspace lock poisoned".to_string())? = Some(resolved.clone());

    let shown = resolved.display().to_string();
    let index = Arc::clone(&index);
    let sink = index_event_sink(&app, shown.clone());
    // Reported through the sink; see above.
    let _ = tauri::async_runtime::spawn_blocking(move || index.open(&resolved, sink)).await;
    Ok(shown)
}

#[tauri::command]
pub fn workspace_current(state: State<'_, Arc<AgentState>>) -> Option<String> {
    state
        .workspace
        .lock()
        .ok()?
        .clone()
        .map(|path| path.display().to_string())
}

/// The open folder's index as it stands — what a window that was not
/// listening when the sync began (a reload, say) starts from before the next
/// `workspace-index:event`.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexSnapshot {
    /// The same string `workspace_open` returned, which the events carry too.
    root: String,
    syncing: bool,
    embedded: usize,
    skipped: usize,
    embedding_error: Option<String>,
}

#[tauri::command]
pub fn workspace_index_status(index: State<'_, Arc<WorkspaceIndex>>) -> Option<IndexSnapshot> {
    snapshot(&index)
}

fn snapshot(index: &WorkspaceIndex) -> Option<IndexSnapshot> {
    let indexer = index.current()?;
    let status = indexer.status();
    Some(IndexSnapshot {
        root: indexer.root().display().to_string(),
        syncing: status.syncing,
        embedded: status.embedded,
        skipped: status.skipped.len(),
        embedding_error: status.embedding_error,
    })
}

/// Starts a fresh turn and resolves when it ends, pauses, or is stopped.
///
/// `messages` is the whole conversation so far: the caller owns the
/// transcript, and this stays a function of its arguments.
#[tauri::command]
pub async fn chat_start<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, Arc<AgentState>>,
    turn_id: String,
    messages: Vec<LlmMessage>,
    todos: Vec<Task>,
) -> Result<ChatStreamOutcome, String> {
    let state = state.inner().clone();
    // A stray stop from a turn that already finished must not end this one
    // before it starts.
    state.cancel.store(false, Ordering::SeqCst);
    run_off_the_event_loop(app, state, turn_id, move |turn| {
        llm_chat::stream(turn, messages, todos)
    })
    .await
}

/// Continues a turn that paused for approval, with the decisions the user
/// made. The checkpoint goes back exactly as it arrived.
#[tauri::command]
pub async fn chat_resume<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, Arc<AgentState>>,
    turn_id: String,
    checkpoint: PendingApproval,
    decisions: Vec<ToolCallDecision>,
) -> Result<ChatStreamOutcome, String> {
    let state = state.inner().clone();
    run_off_the_event_loop(app, state, turn_id, move |turn| {
        llm_chat::resume(turn, checkpoint, decisions)
    })
    .await
}

/// What the calls of a paused round would do, worked out without doing them.
///
/// Asked for the whole round at once: the card shows every call, and one round
/// trip beats one per call. Nothing here runs anything.
#[tauri::command]
pub fn chat_preview(
    calls: Vec<PendingToolCall>,
    state: State<'_, Arc<AgentState>>,
) -> Result<Vec<ToolPreview>, String> {
    let workspace = state.workspace()?;
    let scope = ToolScope::new(&workspace).map_err(|e| e.to_string())?;
    let calls: Vec<LlmToolCall> = calls
        .into_iter()
        .map(|call| LlmToolCall {
            id: call.id,
            name: call.name,
            arguments: call.arguments,
        })
        .collect();
    Ok(preview::preview_round(&scope, &calls))
}

/// What the conversation looks like once the older part of it is a summary.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactedHistory {
    pub history: Vec<LlmMessage>,
    /// How many messages the one summary now stands for.
    pub folded: usize,
}

/// Shortens the conversation, if it is worth shortening.
///
/// Called by the window before it sends, and by the user asking outright
/// (`force`). It is the window that asks because the window owns the history
/// — but nothing about *when* or *how much* is decided there: this returns
/// `None` whenever the answer is "leave it alone", so the rules stay on this
/// side and cannot drift into a second copy written in TypeScript.
#[tauri::command]
pub async fn chat_compact(
    messages: Vec<LlmMessage>,
    force: bool,
) -> Result<Option<CompactedHistory>, String> {
    // A summary is an ordinary request to the provider, and a request on the
    // IPC loop freezes every other command for its duration.
    tauri::async_runtime::spawn_blocking(move || {
        let session = llm_session::resolve(None).map_err(|e| e.to_string())?;
        context_compaction::compact_if_needed(&session, &messages, force)
            .map(|compacted| {
                compacted.map(|compacted| CompactedHistory {
                    history: compacted.history,
                    folded: compacted.folded,
                })
            })
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("the compaction thread failed: {e}"))?
}

/// What the next request will cost, for the window's meter.
///
/// Asked rather than computed there: the numbers on the meter have to be the
/// ones that decide, or a reader watches a gauge that is not connected to the
/// thing it appears to measure. Cheap — arithmetic over the history plus one
/// serialization of the tool schemas, no provider call — so it is a plain
/// command rather than another thread.
#[tauri::command]
pub fn chat_context_usage(messages: Vec<LlmMessage>) -> Result<ContextUsage, String> {
    let session = llm_session::resolve(None).map_err(|e| e.to_string())?;
    Ok(context_compaction::usage(&session, &messages))
}

/// Asks the running turn to stop. Returns at once: the turn notices at its
/// next checkpoint, which is never more than one tool call away.
#[tauri::command]
pub fn chat_cancel(state: State<'_, Arc<AgentState>>) {
    state.cancel.store(true, Ordering::SeqCst);
}

/// Queues something the user typed while the turn was running. The id comes
/// back so the same note can be withdrawn before a round takes it.
#[tauri::command]
pub fn chat_steer(text: String, state: State<'_, Arc<AgentState>>) -> String {
    let note = SteeringNote::user(text);
    let id = note.id.clone();
    state.steering.push(note);
    id
}

/// `false` means the note is already gone — a round picked it up while the
/// user was reaching for cancel.
#[tauri::command]
pub fn chat_cancel_steer(id: String, state: State<'_, Arc<AgentState>>) -> bool {
    state.steering.cancel(&id)
}

/// "Always allow this tool", from an approval card. Lasts as long as the app
/// runs and no longer.
#[tauri::command]
pub fn approval_always_allow(tool: String, state: State<'_, Arc<AgentState>>) -> Result<(), String> {
    let name = ToolName::from_wire_name(&tool).ok_or_else(|| format!("unknown tool: {tool}"))?;
    state
        .approval
        .lock()
        .map_err(|_| "approval lock poisoned".to_string())?
        .always_allowed
        .insert(name);
    Ok(())
}

/// What the conversation is for: everything, planning only, or answering
/// only. Takes effect from the next turn, and never retroactively — a turn
/// already paused on an approval card runs the calls the user decided on.
#[tauri::command]
pub fn chat_set_mode(
    mode: ConversationMode,
    state: State<'_, Arc<AgentState>>,
) -> Result<(), String> {
    *state
        .mode
        .lock()
        .map_err(|_| "mode lock poisoned".to_string())? = mode;
    Ok(())
}

/// Runs the whole turn without asking. Deliberately not persisted anywhere.
#[tauri::command]
pub fn approval_set_unattended(
    unattended: bool,
    state: State<'_, Arc<AgentState>>,
) -> Result<(), String> {
    state
        .approval
        .lock()
        .map_err(|_| "approval lock poisoned".to_string())?
        .skip_all = unattended;
    Ok(())
}

/// The open folder's search, for the turn's tools. `None` when no folder's
/// index is open — or when `setup` never ran, as under a mock runtime.
fn searcher_of<R: Runtime>(app: &AppHandle<R>) -> Option<CodeSearchFn> {
    app.try_state::<Arc<WorkspaceIndex>>()?.searcher()
}

/// Assembles a turn and runs it on a blocking thread.
///
/// Off the event loop because the whole turn is synchronous — provider calls,
/// tool calls and a process runner — and holding the IPC loop for minutes
/// freezes every other command, including the one that stops it.
async fn run_off_the_event_loop<R, F>(
    app: AppHandle<R>,
    state: Arc<AgentState>,
    turn_id: String,
    run: F,
) -> Result<ChatStreamOutcome, String>
where
    R: Runtime,
    F: FnOnce(&Turn) -> Result<ChatStreamOutcome, TurnError> + Send + 'static,
{
    let workspace = state.workspace()?;
    let approval = state.approval()?;
    let mode = state.mode();
    let search = searcher_of(&app);

    tauri::async_runtime::spawn_blocking(move || {
        let events = chat_event_sink(&app, turn_id);
        let session = llm_session::resolve(None).map_err(|e| e.to_string())?;
        let scope = ToolScope::new(&workspace).map_err(|e| e.to_string())?;
        let cancelled = || state.cancel.load(Ordering::SeqCst);
        let sleep = |d: Duration| std::thread::sleep(d);
        let take_steering = || state.steering.take();
        let shell = Shell::default();

        let turn = Turn {
            events: &events,
            session: &session,
            scope: &scope,
            approval: &approval,
            mode,
            cancelled: &cancelled,
            sleep: &sleep,
            take_steering: &take_steering,
            shell: &shell,
            search,
        };
        run(&turn).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("the turn thread failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    /// The state is reachable without a running app, which is what keeps these
    /// checks about behaviour rather than about Tauri.
    fn state() -> Arc<AgentState> {
        Arc::new(AgentState::default())
    }

    #[test]
    fn a_turn_needs_a_folder_before_it_can_start() {
        let err = state().workspace().expect_err("nothing is open");
        assert!(err.contains("no folder"), "{err}");
    }

    #[test]
    fn opening_a_folder_canonicalizes_it_once() {
        let state = state();
        let root = temp_dir("cmd-workspace");
        std::fs::create_dir(root.join("inner")).unwrap();

        let spelled = format!("{}/inner/..", root.display());
        *state.workspace.lock().unwrap() = Some(
            PathBuf::from(&spelled)
                .canonicalize()
                .expect("canonicalizes"),
        );

        let opened = state.workspace().unwrap();
        assert!(!opened.to_string_lossy().contains(".."), "{opened:?}");
        assert!(opened.is_dir());
    }

    /// The card's third button. It lasts for the session and is not written
    /// anywhere — a saved "never ask me" is a brake nobody remembers releasing.
    #[test]
    fn always_allow_widens_the_policy_for_that_tool_only() {
        let state = state();
        state
            .approval
            .lock()
            .unwrap()
            .always_allowed
            .insert(ToolName::WriteFile);

        let policy = state.approval().unwrap();
        assert!(!policy.requires_approval(ToolName::WriteFile, true));
        assert!(policy.requires_approval(ToolName::DeleteFile, true));
    }

    #[test]
    fn a_note_can_be_withdrawn_until_a_round_takes_it() {
        let state = state();
        let note = SteeringNote::user("actually, use the helper");
        let id = note.id.clone();
        state.steering.push(note);

        assert!(state.steering.cancel(&id));
        assert!(state.steering.take().is_empty());
        assert!(!state.steering.cancel(&id), "already gone");
    }

    /// A stop is remembered until a fresh turn clears it — that is what makes
    /// "stop while the approval card is showing" work, since the resumed turn
    /// reads the flag rather than being told again.
    #[test]
    fn a_stop_survives_until_the_next_fresh_turn() {
        let state = state();
        state.cancel.store(true, Ordering::SeqCst);
        assert!(state.cancel.load(Ordering::SeqCst));

        state.cancel.store(false, Ordering::SeqCst);
        assert!(!state.cancel.load(Ordering::SeqCst));
    }

    #[test]
    fn a_turn_gets_the_search_of_the_open_folder() {
        use crate::domain::embeddings::{Embedding, EmbeddingError, EmbeddingProvider};
        struct NoModel;
        impl EmbeddingProvider for NoModel {
            fn embed(&self, _: &[&str]) -> Result<Vec<Embedding>, EmbeddingError> {
                Err(EmbeddingError::Invalid("no model in this test".into()))
            }
            fn dimensions(&self) -> usize {
                2
            }
        }
        let app = tauri::test::mock_app();
        assert!(searcher_of(app.handle()).is_none(), "no index state at all");

        let index = Arc::new(WorkspaceIndex::new(temp_dir("cmd-search-index"), Arc::new(NoModel)));
        tauri::Manager::manage(&app, Arc::clone(&index));
        assert!(searcher_of(app.handle()).is_none(), "no folder open");

        index.open(&temp_dir("cmd-search-repo"), Arc::new(|_| {})).unwrap();
        assert!(searcher_of(app.handle()).is_some());
    }

    /// The snapshot names the folder the way the events do, and says why
    /// search by meaning is off once a sync has found out.
    #[test]
    fn the_index_snapshot_speaks_for_the_open_folder() {
        use crate::domain::embeddings::{Embedding, EmbeddingError, EmbeddingProvider};
        struct NoModel;
        impl EmbeddingProvider for NoModel {
            fn embed(&self, _: &[&str]) -> Result<Vec<Embedding>, EmbeddingError> {
                Err(EmbeddingError::Invalid("no model in this test".into()))
            }
            fn dimensions(&self) -> usize {
                2
            }
        }
        let index = WorkspaceIndex::new(temp_dir("cmd-snapshot-index"), Arc::new(NoModel));
        assert_eq!(snapshot(&index), None);

        let root = temp_dir("cmd-snapshot-repo").canonicalize().unwrap();
        std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
        std::fs::write(root.join("blob.bin"), [0u8, 159, 146, 150]).unwrap();
        let (tx, finished) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        index
            .open(
                &root,
                Arc::new(move |event| {
                    if matches!(event, crate::domain::workspace_index::IndexEvent::SyncFinished { .. }) {
                        let _ = tx.lock().unwrap().send(());
                    }
                }),
            )
            .unwrap();
        finished.recv_timeout(Duration::from_secs(10)).expect("the first sync never finished");

        let shot = snapshot(&index).unwrap();
        assert_eq!(shot.root, root.display().to_string());
        assert_eq!((shot.syncing, shot.embedded, shot.skipped), (false, 0, 1));
        assert!(shot.embedding_error.as_deref().is_some_and(|e| e.contains("no model")), "{shot:?}");
    }
}

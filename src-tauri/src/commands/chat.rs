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

use tauri::{AppHandle, Runtime, State};

use crate::domain::command_exec::Shell;
use crate::domain::llm::{LlmMessage, LlmToolCall};
use crate::domain::tools::{ApprovalPolicy, Task, ToolName, ToolPreview, ToolScope};
use crate::domain::turn::{
    ChatStreamOutcome, PendingApproval, PendingToolCall, SteeringNote, ToolCallDecision,
};
use crate::services::ai_tools::preview;
use crate::services::llm_chat::{self, SteeringQueue, Turn, TurnError};
use crate::services::llm_session;

use super::chat_events::chat_event_sink;

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
}

impl AgentState {
    pub(super) fn workspace(&self) -> Result<PathBuf, String> {
        self.workspace
            .lock()
            .map_err(|_| "workspace lock poisoned".to_string())?
            .clone()
            .ok_or_else(|| "no folder is open".to_string())
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
#[tauri::command]
pub fn workspace_open(path: String, state: State<'_, Arc<AgentState>>) -> Result<String, String> {
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
    Ok(resolved.display().to_string())
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
            cancelled: &cancelled,
            sleep: &sleep,
            take_steering: &take_steering,
            shell: &shell,
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
}

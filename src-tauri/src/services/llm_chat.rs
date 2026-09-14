//! The agent loop and the small decisions a round makes on its own.
//!
//! This file grows in three steps (`docs/06-port-plan.md`, F-1.14): the
//! round-level rules below, then the loop that uses them, then steering. Each
//! rule here is pure and takes no provider, which is what makes it testable
//! without a model at the other end.

use std::cell::Cell;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::Duration;

use crate::domain::llm::{
    ChatRequest, ChatStreamResult, LlmError, LlmMessage, LlmRole, LlmToolCall,
    sanitize_tool_call_arguments,
};
use crate::domain::llm_retry::{MAX_ATTEMPTS, retry_delay};
use crate::domain::tools::{ApprovalPolicy, ReadFiles, Task, ToolName, ToolResult, ToolScope};
use crate::domain::turn::{
    ChatDone, ChatEventPayload, ChatEventSink, ChatStreamOutcome, ChatTurnEvent, DecisionError,
    PendingApproval, PendingToolCall, SteeringNote, ToolCallDecision, ToolCallEvent,
    ToolResultEvent,
};
use crate::infra::llm_debug_log;
use crate::services::ai_tools::parse::{parse_tool_call, preflight_tool_call};
use crate::services::ai_tools::tools::list_files::render_file_tree;
use crate::services::ai_tools::tools::{execute_tool, tool_definitions};
use crate::services::llm_session::LlmSession;

/// How many model↔tool round trips one turn may run.
///
/// A backstop beside [`MAX_TOOL_BUDGET`], not a duplicate of it: a tool whose
/// weight is misconfigured to zero would otherwise make the loop unstoppable,
/// and a model stuck in a cycle must not be able to hold the UI in "thinking"
/// forever.
pub const MAX_TOOL_ITERATIONS: usize = 60;

/// The weighted ceiling, and the one that binds in practice. Counted in
/// [`ToolName::loop_weight`] units so that sixty cheap reads and sixty
/// repository-wide searches are not treated as the same amount of work.
pub const MAX_TOOL_BUDGET: u32 = 250;

/// What the model reads instead of a file it already has verbatim, earlier in
/// this same turn.
const REPEAT_READ_NOTE: &str = "This file was already read in this turn, over the same range, and has not changed since — the result is earlier in the conversation and is not repeated here.";

/// The same, for a search that ranked identically.
const REPEAT_SEARCH_NOTE: &str = "This search already ran in this turn with the same parameters and returned the same result — it is earlier in the conversation and is not repeated here.";

/// Appended to a tool error from a round the provider cut off.
const TRUNCATED_ROUND_NOTE: &str = "Note: the model's reply was cut off mid-way — the response length limit (max_tokens) ran out; the arguments were not malformed. Retry the call more compactly: shorter arguments, fewer files or lines at a time, splitting the work across several calls if needed.";

/// The cost of one round: [`ToolName::loop_weight`] over every call it
/// contains, since a round can bundle several.
///
/// A name that is not a tool costs `1` — the floor of the cheapest real tool —
/// so a hallucinated tool name still moves the budget forward instead of
/// letting the loop spin for free.
pub fn round_cost(calls: &[LlmToolCall]) -> u32 {
    calls
        .iter()
        .map(|call| {
            ToolName::from_wire_name(&call.name)
                .map(ToolName::loop_weight)
                .unwrap_or(1)
        })
        .sum()
}

/// Replaces the body of a result the model has already been given, byte for
/// byte, earlier in this turn.
///
/// Not a correctness fix — re-reading is legitimate, and after a write it is
/// required. It is a context fix: the same file read three times is re-sent in
/// full on every subsequent round of the turn, and so is a search that keeps
/// returning the same hits.
///
/// Keyed on the tool name plus its raw arguments, so a different line range or
/// a different query is a different call, and gated on a hash of the payload,
/// so an edited file — or a search that now ranks differently — comes back in
/// full.
///
/// `result` is `None` for a failed call: an error is short and worth repeating
/// as often as it happens.
pub fn dedupe_repeat_result(
    seen: &mut HashMap<String, u64>,
    call: &LlmToolCall,
    result: Option<&ToolResult>,
    content: String,
) -> String {
    let note = match result {
        Some(ToolResult::File { .. }) => REPEAT_READ_NOTE,
        Some(ToolResult::GrepResults { .. }) => REPEAT_SEARCH_NOTE,
        _ => return content,
    };
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let hash = hasher.finish();
    match seen.insert(format!("{}|{}", call.name, call.arguments), hash) {
        Some(previous) if previous == hash => note.to_string(),
        _ => content,
    }
}

/// Adds [`TRUNCATED_ROUND_NOTE`] to a failed call from a truncated round.
///
/// `finish_reason: "length"` means generation stopped because the response
/// budget ran out — mid-sentence, and just as easily mid-`arguments`. The
/// model is never told that budget exists, so the severed JSON comes back to
/// it as a bare "invalid arguments" error, which invites the obvious wrong
/// repair: send the same oversized call again.
///
/// Only to a **failed** call: one whose arguments happened to close before the
/// cut still executed correctly, and telling the model its successful result
/// was somehow damaged is worse than saying nothing.
///
/// Takes a plain `failed` flag rather than the outcome, because the two places
/// a tool error becomes content for the model do not share a type — the
/// preflight rejects a call before there is any result to inspect, and severed
/// arguments fail exactly there.
pub fn truncated_round_note(round_truncated: bool, failed: bool, content: String) -> String {
    if round_truncated && failed {
        format!("{content}\n\n{TRUNCATED_ROUND_NOTE}")
    } else {
        content
    }
}

/// Everything one turn needs that is not the conversation itself.
///
/// Cancellation and waiting arrive as functions rather than as a flag and a
/// `thread::sleep`, for the same reason the events do: the loop then runs in a
/// test at full speed, and neither the retry wait nor the stop button needs a
/// real clock to be exercised.
pub struct Turn<'a> {
    pub events: &'a ChatEventSink,
    pub session: &'a LlmSession,
    pub scope: &'a ToolScope,
    pub approval: &'a ApprovalPolicy,
    /// Polled between rounds, after a round streams, between individual calls,
    /// and during a retry wait.
    pub cancelled: &'a dyn Fn() -> bool,
    /// Called in one-second slices while waiting to retry, so a stop takes
    /// effect during the wait rather than after it.
    pub sleep: &'a dyn Fn(Duration),
    /// Takes whatever the user has typed since it was last called. Draining
    /// rather than reading is deliberate: a note handed to the model must
    /// leave the queue in the same step, or a round that is retried or
    /// interrupted can deliver it twice.
    pub take_steering: &'a dyn Fn() -> Vec<SteeringNote>,
}

/// Notes typed while a turn is running, waiting for the next round.
///
/// Shared between the turn and whatever accepts the user's typing, so it owns
/// its own lock. A poisoned lock is recovered rather than propagated: losing
/// the queue must not take down a turn that is otherwise fine.
#[derive(Default)]
pub struct SteeringQueue(Mutex<Vec<SteeringNote>>);

impl SteeringQueue {
    pub fn push(&self, note: SteeringNote) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(note);
    }

    pub fn take(&self) -> Vec<SteeringNote> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Removes the note with this id, if it is still queued.
    ///
    /// `false` means it is already gone — a round picked it up while the user
    /// was reaching for cancel. What has been said to the model cannot be
    /// unsaid, and the answer is what lets the caller tell "withdrawn" from
    /// "too late".
    pub fn cancel(&self, id: &str) -> bool {
        let mut notes = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let before = notes.len();
        notes.retain(|note| note.id != id);
        notes.len() != before
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TurnError {
    #[error("{0}")]
    Provider(#[from] LlmError),
    #[error("the assistant did not finish within {rounds} rounds of tool calls. Ask it to continue if it still has work to do.")]
    Exhausted { rounds: u32 },
    #[error("cannot resume: {0}")]
    BadResume(String),
    #[error(transparent)]
    Decision(#[from] DecisionError),
}

/// Assigns each event its place in the stream. The cursor is restored from the
/// checkpoint on resume, which is what keeps one turn's numbering monotonic
/// across the gap.
struct Events<'a> {
    sink: &'a ChatEventSink,
    seq: Cell<u64>,
}

impl<'a> Events<'a> {
    fn new(sink: &'a ChatEventSink, seq: u64) -> Self {
        Self {
            sink,
            seq: Cell::new(seq),
        }
    }

    fn emit(&self, round: u32, target_id: Option<String>, event: ChatEventPayload) {
        let seq = self.seq.get().saturating_add(1);
        self.seq.set(seq);
        (self.sink)(ChatTurnEvent {
            seq,
            round,
            target_id,
            event,
        });
    }

    fn last_seq(&self) -> u64 {
        self.seq.get()
    }
}

/// What the loop carries from round to round, and what a pause has to hand
/// back. One struct rather than eight parameters, because the two entry points
/// below would otherwise have to keep the same order twice.
struct State {
    history: Vec<LlmMessage>,
    round: u32,
    budget_used: u32,
    todos: Vec<Task>,
    reads: ReadFiles,
}

/// A fresh turn: run from the first round until the model stops asking for
/// tools, a call needs a human, or the turn is cancelled.
pub fn stream(
    turn: &Turn,
    messages: Vec<LlmMessage>,
    todos: Vec<Task>,
) -> Result<ChatStreamOutcome, TurnError> {
    // A note queued after the previous turn ended is not part of this one:
    // the user typed it at a conversation that had already finished, and it
    // reaches the model as their next message instead.
    let _ = (turn.take_steering)();
    let state = State {
        history: messages,
        round: 0,
        budget_used: 0,
        todos,
        reads: ReadFiles::default(),
    };
    run(turn, state, 0, None)
}

/// Continues a turn that paused for approval.
///
/// Takes the checkpoint whole, exactly as [`ChatStreamOutcome::PendingApproval`]
/// handed it over. Alfa Atlas takes its six fields apart into six parameters and
/// relies on the front end to reassemble them — a forgotten field is then a
/// silently reset round ceiling rather than a compile error (see
/// `docs/07-upstream-findings.md`, B-4).
pub fn resume(
    turn: &Turn,
    checkpoint: PendingApproval,
    decisions: Vec<ToolCallDecision>,
) -> Result<ChatStreamOutcome, TurnError> {
    checkpoint.check_decisions(&decisions)?;

    // The history has to still end with the assistant's tool-call turn: the
    // resumed round appends this round's tool results, and results with no
    // request in front of them are rejected by the provider — long after the
    // point where the mismatch could be explained.
    match checkpoint.history.last() {
        Some(last) if last.role == LlmRole::Assistant && !last.tool_calls.is_empty() => {}
        _ => {
            return Err(TurnError::BadResume(
                "the history must end with the assistant's tool-call round".to_string(),
            ));
        }
    }

    let calls: Vec<LlmToolCall> = checkpoint
        .calls
        .iter()
        .map(|call| LlmToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        })
        .collect();
    let state = State {
        history: checkpoint.history,
        round: checkpoint.round,
        budget_used: checkpoint.budget_used,
        todos: checkpoint.todos,
        reads: checkpoint.reads,
    };
    run(turn, state, checkpoint.event_seq, Some((calls, decisions)))
}

/// The loop both entry points run.
///
/// `resume` carries a round whose calls are already known and already decided;
/// a fresh round asks the model instead. Everything after that point is the
/// same code, which is the reason a paused turn behaves like an ordinary one.
fn run(
    turn: &Turn,
    mut state: State,
    event_seq: u64,
    mut resume: Option<(Vec<LlmToolCall>, Vec<ToolCallDecision>)>,
) -> Result<ChatStreamOutcome, TurnError> {
    let events = Events::new(turn.events, event_seq);
    // `<tool>|<arguments>` → hash of what came back, see `dedupe_repeat_result`.
    // Not part of the checkpoint: after a pause the first repeat of a read
    // comes back in full once more, which costs context and loses nothing.
    let mut seen_results: HashMap<String, u64> = HashMap::new();

    loop {
        // Checkpoint one. Before the ceiling check as well, so a turn the user
        // stopped reports as cancelled rather than as having run out of rounds.
        if (turn.cancelled)() {
            return Ok(ChatStreamOutcome::Cancelled(ChatDone {
                result: ChatStreamResult::default(),
                todos: state.todos,
            }));
        }
        if state.round >= MAX_TOOL_ITERATIONS as u32 || state.budget_used >= MAX_TOOL_BUDGET {
            return Err(TurnError::Exhausted {
                rounds: state.round,
            });
        }
        state.round += 1;
        let round = state.round;

        // Per round, not per turn: whether *this* round's reply was cut off.
        // A resumed round has no reply of its own — the round that produced
        // these calls already reported, and whatever note it earned is in the
        // history.
        let mut round_truncated = false;

        let (calls, decisions) = if let Some((calls, decisions)) = resume.take() {
            // Charged again on the resumed pass, exactly as `round` is counted
            // twice: otherwise pausing would be a way to buy budget.
            state.budget_used += round_cost(&calls);
            (calls, decisions)
        } else {
            // Before the round is announced, so the notes and the boundary
            // land in the transcript in the order the history has them.
            apply_steering(&events, round, &mut state.history, (turn.take_steering)());
            events.emit(round, Some(format!("round:{round}")), ChatEventPayload::RoundStarted);

            let request = ChatRequest {
                messages: state.history.clone(),
                tools: tool_definitions(),
                model: turn.session.model.clone(),
            };
            let result = match stream_one_round(turn, &events, round, request)? {
                Some(result) => result,
                // Cancelled during a retry wait.
                None => {
                    return Ok(ChatStreamOutcome::Cancelled(ChatDone {
                        result: ChatStreamResult::default(),
                        todos: state.todos,
                    }));
                }
            };

            round_truncated = result.truncated;
            if let Some(usage) = result.usage {
                events.emit(
                    round,
                    Some(format!("round:{round}")),
                    ChatEventPayload::ContextUsage(usage),
                );
            }
            // Said outright rather than left to the deltas that streamed it,
            // and said before the two exits below: a round that is about to be
            // cancelled, or to pause on a confirmation, has still reported
            // what it said.
            events.emit(
                round,
                Some(format!("round:{round}")),
                ChatEventPayload::RoundCompleted {
                    text: result.text.clone(),
                    reasoning: result.reasoning.clone(),
                },
            );

            // Checkpoint two. Before the pause check and before any call runs,
            // so a stop that landed as the round finished pre-empts the write
            // that round asked for — not merely the model's next sentence.
            if (turn.cancelled)() {
                return Ok(ChatStreamOutcome::Cancelled(ChatDone {
                    result,
                    todos: state.todos,
                }));
            }

            if result.tool_calls.is_empty() {
                // The model is done — unless the user said something while it
                // was answering. Ending the turn here would silently drop
                // what they typed, and they would have no way to tell it was
                // never seen.
                let waiting = (turn.take_steering)();
                if waiting.is_empty() {
                    return Ok(ChatStreamOutcome::Done(ChatDone {
                        result,
                        todos: state.todos,
                    }));
                }
                state.history.push(LlmMessage {
                    role: LlmRole::Assistant,
                    content: (!result.text.is_empty()).then(|| result.text.clone()),
                    tool_call_id: None,
                    tool_calls: vec![],
                });
                apply_steering(&events, round, &mut state.history, waiting);
                continue;
            }

            // The assistant's own turn goes back into the history before its
            // results do, so the next request shows the provider its own prior
            // request. `None` content for a tool-only turn is what the wire
            // actually says.
            state.history.push(LlmMessage {
                role: LlmRole::Assistant,
                content: (!result.text.is_empty()).then(|| result.text.clone()),
                tool_call_id: None,
                tool_calls: sanitize_tool_call_arguments(&result.tool_calls),
            });
            state.budget_used += round_cost(&result.tool_calls);

            // Containment before approval: a write outside the workspace has
            // to fail as a tool error now, not show the user a card for an
            // operation that cannot happen. Severed arguments fail here too,
            // which is the case the truncation note exists for.
            let mut runnable: Vec<LlmToolCall> = Vec::new();
            for call in &result.tool_calls {
                match preflight_tool_call(turn.scope, &state.reads, call) {
                    Ok(()) => runnable.push(call.clone()),
                    Err(e) => {
                        report_call(&events, round, call);
                        let message = format!("Error: {e}");
                        report_result(&events, round, &call.id, None, Some(&message));
                        state.history.push(tool_message(
                            &call.id,
                            truncated_round_note(round_truncated, true, message),
                        ));
                    }
                }
            }
            if runnable.is_empty() {
                // Every call in the round was refused before running. Let the
                // model react to the errors on the next round.
                continue;
            }

            let pending: Vec<PendingToolCall> = runnable
                .iter()
                .map(|call| PendingToolCall {
                    requires_confirmation: needs_approval(turn.approval, call),
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                })
                .collect();
            if pending.iter().any(|call| call.requires_confirmation) {
                // The whole round pauses, including the calls that need no
                // decision: with nothing executed there is no partial round to
                // describe to whoever resumes it.
                return Ok(ChatStreamOutcome::PendingApproval(PendingApproval {
                    history: state.history,
                    round,
                    budget_used: state.budget_used,
                    event_seq: events.last_seq(),
                    calls: pending,
                    todos: state.todos,
                    reads: state.reads,
                }));
            }

            (runnable, Vec::new())
        };

        for call in &calls {
            // The "stopped between two calls of one round" case. Breaking
            // rather than returning here lets checkpoint one build the outcome,
            // so there is one place that decides what a cancelled turn looks
            // like.
            if (turn.cancelled)() {
                break;
            }
            report_call(&events, round, call);

            let decision = decisions.iter().find(|d| d.id == call.id);
            let outcome = match decision {
                Some(d) if !d.approved => Err(denial(d)),
                _ => parse_tool_call(call)
                    .and_then(|parsed| {
                        execute_tool(turn.scope, &parsed, &mut state.reads, &mut state.todos)
                    })
                    .map_err(|e| format!("Error: {e}")),
            };

            report_result(
                &events,
                round,
                &call.id,
                outcome.as_ref().ok(),
                outcome.as_ref().err().map(String::as_str),
            );

            // What the model reads, as opposed to what the UI was given: a
            // listing is a tree rather than a flat array of paths, because the
            // model would otherwise have to rebuild the directory structure
            // from N separate strings.
            let content = match &outcome {
                Ok(ToolResult::FileList { entries, truncated }) => {
                    render_file_tree(entries, *truncated)
                }
                Ok(result) => serde_json::to_string(result)
                    .unwrap_or_else(|_| "Error: the result could not be serialized".to_string()),
                Err(message) => message.clone(),
            };
            let content = truncated_round_note(round_truncated, outcome.is_err(), content);
            let content =
                dedupe_repeat_result(&mut seen_results, call, outcome.as_ref().ok(), content);
            state.history.push(tool_message(&call.id, content));
        }
    }
}

/// Adds queued notes to the conversation and says so, one event per note, so
/// the front end can retire each by id rather than by matching its text.
fn apply_steering(
    events: &Events,
    round: u32,
    history: &mut Vec<LlmMessage>,
    notes: Vec<SteeringNote>,
) {
    for note in notes {
        history.push(LlmMessage::user(note.prefixed()));
        events.emit(
            round,
            Some(format!("steer:{}", note.id)),
            ChatEventPayload::SteeringApplied {
                id: note.id,
                text: note.text,
            },
        );
    }
}

/// One round against the provider, retried while [`retry_delay`] allows it.
///
/// `Ok(None)` means the turn was cancelled during a wait — the caller turns
/// that into the same cancelled outcome as every other stopping point.
fn stream_one_round(
    turn: &Turn,
    events: &Events,
    round: u32,
    request: ChatRequest,
) -> Result<Option<ChatStreamResult>, TurnError> {
    let mut attempt = 0;
    loop {
        llm_debug_log::log_request(turn.session.debug_logging, &turn.session.provider_id, round, &request);

        // Set by any callback below: once a byte of this attempt has reached
        // us, the round is no longer repeatable.
        let produced_output = Cell::new(false);
        let on_delta = |delta: &str| {
            produced_output.set(true);
            events.emit(
                round,
                Some(format!("round:{round}:text")),
                ChatEventPayload::Delta {
                    delta: delta.to_string(),
                },
            );
        };
        let on_reasoning = |delta: &str| {
            produced_output.set(true);
            events.emit(
                round,
                Some(format!("round:{round}:reasoning")),
                ChatEventPayload::Reasoning {
                    delta: delta.to_string(),
                },
            );
        };
        let on_tool_call_delta = |id: &str, name: &str, arguments: &str| {
            produced_output.set(true);
            events.emit(
                round,
                Some(format!("round:{round}:tool:{id}")),
                ChatEventPayload::ToolCallDelta(ToolCallEvent {
                    id: id.to_string(),
                    name: name.to_string(),
                    arguments: arguments.to_string(),
                }),
            );
        };

        let result = turn.session.provider.chat_stream(
            request.clone(),
            &on_delta,
            &on_reasoning,
            &on_tool_call_delta,
            turn.cancelled,
        );
        llm_debug_log::log_response(turn.session.debug_logging, &turn.session.provider_id, round, &result);

        let error = match result {
            Ok(result) => return Ok(Some(result)),
            Err(error) => error,
        };
        let Some(delay) = retry_delay(&error, attempt, produced_output.get()) else {
            return Err(TurnError::Provider(error));
        };
        attempt += 1;
        events.emit(
            round,
            Some(format!("round:{round}")),
            ChatEventPayload::Retrying {
                attempt,
                max_attempts: MAX_ATTEMPTS,
                delay_seconds: delay.as_secs(),
            },
        );
        if !wait(turn, delay) {
            return Ok(None);
        }
    }
}

/// Sleeps in one-second slices, checking for a stop between them. `false` if
/// the turn was cancelled before the wait was over — a minute-long wait that
/// ignored the stop button would look exactly like a hang.
fn wait(turn: &Turn, delay: Duration) -> bool {
    let slice = Duration::from_secs(1);
    let mut left = delay;
    while !left.is_zero() {
        if (turn.cancelled)() {
            return false;
        }
        let step = left.min(slice);
        (turn.sleep)(step);
        left -= step;
    }
    !(turn.cancelled)()
}

/// Whether this call has to be shown to a human first.
///
/// An unparseable call is never risky: it cannot run, and asking about a call
/// that is going to fail either way spends the user's attention on nothing.
fn needs_approval(policy: &ApprovalPolicy, call: &LlmToolCall) -> bool {
    match parse_tool_call(call) {
        Ok(parsed) => policy.requires_approval(parsed.name(), parsed.is_risky()),
        Err(_) => false,
    }
}

/// What a refused call tells the model. The reason is the point: a model told
/// only "denied" tries the same call again, then a near variant of it.
fn denial(decision: &ToolCallDecision) -> String {
    match &decision.reason {
        Some(reason) if !reason.trim().is_empty() => {
            format!("Denied by the user: {reason}")
        }
        _ => "Denied by the user.".to_string(),
    }
}

fn tool_message(call_id: &str, content: String) -> LlmMessage {
    LlmMessage {
        role: LlmRole::Tool,
        content: Some(content),
        tool_call_id: Some(call_id.to_string()),
        tool_calls: vec![],
    }
}

fn report_call(events: &Events, round: u32, call: &LlmToolCall) {
    events.emit(
        round,
        Some(format!("round:{round}:tool:{}", call.id)),
        ChatEventPayload::ToolCall(ToolCallEvent {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        }),
    );
}

fn report_result(
    events: &Events,
    round: u32,
    call_id: &str,
    result: Option<&ToolResult>,
    error: Option<&str>,
) {
    events.emit(
        round,
        Some(format!("round:{round}:tool:{call_id}")),
        ChatEventPayload::ToolResult(ToolResultEvent {
            id: call_id.to_string(),
            result: result.cloned(),
            error: error.map(str::to_string),
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::llm::{
        ChatResponse, ChatUsage, LlmModelInfo, LlmProvider, LlmToolDefinition,
    };
    use crate::domain::tools::{ToolScope, ToolName};
    use crate::domain::turn::{ChatTurnEvent, STEERING_PREFIX};
    use crate::testing::temp_dir;
    use std::collections::{HashSet, VecDeque};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    // ---------------------------------------------------------------- doubles

    /// One scripted answer from the provider.
    enum Step {
        Reply(ChatStreamResult),
        Fail(LlmError),
        /// Streams text and *then* fails — the shape that must never be
        /// retried, since half the round has already been reported.
        StreamThenFail(&'static str, LlmError),
        /// Answers, and the user types while it does. The only way to queue a
        /// note *during* a turn when the provider is synchronous.
        ReplyWhileTheUserTypes(ChatStreamResult, &'static str),
    }

    fn text_while_typing(answer: &str, note: &'static str) -> Step {
        Step::ReplyWhileTheUserTypes(
            ChatStreamResult {
                text: answer.to_string(),
                ..Default::default()
            },
            note,
        )
    }

    fn asks_while_typing(calls: Vec<LlmToolCall>, note: &'static str) -> Step {
        Step::ReplyWhileTheUserTypes(
            ChatStreamResult {
                tool_calls: calls,
                ..Default::default()
            },
            note,
        )
    }

    fn text(answer: &str) -> Step {
        Step::Reply(ChatStreamResult {
            text: answer.to_string(),
            ..Default::default()
        })
    }

    fn asks(calls: Vec<LlmToolCall>) -> Step {
        Step::Reply(ChatStreamResult {
            tool_calls: calls,
            ..Default::default()
        })
    }

    fn wants(id: &str, name: &str, arguments: &str) -> LlmToolCall {
        LlmToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        }
    }

    struct Scripted {
        steps: Mutex<VecDeque<Step>>,
        requests: Mutex<Vec<ChatRequest>>,
        steering: Mutex<Option<Arc<SteeringQueue>>>,
    }

    impl Scripted {
        fn new(steps: Vec<Step>) -> Arc<Self> {
            Arc::new(Self {
                steps: Mutex::new(steps.into()),
                requests: Mutex::new(Vec::new()),
                steering: Mutex::new(None),
            })
        }

        fn requests(&self) -> Vec<ChatRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl LlmProvider for Scripted {
        fn chat(&self, _: ChatRequest) -> Result<ChatResponse, LlmError> {
            unreachable!("the loop only ever streams")
        }

        fn chat_stream(
            &self,
            request: ChatRequest,
            on_delta: &dyn Fn(&str),
            _: &dyn Fn(&str),
            _: &dyn Fn(&str, &str, &str),
            _: &dyn Fn() -> bool,
        ) -> Result<ChatStreamResult, LlmError> {
            self.requests.lock().unwrap().push(request);
            let step = self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| panic!("the loop asked for more rounds than the script has"));
            match step {
                Step::Reply(result) => {
                    if !result.text.is_empty() {
                        on_delta(&result.text);
                    }
                    Ok(result)
                }
                Step::Fail(error) => Err(error),
                Step::StreamThenFail(chunk, error) => {
                    on_delta(chunk);
                    Err(error)
                }
                Step::ReplyWhileTheUserTypes(result, note) => {
                    if let Some(queue) = self.steering.lock().unwrap().as_ref() {
                        queue.push(SteeringNote::user(note));
                    }
                    if !result.text.is_empty() {
                        on_delta(&result.text);
                    }
                    Ok(result)
                }
            }
        }

        fn list_models(&self) -> Result<Vec<LlmModelInfo>, LlmError> {
            unreachable!("the loop never lists models")
        }
    }

    /// Everything a turn needs, with the pieces a test wants to reach back
    /// into kept out here.
    struct Harness {
        provider: Arc<Scripted>,
        session: LlmSession,
        scope: ToolScope,
        root: PathBuf,
        events: ChatEventSink,
        log: Arc<Mutex<Vec<ChatTurnEvent>>>,
        approval: ApprovalPolicy,
        cancel_after: Arc<Mutex<Option<usize>>>,
        polls: Arc<Mutex<usize>>,
        slept: Arc<Mutex<Vec<Duration>>>,
        steering: Arc<SteeringQueue>,
    }

    fn harness(label: &str, steps: Vec<Step>) -> Harness {
        let root = temp_dir(label);
        let provider = Scripted::new(steps);
        let log: Arc<Mutex<Vec<ChatTurnEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = log.clone();
        let steering = Arc::new(SteeringQueue::default());
        *provider.steering.lock().unwrap() = Some(steering.clone());
        Harness {
            session: LlmSession {
                provider: provider.clone(),
                provider_id: "test".to_string(),
                model: "m".to_string(),
                debug_logging: false,
            },
            provider,
            scope: ToolScope::new(&root).expect("a scope over the temp root"),
            root,
            events: Arc::new(move |event| sink.lock().unwrap().push(event)),
            log,
            // Unattended by default: the approval gate has its own tests, and
            // every other test would otherwise pause on its first write.
            approval: ApprovalPolicy {
                always_allowed: HashSet::new(),
                skip_all: true,
            },
            cancel_after: Arc::new(Mutex::new(None)),
            polls: Arc::new(Mutex::new(0)),
            slept: Arc::new(Mutex::new(Vec::new())),
            steering,
        }
    }

    impl Harness {
        /// Reports "cancelled" from the `n`-th poll onwards, which is how a
        /// test picks the checkpoint it wants to stop at.
        fn cancel_at_poll(&self, n: usize) {
            *self.cancel_after.lock().unwrap() = Some(n);
        }

        fn events(&self) -> Vec<ChatTurnEvent> {
            self.log.lock().unwrap().clone()
        }

        fn run<T>(&self, f: impl FnOnce(&Turn) -> T) -> T {
            let cancel_after = self.cancel_after.clone();
            let polls = self.polls.clone();
            let cancelled = move || {
                let mut polls = polls.lock().unwrap();
                *polls += 1;
                matches!(*cancel_after.lock().unwrap(), Some(n) if *polls >= n)
            };
            let slept = self.slept.clone();
            let sleep = move |d: Duration| slept.lock().unwrap().push(d);
            let queue = self.steering.clone();
            let take_steering = move || queue.take();
            let turn = Turn {
                events: &self.events,
                session: &self.session,
                scope: &self.scope,
                approval: &self.approval,
                cancelled: &cancelled,
                sleep: &sleep,
                take_steering: &take_steering,
            };
            f(&turn)
        }
    }

    fn payloads(events: &[ChatTurnEvent]) -> Vec<String> {
        events
            .iter()
            .map(|e| match &e.event {
                ChatEventPayload::Delta { .. } => "delta".to_string(),
                ChatEventPayload::Reasoning { .. } => "reasoning".to_string(),
                ChatEventPayload::Retrying { .. } => "retrying".to_string(),
                ChatEventPayload::RoundStarted => "roundStarted".to_string(),
                ChatEventPayload::RoundCompleted { .. } => "roundCompleted".to_string(),
                ChatEventPayload::ToolCallDelta(_) => "toolCallDelta".to_string(),
                ChatEventPayload::ToolCall(c) => format!("toolCall:{}", c.id),
                ChatEventPayload::ToolResult(r) => format!("toolResult:{}", r.id),
                ChatEventPayload::ContextUsage(_) => "contextUsage".to_string(),
                ChatEventPayload::SteeringApplied { id, .. } => format!("steering:{id}"),
            })
            .collect()
    }

    fn tool_contents(request: &ChatRequest) -> Vec<String> {
        request
            .messages
            .iter()
            .filter(|m| m.role == LlmRole::Tool)
            .filter_map(|m| m.content.clone())
            .collect()
    }


    fn call(name: &str, arguments: &str) -> LlmToolCall {
        LlmToolCall {
            id: "call_1".to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        }
    }

    fn file(content: &str) -> ToolResult {
        ToolResult::File {
            content: content.to_string(),
            start_line: 1,
            end_line: 1,
            total_lines: 1,
        }
    }

    fn grep() -> ToolResult {
        ToolResult::GrepResults {
            matches: vec![],
            truncated: false,
        }
    }

    #[test]
    fn a_round_costs_the_weight_of_every_call_in_it() {
        let cost = round_cost(&[call("readFile", "{}"), call("grep", "{}")]);
        assert_eq!(cost, ToolName::ReadFile.loop_weight() + ToolName::Grep.loop_weight());
    }

    #[test]
    fn a_round_with_no_calls_is_free() {
        assert_eq!(round_cost(&[]), 0);
    }

    /// Otherwise a model that keeps inventing tool names spins against a
    /// budget that never moves.
    #[test]
    fn an_invented_tool_name_still_costs_something() {
        assert_eq!(round_cost(&[call("summonDragon", "{}")]), 1);
    }

    #[test]
    fn the_same_read_twice_is_replaced_by_a_note() {
        let mut seen = HashMap::new();
        let call = call("readFile", r#"{"path":"a.rs"}"#);

        let first = dedupe_repeat_result(&mut seen, &call, Some(&file("x")), "x".to_string());
        let second = dedupe_repeat_result(&mut seen, &call, Some(&file("x")), "x".to_string());

        assert_eq!(first, "x");
        assert_eq!(second, REPEAT_READ_NOTE);
    }

    /// The gate that makes this safe after a write: the file changed, so the
    /// model must see it, however many times it has read that path before.
    #[test]
    fn a_changed_file_comes_back_in_full() {
        let mut seen = HashMap::new();
        let call = call("readFile", r#"{"path":"a.rs"}"#);

        dedupe_repeat_result(&mut seen, &call, Some(&file("before")), "before".to_string());
        let after =
            dedupe_repeat_result(&mut seen, &call, Some(&file("after")), "after".to_string());
        assert_eq!(after, "after");

        // And the next identical read is deduped against the *new* content,
        // not the one from before the write.
        let again =
            dedupe_repeat_result(&mut seen, &call, Some(&file("after")), "after".to_string());
        assert_eq!(again, REPEAT_READ_NOTE);
    }

    #[test]
    fn a_different_range_or_query_is_a_different_call() {
        let mut seen = HashMap::new();
        let body = "x".to_string();

        let first = call("readFile", r#"{"path":"a.rs","startLine":1}"#);
        let second = call("readFile", r#"{"path":"a.rs","startLine":40}"#);
        dedupe_repeat_result(&mut seen, &first, Some(&file("x")), body.clone());
        let other = dedupe_repeat_result(&mut seen, &second, Some(&file("x")), body.clone());

        assert_eq!(other, "x", "same content, but the model asked for something else");
    }

    #[test]
    fn a_repeated_search_is_replaced_by_its_own_note() {
        let mut seen = HashMap::new();
        let call = call("grep", r#"{"pattern":"fn main"}"#);

        dedupe_repeat_result(&mut seen, &call, Some(&grep()), "hits".to_string());
        let second = dedupe_repeat_result(&mut seen, &call, Some(&grep()), "hits".to_string());

        assert_eq!(second, REPEAT_SEARCH_NOTE);
    }

    /// Only results the model can re-derive from the transcript are worth
    /// suppressing. A write confirmation is one line and has to be seen every
    /// time, and an error is worth repeating as often as it happens.
    #[test]
    fn nothing_else_is_ever_suppressed() {
        let mut seen = HashMap::new();
        let written = ToolResult::DirectoryCreated {
            path: "src".to_string(),
        };
        let call = call("createDirectory", r#"{"path":"src"}"#);

        for _ in 0..3 {
            let content =
                dedupe_repeat_result(&mut seen, &call, Some(&written), "created".to_string());
            assert_eq!(content, "created");
        }
        for _ in 0..3 {
            let content = dedupe_repeat_result(&mut seen, &call, None, "failed".to_string());
            assert_eq!(content, "failed");
        }
    }

    #[test]
    fn a_failed_call_from_a_cut_off_round_is_told_why() {
        let note = truncated_round_note(true, true, "invalid arguments".to_string());
        assert!(note.starts_with("invalid arguments"), "{note}");
        assert!(note.contains("max_tokens"), "{note}");
    }

    /// A call that closed before the cut executed correctly; calling its
    /// result damaged would be worse than saying nothing.
    #[test]
    fn a_call_that_succeeded_is_not_told_the_round_was_cut() {
        assert_eq!(truncated_round_note(true, false, "ok".to_string()), "ok");
    }

    #[test]
    fn an_ordinary_failure_carries_no_note() {
        assert_eq!(
            truncated_round_note(false, true, "no such file".to_string()),
            "no such file"
        );
    }

    // ------------------------------------------------------------- the loop

    #[test]
    fn a_turn_with_no_tool_calls_answers_and_stops() {
        let h = harness("loop-plain", vec![text("done")]);

        let outcome = h.run(|turn| stream(turn, vec![LlmMessage::user("hi")], vec![]));

        let ChatStreamOutcome::Done(done) = outcome.expect("finishes") else {
            panic!("expected Done");
        };
        assert_eq!(done.result.text, "done");
        assert_eq!(h.provider.requests().len(), 1, "one round, one request");
    }

    /// The loop's actual job: a call runs, and what it produced goes back to
    /// the model as the next request's history.
    #[test]
    fn a_tool_result_reaches_the_next_round() {
        std::fs::write("/dev/null", "").ok();
        let h = harness(
            "loop-tool",
            vec![
                asks(vec![wants("c1", "createDirectory", r#"{"path":"src"}"#)]),
                text("made it"),
            ],
        );

        let outcome = h.run(|turn| stream(turn, vec![LlmMessage::user("make src")], vec![]));

        assert!(matches!(outcome.expect("finishes"), ChatStreamOutcome::Done(_)));
        assert!(h.root.join("src").is_dir(), "the tool actually ran");

        let second = &h.provider.requests()[1];
        let results = tool_contents(second);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("directoryCreated"), "{}", results[0]);
        // The assistant's own tool-call turn has to precede its results, or
        // the provider sees answers to a question it was never shown.
        let assistant = second
            .messages
            .iter()
            .find(|m| m.role == LlmRole::Assistant)
            .expect("the tool-call turn is in the history");
        assert_eq!(assistant.tool_calls.len(), 1);
    }

    /// The order is the contract: a listener pairs a call with its result by
    /// id, and orders everything by `seq`.
    #[test]
    fn events_are_numbered_in_order_and_pair_by_id() {
        let h = harness(
            "loop-events",
            vec![
                asks(vec![wants("c1", "createDirectory", r#"{"path":"a"}"#)]),
                text("ok"),
            ],
        );

        h.run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![])).expect("finishes");

        let events = h.events();
        assert_eq!(
            payloads(&events),
            [
                "roundStarted",
                "roundCompleted",
                "toolCall:c1",
                "toolResult:c1",
                "roundStarted",
                "delta",
                "roundCompleted",
            ]
        );
        let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, (1..=events.len() as u64).collect::<Vec<_>>());
        assert_eq!(events[0].round, 1);
        assert_eq!(events.last().unwrap().round, 2);
    }

    // ------------------------------------------------------------- approval

    fn asking() -> ApprovalPolicy {
        ApprovalPolicy {
            always_allowed: HashSet::new(),
            skip_all: false,
        }
    }

    /// Nothing in the round runs — not even the calls that needed no decision.
    /// A half-executed round is a state nobody could describe to whoever
    /// resumes it.
    #[test]
    fn a_risky_call_pauses_the_whole_round_with_nothing_run() {
        let mut h = harness(
            "loop-pause",
            vec![asks(vec![
                wants("l1", "listFiles", "{}"),
                wants("w1", "writeFile", r#"{"path":"a.rs","content":"x"}"#),
            ])],
        );
        h.approval = asking();

        let outcome = h.run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![]));

        let ChatStreamOutcome::PendingApproval(pending) = outcome.expect("pauses") else {
            panic!("expected a pause");
        };
        assert_eq!(
            pending.calls.iter().map(|c| c.requires_confirmation).collect::<Vec<_>>(),
            [false, true],
            "both calls are carried, only the write needs an answer"
        );
        assert!(!h.root.join("a.rs").exists());
        assert!(
            !payloads(&h.events()).iter().any(|p| p.starts_with("toolCall:")),
            "the harmless call did not run either — nothing in the round did"
        );
        assert_eq!(pending.round, 1);
        assert!(pending.budget_used > 0, "a paused round is still charged");
        assert!(
            payloads(&h.events()).contains(&"roundCompleted".to_string()),
            "a round that pauses has still reported what it said"
        );
    }

    /// The registry has to cross the pause. Without it the write the user just
    /// approved is refused for never having read the file — the pause itself
    /// would be what broke it.
    #[test]
    fn a_read_from_before_the_pause_still_counts_after_it() {
        std::fs::write(temp_dir("loop-seed").join("ignored"), "").ok();
        let mut h = harness(
            "loop-resume-reads",
            vec![
                asks(vec![wants("r1", "readFile", r#"{"path":"a.rs"}"#)]),
                asks(vec![wants(
                    "w1",
                    "writeFile",
                    r#"{"path":"a.rs","content":"new"}"#,
                )]),
                text("written"),
            ],
        );
        h.approval = asking();
        std::fs::write(h.root.join("a.rs"), "old").unwrap();

        let paused = h.run(|turn| stream(turn, vec![LlmMessage::user("rewrite a.rs")], vec![]));
        let ChatStreamOutcome::PendingApproval(pending) = paused.expect("pauses") else {
            panic!("expected a pause");
        };

        let resumed = h.run(|turn| {
            resume(
                turn,
                pending,
                vec![ToolCallDecision {
                    id: "w1".to_string(),
                    approved: true,
                    reason: None,
                }],
            )
        });

        assert!(matches!(resumed.expect("finishes"), ChatStreamOutcome::Done(_)));
        assert_eq!(std::fs::read_to_string(h.root.join("a.rs")).unwrap(), "new");
    }

    /// A refusal is not a failure of the turn, and the reason is what stops
    /// the model from trying the same call again.
    #[test]
    fn a_denied_call_hands_the_model_the_reason_and_the_turn_continues() {
        let mut h = harness(
            "loop-denied",
            vec![
                asks(vec![wants("w1", "writeFile", r#"{"path":"a.rs","content":"x"}"#)]),
                text("understood"),
            ],
        );
        h.approval = asking();

        let paused = h.run(|turn| stream(turn, vec![LlmMessage::user("write it")], vec![]));
        let ChatStreamOutcome::PendingApproval(pending) = paused.expect("pauses") else {
            panic!("expected a pause");
        };
        let resumed = h.run(|turn| {
            resume(
                turn,
                pending,
                vec![ToolCallDecision {
                    id: "w1".to_string(),
                    approved: false,
                    reason: Some("use the existing helper".to_string()),
                }],
            )
        });

        assert!(matches!(resumed.expect("finishes"), ChatStreamOutcome::Done(_)));
        assert!(!h.root.join("a.rs").exists(), "a denied call must not run");
        let told = tool_contents(h.provider.requests().last().unwrap());
        assert!(told[0].contains("use the existing helper"), "{}", told[0]);
    }

    /// Pausing must not be a way to buy more budget: the resumed pass charges
    /// the round again, exactly as `round` itself is counted twice.
    #[test]
    fn a_paused_round_is_charged_on_both_passes() {
        let write = |id: &str| wants(id, "writeFile", r#"{"path":"a.rs","content":"x"}"#);
        let mut h = harness(
            "loop-budget",
            vec![asks(vec![write("w1")]), asks(vec![write("w2")])],
        );
        h.approval = asking();
        let weight = ToolName::WriteFile.loop_weight();

        let ChatStreamOutcome::PendingApproval(first) = h
            .run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![]))
            .expect("pauses")
        else {
            panic!("expected a pause");
        };
        assert_eq!(first.budget_used, weight);

        let approve = |id: &str| ToolCallDecision {
            id: id.to_string(),
            approved: true,
            reason: None,
        };
        let ChatStreamOutcome::PendingApproval(second) = h
            .run(|turn| resume(turn, first, vec![approve("w1")]))
            .expect("pauses again")
        else {
            panic!("expected a second pause");
        };

        assert_eq!(
            second.budget_used,
            weight * 3,
            "the resumed round is charged again, and then the new round"
        );
        assert_eq!(second.round, 3, "and the round counter moves the same way");
    }

    // ---------------------------------------------------------------- resume

    #[test]
    fn a_resume_missing_a_decision_is_refused() {
        let h = harness("loop-resume-missing", vec![]);
        let pending = PendingApproval {
            history: vec![LlmMessage {
                role: LlmRole::Assistant,
                content: None,
                tool_call_id: None,
                tool_calls: vec![wants("w1", "writeFile", "{}")],
            }],
            round: 1,
            budget_used: 2,
            event_seq: 4,
            calls: vec![PendingToolCall {
                id: "w1".to_string(),
                name: "writeFile".to_string(),
                arguments: "{}".to_string(),
                requires_confirmation: true,
            }],
            todos: vec![],
            reads: ReadFiles::default(),
        };

        let err = h.run(|turn| resume(turn, pending, vec![])).expect_err("refused");
        assert!(matches!(err, TurnError::Decision(_)), "{err}");
    }

    /// Tool results with no request in front of them are rejected by the
    /// provider, far from the point where the mismatch could be explained.
    #[test]
    fn a_resume_whose_history_lost_the_tool_call_round_is_refused() {
        let h = harness("loop-resume-history", vec![]);
        let pending = PendingApproval {
            history: vec![LlmMessage::user("go")],
            round: 1,
            budget_used: 0,
            event_seq: 0,
            calls: vec![],
            todos: vec![],
            reads: ReadFiles::default(),
        };

        let err = h.run(|turn| resume(turn, pending, vec![])).expect_err("refused");
        assert!(matches!(err, TurnError::BadResume(_)), "{err}");
    }

    /// One turn, one stream of numbers: a listener that reconnects after the
    /// pause cannot order anything if resuming starts again from zero.
    #[test]
    fn the_event_stream_continues_across_a_pause() {
        let mut h = harness(
            "loop-seq",
            vec![
                asks(vec![wants("w1", "writeFile", r#"{"path":"a.rs","content":"x"}"#)]),
                text("ok"),
            ],
        );
        h.approval = asking();

        let paused = h.run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![]));
        let ChatStreamOutcome::PendingApproval(pending) = paused.expect("pauses") else {
            panic!("expected a pause");
        };
        let before = h.events().last().expect("events").seq;
        assert_eq!(pending.event_seq, before);

        let decisions = vec![ToolCallDecision {
            id: "w1".to_string(),
            approved: true,
            reason: None,
        }];
        h.run(|turn| resume(turn, pending, decisions)).expect("finishes");

        let seqs: Vec<u64> = h.events().iter().map(|e| e.seq).collect();
        assert_eq!(seqs, (1..=seqs.len() as u64).collect::<Vec<_>>());
    }

    // ----------------------------------------------------------- cancelling

    #[test]
    fn a_stop_before_the_first_round_runs_nothing() {
        let h = harness("loop-cancel-early", vec![]);
        h.cancel_at_poll(1);

        let outcome = h.run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![]));

        assert!(matches!(outcome.expect("stops"), ChatStreamOutcome::Cancelled(_)));
        assert!(h.provider.requests().is_empty(), "the model was never asked");
    }

    /// The point of the second checkpoint: a stop that lands as the round
    /// finishes pre-empts the write that round asked for, not merely the
    /// model's next sentence.
    #[test]
    fn a_stop_as_the_round_finishes_pre_empts_its_tool_calls() {
        let h = harness(
            "loop-cancel-mid",
            vec![asks(vec![wants(
                "w1",
                "createDirectory",
                r#"{"path":"never"}"#,
            )])],
        );
        // Poll 1 is the top of the round; poll 2 is the provider's own
        // cancellation callback; poll 3 is the checkpoint after it returns.
        h.cancel_at_poll(3);

        let outcome = h.run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![]));

        assert!(matches!(outcome.expect("stops"), ChatStreamOutcome::Cancelled(_)));
        assert!(!h.root.join("never").exists(), "the call was pre-empted");
        assert!(
            !payloads(&h.events()).iter().any(|p| p.starts_with("toolCall:")),
            "and was never even announced"
        );
    }

    // ------------------------------------------------------------- retrying

    fn rate_limited(seconds: u64) -> LlmError {
        LlmError::RateLimited {
            retry_after_seconds: Some(seconds),
            message: "slow down".to_string(),
        }
    }

    #[test]
    fn a_rate_limited_round_waits_and_runs_again() {
        let h = harness(
            "loop-retry",
            vec![Step::Fail(rate_limited(3)), text("second time lucky")],
        );

        let outcome = h.run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![]));

        let ChatStreamOutcome::Done(done) = outcome.expect("finishes") else {
            panic!("expected Done");
        };
        assert_eq!(done.result.text, "second time lucky");
        assert_eq!(h.provider.requests().len(), 2, "the same round, twice");
        assert_eq!(
            h.slept.lock().unwrap().iter().sum::<Duration>(),
            Duration::from_secs(3),
            "waited exactly as long as the server asked"
        );
        assert!(payloads(&h.events()).contains(&"retrying".to_string()));
    }

    /// The rule that makes retrying safe at all: half the round has already
    /// reached the transcript, and sending the request again would append the
    /// text twice.
    #[test]
    fn a_round_that_already_streamed_is_not_retried() {
        let h = harness(
            "loop-retry-unsafe",
            vec![Step::StreamThenFail("half an answer", rate_limited(1))],
        );

        let err = h
            .run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![]))
            .expect_err("gives up");

        assert!(matches!(err, TurnError::Provider(LlmError::RateLimited { .. })), "{err}");
        assert_eq!(h.provider.requests().len(), 1, "asked once, never repeated");
        assert!(h.slept.lock().unwrap().is_empty());
    }

    /// A refusal the provider meant is not retried at all.
    #[test]
    fn a_considered_refusal_ends_the_turn() {
        let h = harness(
            "loop-refusal",
            vec![Step::Fail(LlmError::Http("http status 400: no such model".to_string()))],
        );

        let err = h
            .run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![]))
            .expect_err("fails");

        assert!(matches!(err, TurnError::Provider(_)), "{err}");
        assert_eq!(h.provider.requests().len(), 1);
    }

    #[test]
    fn a_stop_during_a_retry_wait_takes_effect_inside_it() {
        let h = harness("loop-retry-cancel", vec![Step::Fail(rate_limited(60))]);
        // Past the round's own checkpoints, so the stop lands in the wait.
        h.cancel_at_poll(4);

        let outcome = h.run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![]));

        assert!(matches!(outcome.expect("stops"), ChatStreamOutcome::Cancelled(_)));
        assert!(
            h.slept.lock().unwrap().iter().sum::<Duration>() < Duration::from_secs(60),
            "the stop was not made to wait out the whole window"
        );
    }

    // -------------------------------------------------------------- ceilings

    /// A model that never stops asking for tools must not hold the turn open
    /// forever.
    #[test]
    fn a_turn_that_never_finishes_is_cut_off() {
        let steps = (0..MAX_TOOL_ITERATIONS + 1)
            .map(|i| {
                asks(vec![wants(
                    &format!("c{i}"),
                    "createDirectory",
                    &format!(r#"{{"path":"d{i}"}}"#),
                )])
            })
            .collect();
        let h = harness("loop-ceiling", steps);

        let err = h
            .run(|turn| stream(turn, vec![LlmMessage::user("loop forever")], vec![]))
            .expect_err("is cut off");

        assert!(matches!(err, TurnError::Exhausted { .. }), "{err}");
        assert!(h.provider.requests().len() <= MAX_TOOL_ITERATIONS);
    }

    // ------------------------------------------------------- what the model reads

    /// A call refused before it runs still has to be reported and answered,
    /// or the model is left waiting for a result that never comes.
    #[test]
    fn a_call_refused_by_the_preflight_is_reported_as_a_tool_error() {
        let h = harness(
            "loop-preflight",
            vec![
                asks(vec![wants(
                    "w1",
                    "writeFile",
                    r#"{"path":"../outside.rs","content":"x"}"#,
                )]),
                text("understood"),
            ],
        );

        h.run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![])).expect("finishes");

        let told = tool_contents(h.provider.requests().last().unwrap());
        assert_eq!(told.len(), 1);
        assert!(told[0].starts_with("Error:"), "{}", told[0]);
        let events = payloads(&h.events());
        assert!(events.contains(&"toolCall:w1".to_string()));
        assert!(events.contains(&"toolResult:w1".to_string()));
    }

    /// A listing goes to the model as a tree: a flat array of paths makes it
    /// rebuild the directory structure from N separate strings.
    #[test]
    fn a_listing_reaches_the_model_as_a_tree() {
        let h = harness(
            "loop-listing",
            vec![asks(vec![wants("l1", "listFiles", "{}")]), text("seen")],
        );
        std::fs::create_dir(h.root.join("src")).unwrap();
        std::fs::write(h.root.join("src/main.rs"), "fn main() {}").unwrap();

        h.run(|turn| stream(turn, vec![LlmMessage::user("what is here")], vec![]))
            .expect("finishes");

        let told = tool_contents(h.provider.requests().last().unwrap());
        assert!(told[0].contains("main.rs"), "{}", told[0]);
        assert!(!told[0].contains("\"isDir\""), "raw JSON, not a tree: {}", told[0]);
    }

    /// Token usage is reported once per round, and it is the whole context —
    /// every request resends the history.
    #[test]
    fn usage_is_reported_for_the_round_that_produced_it() {
        let h = harness(
            "loop-usage",
            vec![Step::Reply(ChatStreamResult {
                text: "done".to_string(),
                usage: Some(ChatUsage {
                    prompt_tokens: 100,
                    completion_tokens: 7,
                    total_tokens: 107,
                }),
                ..Default::default()
            })],
        );

        h.run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![])).expect("finishes");

        assert!(payloads(&h.events()).contains(&"contextUsage".to_string()));
    }

    /// The model is offered the tools this build actually has, every round.
    #[test]
    fn every_request_carries_the_tool_schemas() {
        let h = harness("loop-tools", vec![text("hi")]);

        h.run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![])).expect("finishes");

        let requests = h.provider.requests();
        let offered: &[LlmToolDefinition] = &requests[0].tools;
        assert_eq!(offered.len(), ToolName::ALL.len());
    }

    // ------------------------------------------------------------- steering

    fn user_messages(request: &ChatRequest) -> Vec<String> {
        request
            .messages
            .iter()
            .filter(|m| m.role == LlmRole::User)
            .filter_map(|m| m.content.clone())
            .collect()
    }

    /// The point of steering: what the user typed mid-turn reaches the model
    /// on the next round, marked as a clarification rather than a new task.
    #[test]
    fn a_note_typed_mid_turn_reaches_the_next_round() {
        let h = harness(
            "steer-applied",
            vec![
                asks_while_typing(
                    vec![wants("l1", "listFiles", "{}")],
                    "use the existing helper",
                ),
                text("understood"),
            ],
        );

        h.run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![])).expect("finishes");

        let second = user_messages(&h.provider.requests()[1]);
        assert!(
            second.iter().any(|m| m.contains("use the existing helper")),
            "{second:?}"
        );
        assert!(
            second.iter().any(|m| m.starts_with(STEERING_PREFIX)),
            "a clarification, not a new task: {second:?}"
        );
        let applied: Vec<String> = payloads(&h.events())
            .into_iter()
            .filter(|p| p.starts_with("steering:"))
            .collect();
        assert_eq!(applied.len(), 1, "the note is announced by id: {applied:?}");
    }

    /// Ending the turn here would silently drop what the user typed, with
    /// nothing to tell them it was never seen.
    #[test]
    fn a_note_that_arrives_as_the_model_finishes_keeps_the_turn_going() {
        let h = harness(
            "steer-late",
            vec![
                text_while_typing("all done", "and rename it too"),
                text("also did the other thing"),
            ],
        );

        let outcome = h.run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![]));

        let ChatStreamOutcome::Done(done) = outcome.expect("finishes") else {
            panic!("expected Done");
        };
        assert_eq!(done.result.text, "also did the other thing");
        assert_eq!(h.provider.requests().len(), 2, "the turn ran another round");
        let second = user_messages(&h.provider.requests()[1]);
        assert!(second.iter().any(|m| m.contains("and rename it too")), "{second:?}");
        // The answer the model had already given stays in the conversation,
        // or the extra round reads as if it never spoke.
        assert!(h.provider.requests()[1]
            .messages
            .iter()
            .any(|m| m.role == LlmRole::Assistant && m.content.as_deref() == Some("all done")));
    }

    /// A note is handed over once. A round that is retried, or a turn that
    /// pauses in between, must not deliver it a second time.
    #[test]
    fn a_note_is_delivered_once() {
        let h = harness(
            "steer-once",
            vec![
                asks_while_typing(vec![wants("l1", "listFiles", "{}")], "watch the indentation"),
                asks(vec![wants("l2", "listFiles", "{}")]),
                text("done"),
            ],
        );

        h.run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![])).expect("finishes");

        let delivered = user_messages(h.provider.requests().last().unwrap())
            .iter()
            .filter(|m| m.contains("watch the indentation"))
            .count();
        assert_eq!(delivered, 1);
    }

    /// The user typed it at a conversation that had already finished. It is
    /// their next message, not a clarification of a turn they cannot see.
    #[test]
    fn a_note_left_over_from_a_finished_turn_does_not_leak_into_the_next() {
        let h = harness("steer-leftover", vec![text("hi")]);
        h.steering.push(SteeringNote::user("from the previous turn"));

        h.run(|turn| stream(turn, vec![LlmMessage::user("a new question")], vec![]))
            .expect("finishes");

        let sent = user_messages(&h.provider.requests()[0]);
        assert_eq!(sent, ["a new question"]);
    }

    #[test]
    fn a_queued_note_can_be_cancelled_until_a_round_takes_it() {
        let queue = SteeringQueue::default();
        let note = SteeringNote::user("never mind");
        let id = note.id.clone();
        queue.push(note);

        assert!(queue.cancel(&id), "still queued");
        assert!(queue.take().is_empty());
        // Cancelling what a round already picked up answers `false`: what has
        // been said to the model cannot be unsaid.
        assert!(!queue.cancel(&id));
    }

    /// Two identical clarifications are indistinguishable by text, which is
    /// why cancelling works by id.
    #[test]
    fn identical_notes_are_still_separate() {
        let queue = SteeringQueue::default();
        let first = SteeringNote::user("check the locale");
        let second = SteeringNote::user("check the locale");
        assert_ne!(first.id, second.id);
        let first_id = first.id.clone();
        queue.push(first);
        queue.push(second);

        assert!(queue.cancel(&first_id));
        assert_eq!(queue.take().len(), 1);
    }

    // --------------------------------------------------- the stage's own bar

    /// What stage one set out to produce: a turn that reads a file, changes
    /// it, and hands the model back what actually landed on disk — not just
    /// `{"path": "…"}`, which tells it nothing about whether the edit took.
    #[test]
    fn a_turn_reads_a_file_edits_it_and_is_told_what_changed() {
        let h = harness(
            "stage-one",
            vec![
                asks(vec![wants("r1", "readFile", r#"{"path":"lib.rs"}"#)]),
                asks(vec![wants(
                    "e1",
                    "editFile",
                    r#"{"path":"lib.rs","edits":[{"old":"one","new":"two"}]}"#,
                )]),
                text("renamed it"),
            ],
        );
        std::fs::write(h.root.join("lib.rs"), "fn one() {}\n").unwrap();

        let outcome = h.run(|turn| stream(turn, vec![LlmMessage::user("rename one to two")], vec![]));

        assert!(matches!(outcome.expect("finishes"), ChatStreamOutcome::Done(_)));
        assert_eq!(
            std::fs::read_to_string(h.root.join("lib.rs")).unwrap(),
            "fn two() {}\n"
        );
        let told = tool_contents(h.provider.requests().last().unwrap());
        let edit = told.last().expect("the edit was reported");
        assert!(edit.contains("linesAdded"), "no diff stats: {edit}");
        assert!(edit.contains("-fn one"), "no diff itself: {edit}");
    }

    /// Every field of the checkpoint, exercised through a real pause rather
    /// than by serializing a fixture: the checklist an earlier round built has
    /// to come out the other side, along with the history, the ceilings, the
    /// numbering and the read registry.
    #[test]
    fn the_whole_checkpoint_survives_a_real_pause() {
        let mut h = harness(
            "stage-one-checkpoint",
            vec![
                asks(vec![wants(
                    "t1",
                    "todo",
                    r#"{"op":"write","tasks":["read it","rewrite it"]}"#,
                )]),
                asks(vec![wants("r1", "readFile", r#"{"path":"a.rs"}"#)]),
                asks(vec![wants(
                    "w1",
                    "writeFile",
                    r#"{"path":"a.rs","content":"new"}"#,
                )]),
                text("done"),
            ],
        );
        h.approval = asking();
        std::fs::write(h.root.join("a.rs"), "old").unwrap();

        let ChatStreamOutcome::PendingApproval(pending) = h
            .run(|turn| stream(turn, vec![LlmMessage::user("go")], vec![]))
            .expect("pauses")
        else {
            panic!("expected a pause");
        };

        assert_eq!(pending.round, 3, "the ceiling cannot be reset by pausing");
        assert!(pending.budget_used > 0);
        assert_eq!(pending.event_seq, h.events().last().unwrap().seq);
        assert_eq!(pending.calls.len(), 1);
        assert_eq!(
            pending.todos.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
            ["read it", "rewrite it"],
            "the checklist an earlier round built"
        );
        assert!(pending.history.len() > 1);
        assert!(pending.reads.check("a.rs", "old", true).is_ok());

        let outcome = h.run(|turn| {
            resume(
                turn,
                pending,
                vec![ToolCallDecision {
                    id: "w1".to_string(),
                    approved: true,
                    reason: None,
                }],
            )
        });

        let ChatStreamOutcome::Done(done) = outcome.expect("finishes") else {
            panic!("expected Done");
        };
        assert_eq!(std::fs::read_to_string(h.root.join("a.rs")).unwrap(), "new");
        assert_eq!(done.todos.len(), 2, "and the checklist comes out with it");
    }
}

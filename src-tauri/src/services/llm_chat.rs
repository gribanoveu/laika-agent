//! The agent loop and the small decisions a round makes on its own.
//!
//! This file grows in three steps (`docs/06-port-plan.md`, F-1.14): the
//! round-level rules below, then the loop that uses them, then steering. Each
//! rule here is pure and takes no provider, which is what makes it testable
//! without a model at the other end.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::domain::llm::LlmToolCall;
use crate::domain::tools::{ToolName, ToolResult};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}

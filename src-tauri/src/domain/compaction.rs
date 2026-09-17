//! When a conversation no longer fits, and what to fold away.
//!
//! The rules only — no summarizing happens here, and nothing is called. The
//! summary itself costs a request to the model, which belongs to a service;
//! what belongs here is the arithmetic that decides whether that request is
//! worth making and which messages it is about.
//!
//! Ported from Alfa Atlas's `src/lib/contextCompaction.ts`, which lived in the
//! frontend. Two things changed in the move, both because the history here is
//! the wire format rather than a transcript of blocks:
//!
//! * **No summary cache.** Upstream kept the whole conversation and rebuilt
//!   the wire history on every turn, so it cached the last summary to avoid
//!   re-summarizing from scratch. Here the compacted history *is* the history
//!   from then on — what the reader sees is a separate list of blocks — so the
//!   previous summary is simply the message the next pass starts from.
//! * **A cut cannot fall anywhere.** A tool result is a message that only
//!   makes sense after the assistant message that asked for it; a provider
//!   rejects a history whose first message is an answer to a question that is
//!   no longer there. Upstream had no such rule to break, because its
//!   messages carried their tool calls inside them.

use super::llm::{LlmMessage, LlmRole, LlmToolDefinition};

/// Compaction starts once the estimate crosses this much of the context
/// window. Early enough that the rest of the turn — a tool-calling loop can
/// add a lot in one round — still fits, and early enough to absorb what the
/// estimate below is known to miss.
///
/// A percentage rather than a ratio because the comparison is then exact:
/// `10_000 * 0.8` is not 8_000 in either float width, so a threshold written
/// that way fires one token later than it reads, and says so only under a
/// test that lands exactly on it.
pub const TRIGGER_PERCENT: u64 = 80;

/// How many recent messages stay verbatim. The summary is for what the
/// conversation was about; the tail is for what it is doing right now, and
/// that part has to survive word for word.
pub const KEEP_LAST_MESSAGES: usize = 12;

/// The tail after a real overflow, rather than a predicted one: the provider
/// has already refused, so a pass that compacts as gently as the one that
/// failed to prevent it would just fail again.
pub const RETRY_KEEP_LAST_MESSAGES: usize = 6;

/// Below this, compaction never runs. Summarizing a short conversation trades
/// a request and some fidelity for almost no room.
pub const MIN_MESSAGES: usize = KEEP_LAST_MESSAGES + 6;

/// What the summary message says it is. The model is told plainly that it is
/// reading a summary rather than a transcript — a summary presented as the
/// real conversation invites it to quote things nobody said.
pub const SUMMARY_PREFIX: &str = "[Compacted summary of earlier conversation]";

/// Around every tool call: its id, the `{"type":"function"…}` envelope, and
/// the `role`/`tool_call_id` of the message that answers it. Small on its own,
/// and there are dozens in a working turn.
const TOOL_CALL_OVERHEAD_CHARS: usize = 60;

/// Around every message: the role, the braces, the commas.
const MESSAGE_OVERHEAD_TOKENS: usize = 4;

/// The rule of thumb, and the reason this is called an estimate.
const CHARS_PER_TOKEN: usize = 4;

/// What the next request will cost, near enough to decide with.
///
/// Four characters to a token is the English rule of thumb. Cyrillic packs
/// more tokens into the same characters, so this **underestimates** exactly
/// where a long conversation is most likely to be — which is why the trigger
/// ratio leaves room rather than sitting at the edge.
pub fn estimate_tokens(messages: &[LlmMessage]) -> usize {
    messages.iter().map(estimate_message_tokens).sum()
}

/// The same rule of thumb, over a bare string.
///
/// Public because the two largest things in a request are not messages: the
/// system prompt and the tool schemas.
pub fn estimate_text_tokens(text: &str) -> usize {
    text.len().div_ceil(CHARS_PER_TOKEN)
}

/// What the tool schemas cost, on **every** request.
///
/// They are not messages and not part of the system prompt, so nothing else in
/// this file sees them — and they are not small. Upstream measured its 24
/// advertised tools at ~37 800 characters, about 9 500 tokens, resent verbatim
/// with every request; leaving them out had the estimate running a stable ~36%
/// under the provider's own `promptTokens`.
///
/// Serialized rather than stored as a number: the descriptions are edited
/// where the tools live, and a constant here would be wrong the first time one
/// of them grew a paragraph. Slightly under the wire form, which wraps each
/// entry in `{"type":"function","function":{…}}` — about 40 characters a tool,
/// inside the noise of the estimate itself.
pub fn estimate_tool_schema_tokens(tools: &[LlmToolDefinition]) -> usize {
    if tools.is_empty() {
        return 0;
    }
    match serde_json::to_string(tools) {
        Ok(json) => estimate_text_tokens(&json),
        // An estimate is allowed to be approximate; it is not allowed to take
        // down the turn it is estimating.
        Err(_) => 0,
    }
}

fn estimate_message_tokens(message: &LlmMessage) -> usize {
    let mut chars = message.content.as_deref().map_or(0, str::len);
    chars += message.tool_call_id.as_deref().map_or(0, str::len);
    for call in &message.tool_calls {
        chars += call.name.len() + call.arguments.len() + TOOL_CALL_OVERHEAD_CHARS;
    }
    MESSAGE_OVERHEAD_TOKENS + chars.div_ceil(CHARS_PER_TOKEN)
}

/// Whether a pass is worth making now.
///
/// No configured limit means no: the app talks to gateways it knows nothing
/// about, and compacting against a guessed window would throw away
/// conversation to solve a problem that may not exist.
pub fn should_compact(estimated_tokens: usize, context_limit: Option<u32>, messages: &[LlmMessage]) -> bool {
    let Some(limit) = context_limit.filter(|limit| *limit > 0) else {
        return false;
    };
    if messages.len() < MIN_MESSAGES {
        return false;
    }
    estimated_tokens as u64 * 100 >= u64::from(limit) * TRIGGER_PERCENT
}

/// One pass, as positions in the history: `[..keep_head]` stays, the
/// `summarize` messages after it become one summary, and the rest is kept
/// word for word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionPlan {
    /// Leading system messages. The instructions are not conversation and
    /// summarizing them would quietly rewrite how the agent behaves.
    pub keep_head: usize,
    pub summarize: usize,
}

impl CompactionPlan {
    /// Where the verbatim tail begins.
    pub fn tail_start(&self) -> usize {
        self.keep_head + self.summarize
    }
}

/// What to fold away, or `None` when there is nothing worth folding.
///
/// The cut is pulled back from the recency floor until it is a place a
/// history can legally be cut: never between an assistant message asking for
/// tool calls and the messages answering them. A provider refuses a history
/// that opens with an answer to a question it cannot see, so a cut in the
/// middle of a round does not shorten the conversation — it breaks it.
pub fn plan_compaction(messages: &[LlmMessage], keep_last: usize) -> Option<CompactionPlan> {
    let keep_head = messages
        .iter()
        .take_while(|message| message.role == LlmRole::System)
        .count();

    let floor = messages.len().saturating_sub(keep_last).max(keep_head);
    let cut = safe_cut(messages, floor, keep_head)?;

    let summarize = cut - keep_head;
    (summarize > 0).then_some(CompactionPlan {
        keep_head,
        summarize,
    })
}

/// The first legal cut at or before `from`, or `None` if there is none above
/// `floor` — a single round of tool calls longer than the whole tail, which
/// is a history to leave alone rather than to break.
fn safe_cut(messages: &[LlmMessage], from: usize, floor: usize) -> Option<usize> {
    let mut cut = from;
    while cut > floor && messages.get(cut).is_some_and(|m| m.role == LlmRole::Tool) {
        cut -= 1;
    }
    (!messages.get(cut).is_some_and(|m| m.role == LlmRole::Tool)).then_some(cut)
}

/// The history as it will be sent from now on. The summary goes in as the
/// user's own recap rather than as a system instruction: it is a record of
/// what was said, and a model that reads it as an instruction starts
/// following the old conversation again instead of continuing it.
pub fn apply(messages: &[LlmMessage], plan: CompactionPlan, summary: &str) -> Vec<LlmMessage> {
    let mut compacted = messages[..plan.keep_head].to_vec();
    compacted.push(summary_message(summary));
    compacted.extend_from_slice(&messages[plan.tail_start()..]);
    compacted
}

pub fn summary_message(summary: &str) -> LlmMessage {
    LlmMessage::user(format!("{SUMMARY_PREFIX}\n\n{summary}"))
}

/// How the provider says "this conversation no longer fits".
///
/// Every one of these is someone's prose, which is why this is a list of
/// phrases rather than a status code: the OpenAI-compatible protocol has no
/// code for it, and each gateway phrases it its own way. Matching too
/// eagerly is the expensive mistake — an unrelated failure answered with a
/// summarizing request costs money and loses history — so these are phrases
/// specific enough that nothing else produces them.
const CONTEXT_LENGTH_PHRASES: [&str; 6] = [
    "context length",
    "context_length",
    "context window",
    "too many tokens",
    "prompt is too long",
    "maximum context",
];

pub fn is_context_length_error(message: &str) -> bool {
    let message = message.to_lowercase();
    CONTEXT_LENGTH_PHRASES
        .iter()
        .any(|phrase| message.contains(phrase))
}

/// What the summarizer is asked to do. Written for the next model reading
/// its own summary, not for a person: what matters is what would otherwise
/// have to be asked again.
pub const SUMMARY_INSTRUCTIONS: &str = "\
You are compacting the earlier part of a conversation between a user and a \
coding agent so it can continue in less context. Write a summary in English \
covering: what the user is trying to achieve, decisions already made and \
why, files touched by path and what changed in them, what has been tried \
and failed, and anything the agent must not forget to do. Be specific — \
names, paths, error messages. Do not add advice, do not speculate, and do \
not describe the conversation ('the user asked…'); write the state of the \
work. Plain prose and short lists only.";

/// The longest any one message is rendered at for the summarizer.
///
/// A file read is thirty thousand characters. Sending those verbatim to be
/// summarized would send the very context that just overflowed — the request
/// that is meant to make room would be the largest one of the session. What
/// a summary needs from a tool result is that it happened and roughly what
/// came back, and that survives truncation.
const MAX_RENDERED_CHARS: usize = 1_500;

/// The messages to be folded away, as text for the summarizer.
pub fn render_for_summary(messages: &[LlmMessage]) -> String {
    let mut out = String::new();
    for message in messages {
        let line = match message.role {
            LlmRole::System => continue,
            LlmRole::User => format!("User: {}", content_of(message)),
            LlmRole::Tool => format!("  [result] {}", content_of(message)),
            LlmRole::Assistant => {
                let mut parts = Vec::new();
                let text = content_of(message);
                if !text.is_empty() {
                    parts.push(format!("Assistant: {text}"));
                }
                for call in &message.tool_calls {
                    // The arguments, not only the name: the summary is asked
                    // for the files that were touched, and `writeFile` alone
                    // names none of them.
                    parts.push(format!("  [tool] {} {}", call.name, truncate(&call.arguments)));
                }
                parts.join("\n")
            }
        };
        if line.is_empty() {
            continue;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn content_of(message: &LlmMessage) -> String {
    truncate(message.content.as_deref().unwrap_or(""))
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_RENDERED_CHARS {
        return text.to_string();
    }
    let kept: String = text.chars().take(MAX_RENDERED_CHARS).collect();
    format!("{kept}… [{} characters omitted]", text.chars().count() - MAX_RENDERED_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::llm::LlmToolCall;

    fn user(text: &str) -> LlmMessage {
        LlmMessage::user(text)
    }

    fn definition(name: &str, description: &str) -> LlmToolDefinition {
        LlmToolDefinition {
            name: name.to_string(),
            description: description.to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }
    }

    /// No tools advertised is no cost — not a floor, and not the JSON of an
    /// empty array.
    #[test]
    fn no_schemas_cost_nothing() {
        assert_eq!(estimate_tool_schema_tokens(&[]), 0);
    }

    /// The description is the bulk of a schema and the part that grows: a
    /// count that only saw the names would stay flat while the real cost
    /// doubled.
    #[test]
    fn a_schema_costs_what_its_description_costs() {
        let short = [definition("readFile", "Reads a file.")];
        let long = [definition("readFile", &"Reads a file. ".repeat(100))];

        let grown = estimate_tool_schema_tokens(&long) - estimate_tool_schema_tokens(&short);
        assert!(
            grown > 300,
            "a description 1 400 characters longer added {grown} tokens"
        );
    }

    #[test]
    fn every_advertised_tool_is_paid_for() {
        let one = [definition("readFile", "Reads a file.")];
        let two = [
            definition("readFile", "Reads a file."),
            definition("grep", "Searches for a pattern."),
        ];

        assert!(estimate_tool_schema_tokens(&two) > estimate_tool_schema_tokens(&one));
    }

    fn assistant(text: &str) -> LlmMessage {
        LlmMessage::assistant(text)
    }

    fn calling(id: &str) -> LlmMessage {
        LlmMessage {
            role: LlmRole::Assistant,
            content: None,
            tool_call_id: None,
            tool_calls: vec![LlmToolCall {
                id: id.to_string(),
                name: "readFile".to_string(),
                arguments: r#"{"path":"a.rs"}"#.to_string(),
            }],
        }
    }

    fn answered(id: &str) -> LlmMessage {
        LlmMessage {
            role: LlmRole::Tool,
            content: Some("fn main() {}".to_string()),
            tool_call_id: Some(id.to_string()),
            tool_calls: Vec::new(),
        }
    }

    fn conversation(len: usize) -> Vec<LlmMessage> {
        (0..len)
            .map(|i| {
                if i % 2 == 0 {
                    user(&format!("question {i}"))
                } else {
                    assistant(&format!("answer {i}"))
                }
            })
            .collect()
    }

    #[test]
    fn an_empty_conversation_costs_nothing() {
        assert_eq!(estimate_tokens(&[]), 0);
    }

    #[test]
    fn what_a_tool_call_carries_is_counted_too() {
        let prose = estimate_tokens(&[assistant("")]);
        let call = estimate_tokens(&[calling("c1")]);

        // The arguments and the name are in the request whether or not the
        // message has any prose in it.
        assert!(call > prose, "{call} is not more than {prose}");
    }

    #[test]
    fn a_tool_result_costs_what_it_says() {
        let short = estimate_tokens(&[answered("c1")]);
        let long = estimate_tokens(&[LlmMessage {
            content: Some("x".repeat(4_000)),
            ..answered("c1")
        }]);

        // Four thousand characters, four characters to a token.
        assert!((950..=1_050).contains(&(long - short)), "{long} vs {short}");
    }

    #[test]
    fn nothing_is_compacted_without_a_known_window() {
        let messages = conversation(MIN_MESSAGES);
        assert!(!should_compact(1_000_000, None, &messages));
        assert!(!should_compact(1_000_000, Some(0), &messages));
    }

    /// Summarizing four messages buys nothing and costs a request right as
    /// the user is trying to say something.
    #[test]
    fn a_short_conversation_is_left_alone_however_big_it_is() {
        let messages = conversation(MIN_MESSAGES - 1);
        assert!(!should_compact(1_000_000, Some(8_000), &messages));
    }

    #[test]
    fn compaction_starts_before_the_window_is_full() {
        let messages = conversation(MIN_MESSAGES);
        let limit = 10_000;

        assert!(!should_compact(7_999, Some(limit), &messages));
        assert!(should_compact(8_000, Some(limit), &messages));
    }

    #[test]
    fn the_last_messages_stay_word_for_word() {
        let messages = conversation(30);
        let plan = plan_compaction(&messages, KEEP_LAST_MESSAGES).expect("something to fold");

        assert_eq!(plan.keep_head, 0);
        assert_eq!(plan.summarize, 30 - KEEP_LAST_MESSAGES);
        assert_eq!(messages.len() - plan.tail_start(), KEEP_LAST_MESSAGES);
    }

    /// The instructions are not conversation: folding them into a summary
    /// rewrites how the agent behaves, quietly and permanently.
    #[test]
    fn the_system_prompt_is_never_summarized() {
        let mut messages = vec![LlmMessage::system("you are an agent")];
        messages.extend(conversation(30));

        let plan = plan_compaction(&messages, KEEP_LAST_MESSAGES).expect("something to fold");
        assert_eq!(plan.keep_head, 1);

        let compacted = apply(&messages, plan, "they talked about the parser");
        assert_eq!(compacted[0], LlmMessage::system("you are an agent"));
        assert!(compacted[1].content.as_deref().unwrap().starts_with(SUMMARY_PREFIX));
    }

    #[test]
    fn a_conversation_with_nothing_to_fold_is_left_alone() {
        assert_eq!(plan_compaction(&conversation(KEEP_LAST_MESSAGES), KEEP_LAST_MESSAGES), None);
        assert_eq!(plan_compaction(&[], KEEP_LAST_MESSAGES), None);
    }

    /// The rule the wire format adds: an answer whose question has been
    /// summarized away is a message the provider refuses outright.
    #[test]
    fn a_cut_never_separates_a_tool_call_from_its_answer() {
        // The floor lands inside the round: 20 messages, keep 3, so the cut
        // would fall on the second result.
        let mut messages = conversation(16);
        messages.push(calling("c1"));
        messages.push(answered("c1"));
        messages.push(answered("c1b"));
        messages.push(user("and now?"));

        let plan = plan_compaction(&messages, 3).expect("something to fold");
        let tail = &messages[plan.tail_start()..];

        assert_eq!(tail[0], calling("c1"), "the tail opens with an orphaned result");
        assert!(!tail.is_empty());
    }

    /// Where every message of the tail is part of one unfinished round,
    /// there is no legal cut. Leaving the history long is the lesser harm:
    /// the illegal one is refused by the provider outright.
    #[test]
    fn a_round_too_long_to_cut_is_left_whole() {
        let mut messages = vec![calling("c1")];
        messages.extend((0..30).map(|_| answered("c1")));

        assert_eq!(plan_compaction(&messages, 2), None);
    }

    #[test]
    fn the_compacted_history_is_the_head_the_summary_and_the_tail() {
        let messages = conversation(30);
        let plan = plan_compaction(&messages, KEEP_LAST_MESSAGES).unwrap();

        let compacted = apply(&messages, plan, "they talked about the parser");

        assert_eq!(compacted.len(), 1 + KEEP_LAST_MESSAGES);
        assert_eq!(compacted[0], summary_message("they talked about the parser"));
        assert_eq!(&compacted[1..], &messages[plan.tail_start()..]);
    }

    /// Two passes in a row: the second one folds the first one's summary in
    /// with everything since, which is what replaces the cache upstream kept.
    #[test]
    fn a_second_pass_starts_from_the_first_ones_summary() {
        let messages = conversation(40);
        let first = apply(
            &messages,
            plan_compaction(&messages, KEEP_LAST_MESSAGES).unwrap(),
            "the parser",
        );
        let mut grown = first.clone();
        grown.extend(conversation(20));

        let plan = plan_compaction(&grown, KEEP_LAST_MESSAGES).expect("something to fold");
        assert_eq!(plan.keep_head, 0, "the summary is a message like any other");
        assert!(plan.summarize >= 1);

        let second = apply(&grown, plan, "the parser and the lexer");
        assert_eq!(second.iter().filter(|m| is_summary(m)).count(), 1);
    }

    #[test]
    fn a_provider_saying_the_conversation_is_too_long_is_recognised() {
        for message in [
            "http status 400: This model's maximum context length is 8192 tokens",
            "provider error: context_length_exceeded",
            "Error: prompt is too long: 210000 tokens > 200000 maximum",
            "input exceeds the context window of this model",
            "too many tokens in the request",
        ] {
            assert!(is_context_length_error(message), "missed {message:?}");
        }
    }

    /// The expensive mistake is the other one: answering an unrelated failure
    /// with a summarizing request costs a call and folds away history for
    /// nothing.
    #[test]
    fn and_other_failures_are_not_mistaken_for_it() {
        for message in [
            "http status 401: invalid api key",
            "rate limited by the provider",
            "connection closed before message completed",
            "model not found: qwen3",
            "context deadline exceeded",
        ] {
            assert!(!is_context_length_error(message), "matched {message:?}");
        }
    }

    #[test]
    fn what_the_summarizer_reads_names_the_files_that_were_touched() {
        let messages = vec![
            user("fix the parser"),
            calling("c1"),
            answered("c1"),
            assistant("done"),
        ];

        let rendered = render_for_summary(&messages);
        assert!(rendered.contains("User: fix the parser"), "{rendered}");
        assert!(rendered.contains("[tool] readFile"), "{rendered}");
        assert!(rendered.contains("a.rs"), "{rendered}");
        assert!(rendered.contains("Assistant: done"), "{rendered}");
    }

    /// The request that is supposed to make room must not be the largest one
    /// of the session: a file read is thirty thousand characters, and there
    /// are dozens of them in what is being folded away.
    #[test]
    fn a_huge_tool_result_is_cut_down_before_it_is_summarized() {
        let huge = LlmMessage {
            content: Some("x".repeat(30_000)),
            ..answered("c1")
        };

        let rendered = render_for_summary(&[huge]);
        assert!(rendered.len() < 3_000, "{} characters", rendered.len());
        assert!(rendered.contains("characters omitted"), "{rendered}");
    }

    /// Cutting by characters on a multi-byte string is how this kind of code
    /// panics in production and nowhere else.
    #[test]
    fn cutting_a_long_message_does_not_split_a_character() {
        let cyrillic = LlmMessage {
            content: Some("я".repeat(30_000)),
            ..answered("c1")
        };

        let rendered = render_for_summary(&cyrillic_message(cyrillic));
        assert!(rendered.contains("characters omitted"), "{rendered}");
    }

    fn cyrillic_message(message: LlmMessage) -> Vec<LlmMessage> {
        vec![message]
    }

    #[test]
    fn the_system_prompt_is_not_part_of_what_is_summarized() {
        let rendered = render_for_summary(&[LlmMessage::system("you are an agent"), user("hi")]);
        assert!(!rendered.contains("you are an agent"), "{rendered}");
        assert!(rendered.contains("User: hi"), "{rendered}");
    }

    fn is_summary(message: &LlmMessage) -> bool {
        message
            .content
            .as_deref()
            .is_some_and(|text| text.starts_with(SUMMARY_PREFIX))
    }
}

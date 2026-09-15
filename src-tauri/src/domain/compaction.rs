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

use super::llm::{LlmMessage, LlmRole};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::llm::LlmToolCall;

    fn user(text: &str) -> LlmMessage {
        LlmMessage::user(text)
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

    fn is_summary(message: &LlmMessage) -> bool {
        message
            .content
            .as_deref()
            .is_some_and(|text| text.starts_with(SUMMARY_PREFIX))
    }
}

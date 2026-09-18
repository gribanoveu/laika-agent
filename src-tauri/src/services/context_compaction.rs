//! Folding the older part of a conversation into a summary, so the rest of it
//! still fits.
//!
//! The rules are in `domain::compaction`; what is here is the one thing they
//! cannot do, which is ask the model. Nothing decides here: the caller says
//! how much to keep and this either comes back with a shorter history or with
//! nothing at all.

use crate::domain::compaction::{self, SUMMARY_INSTRUCTIONS};
use crate::domain::llm::{ChatRequest, LlmError, LlmMessage};
use crate::domain::prompt;
use crate::infra::llm_debug_log;
use crate::services::ai_tools::tools::tool_definitions;
use crate::services::llm_session::LlmSession;

/// A conversation, shorter than it was.
#[derive(Debug, Clone, PartialEq)]
pub struct Compacted {
    pub history: Vec<LlmMessage>,
    /// How many messages the summary stands for — what the transcript says
    /// happened.
    pub folded: usize,
}

/// What every request pays before a single message of the conversation is
/// added to it.
///
/// Two things, and neither is a message, which is why the estimate could not
/// see them: the system prompt, sent in front of the history on every round,
/// and the tool schemas, sent beside it. Together they are the largest fixed
/// item in the request and they do not shrink when the conversation does — a
/// window that looks 60% free can be nearly full.
///
/// The prompt's varying half is deliberately left out. It is a date, a path,
/// a shell and a checklist: a few hundred characters against tens of
/// thousands, and counting it exactly would mean handing this function a
/// workspace and a todo list it otherwise has no use for.
pub fn fixed_request_tokens() -> usize {
    let usage = compaction::ContextUsage::new(
        compaction::estimate_text_tokens(prompt::INSTRUCTIONS),
        compaction::estimate_tool_schema_tokens(&tool_definitions()),
        0,
        None,
    );
    usage.total
}

/// What the next request will cost, as the window should show it.
///
/// The same arithmetic the threshold uses, handed outward rather than computed
/// a second time in TypeScript. It is an estimate and the meter says so by
/// being a meter — but it is the estimate that actually decides, so a reader
/// watching it fill is watching the thing that will fold their conversation.
pub fn usage(session: &LlmSession, history: &[LlmMessage]) -> compaction::ContextUsage {
    compaction::ContextUsage::new(
        compaction::estimate_text_tokens(prompt::INSTRUCTIONS),
        compaction::estimate_tool_schema_tokens(&tool_definitions()),
        compaction::estimate_tokens(history),
        session.context_limit,
    )
}

/// A pass before the conversation fails rather than after, when the session
/// knows how big the window is and the estimate says the next request is
/// close to filling it.
///
/// `force` is the user asking for it outright: the threshold is skipped, but
/// nothing else is — a conversation with nothing worth folding stays as it is
/// whoever asked.
pub fn compact_if_needed(
    session: &LlmSession,
    history: &[LlmMessage],
    force: bool,
) -> Result<Option<Compacted>, LlmError> {
    let needed = force
        || compaction::should_compact(
            fixed_request_tokens() + compaction::estimate_tokens(history),
            session.context_limit,
            history,
        );
    if !needed {
        return Ok(None);
    }
    compact(session, history, compaction::KEEP_LAST_MESSAGES)
}

/// One pass. `None` means the history is unchanged, for any reason: there was
/// nothing worth folding, or the summary came back empty.
///
/// Refusing rather than erroring, because of what the caller does with it. A
/// pass runs when the conversation has already failed to fit; if it cannot
/// help, the useful thing to report is the original failure, not a second one
/// about summarizing.
pub fn compact(
    session: &LlmSession,
    history: &[LlmMessage],
    keep_last: usize,
) -> Result<Option<Compacted>, LlmError> {
    let Some(plan) = compaction::plan_compaction(history, keep_last) else {
        return Ok(None);
    };

    let request = ChatRequest {
        messages: vec![
            LlmMessage::system(SUMMARY_INSTRUCTIONS),
            LlmMessage::user(compaction::render_for_summary(
                &history[plan.keep_head..plan.tail_start()],
            )),
        ],
        // No tools. The summarizer is asked for prose about work already done;
        // a tool call here would be the model trying to continue the
        // conversation it is supposed to be describing — and the schemas cost
        // context in the one request that exists to save it.
        tools: Vec::new(),
        model: session.model.clone(),
    };

    // Round zero: it belongs to no round of the turn, and a debug log missing
    // the request that changed the history is a log that cannot explain it.
    llm_debug_log::log_request(session.debug_logging, &session.provider_id, 0, &request);
    let response = session.provider.chat(request);
    llm_debug_log::log_response(session.debug_logging, &session.provider_id, 0, &response);

    let summary = response?.content.unwrap_or_default();
    // An empty summary is not a short history, it is a deleted one.
    if summary.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(Compacted {
        history: compaction::apply(history, plan, summary.trim()),
        folded: plan.summarize,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::compaction::{KEEP_LAST_MESSAGES, SUMMARY_PREFIX};
    use crate::domain::llm::{
        ChatResponse, ChatStreamResult, LlmModelInfo, LlmProvider, LlmToolCall,
    };
    use std::sync::{Arc, Mutex};

    struct Summarizer {
        answer: Result<Option<String>, LlmError>,
        asked: Mutex<Vec<ChatRequest>>,
    }

    impl Summarizer {
        fn saying(answer: &str) -> Arc<Self> {
            Arc::new(Self {
                answer: Ok(Some(answer.to_string())),
                asked: Mutex::new(Vec::new()),
            })
        }
    }

    impl LlmProvider for Summarizer {
        fn chat(&self, request: ChatRequest) -> Result<ChatResponse, LlmError> {
            self.asked.lock().unwrap().push(request);
            match &self.answer {
                Ok(content) => Ok(ChatResponse {
                    content: content.clone(),
                    tool_calls: Vec::new(),
                    usage: None,
                }),
                Err(error) => Err(LlmError::Message(error.to_string())),
            }
        }

        fn chat_stream(
            &self,
            _: ChatRequest,
            _: &dyn Fn(&str),
            _: &dyn Fn(&str),
            _: &dyn Fn(&str, &str, &str),
            _: &dyn Fn() -> bool,
        ) -> Result<ChatStreamResult, LlmError> {
            unreachable!("compaction never streams")
        }

        fn list_models(&self) -> Result<Vec<LlmModelInfo>, LlmError> {
            unreachable!("compaction never lists models")
        }
    }

    fn session(provider: Arc<Summarizer>) -> LlmSession {
        LlmSession {
            provider,
            provider_id: "test".to_string(),
            model: "m".to_string(),
            debug_logging: false,
            context_limit: None,
        }
    }

    fn session_with_window(provider: Arc<Summarizer>, tokens: u32) -> LlmSession {
        LlmSession {
            context_limit: Some(tokens),
            ..session(provider)
        }
    }

    fn conversation(len: usize) -> Vec<LlmMessage> {
        (0..len)
            .map(|i| {
                if i % 2 == 0 {
                    LlmMessage::user(format!("question {i}"))
                } else {
                    LlmMessage::assistant(format!("answer {i}"))
                }
            })
            .collect()
    }

    #[test]
    fn the_old_part_becomes_one_message_and_the_rest_is_untouched() {
        let provider = Summarizer::saying("they were fixing the parser");
        let history = conversation(40);

        let compacted = compact(&session(provider), &history, KEEP_LAST_MESSAGES)
            .unwrap()
            .expect("a shorter history");

        assert_eq!(compacted.folded, 40 - KEEP_LAST_MESSAGES);
        assert_eq!(compacted.history.len(), 1 + KEEP_LAST_MESSAGES);
        assert!(compacted.history[0]
            .content
            .as_deref()
            .unwrap()
            .starts_with(SUMMARY_PREFIX));
        assert_eq!(&compacted.history[1..], &history[40 - KEEP_LAST_MESSAGES..]);
    }

    /// The request meant to save context must not carry the tool schemas —
    /// and a summarizer with tools tries to use them.
    #[test]
    fn the_summarizer_is_asked_for_prose_and_nothing_else() {
        let provider = Summarizer::saying("a summary");
        compact(&session(provider.clone()), &conversation(40), KEEP_LAST_MESSAGES).unwrap();

        let asked = provider.asked.lock().unwrap();
        assert_eq!(asked.len(), 1);
        assert!(asked[0].tools.is_empty());
        assert_eq!(asked[0].messages.len(), 2, "instructions, then the transcript");
        assert!(asked[0].messages[1]
            .content
            .as_deref()
            .unwrap()
            .contains("question 0"));
    }

    /// Replacing half a conversation with nothing is worse than leaving it
    /// long.
    #[test]
    fn an_empty_summary_changes_nothing() {
        let provider = Arc::new(Summarizer {
            answer: Ok(Some("   ".to_string())),
            asked: Mutex::new(Vec::new()),
        });

        assert_eq!(
            compact(&session(provider), &conversation(40), KEEP_LAST_MESSAGES).unwrap(),
            None
        );
    }

    #[test]
    fn a_summary_that_never_arrived_changes_nothing_either() {
        let provider = Arc::new(Summarizer {
            answer: Ok(None),
            asked: Mutex::new(Vec::new()),
        });

        assert_eq!(
            compact(&session(provider), &conversation(40), KEEP_LAST_MESSAGES).unwrap(),
            None
        );
    }

    /// Nothing to fold: the conversation is short, and whatever overflowed
    /// the window is not the history's length.
    #[test]
    fn a_short_conversation_is_not_worth_a_request() {
        let provider = Summarizer::saying("a summary");
        assert_eq!(
            compact(&session(provider.clone()), &conversation(4), KEEP_LAST_MESSAGES).unwrap(),
            None
        );
        assert!(provider.asked.lock().unwrap().is_empty(), "asked anyway");
    }

    #[test]
    fn a_failed_summary_is_reported_rather_than_guessed_at() {
        let provider = Arc::new(Summarizer {
            answer: Err(LlmError::Http("http status 500".to_string())),
            asked: Mutex::new(Vec::new()),
        });

        assert!(compact(&session(provider), &conversation(40), KEEP_LAST_MESSAGES).is_err());
    }

    /// The app talks to gateways it knows nothing about. Compacting against
    /// a guessed window would throw away conversation to solve a problem that
    /// may not exist.
    #[test]
    fn without_a_known_window_nothing_happens_on_its_own() {
        let provider = Summarizer::saying("a summary");
        let long: Vec<LlmMessage> = (0..40)
            .map(|i| LlmMessage::user(format!("{i} {}", "x".repeat(4_000))))
            .collect();

        assert_eq!(compact_if_needed(&session(provider.clone()), &long, false).unwrap(), None);
        assert!(provider.asked.lock().unwrap().is_empty());
    }

    #[test]
    fn a_conversation_filling_its_window_is_folded_before_it_fails() {
        let provider = Summarizer::saying("a summary");
        let long: Vec<LlmMessage> = (0..40)
            .map(|i| LlmMessage::user(format!("{i} {}", "x".repeat(4_000))))
            .collect();

        let compacted = compact_if_needed(&session_with_window(provider, 10_000), &long, false)
            .unwrap()
            .expect("a shorter history");
        assert_eq!(compacted.folded, 40 - KEEP_LAST_MESSAGES);
    }

    #[test]
    fn a_conversation_with_room_to_spare_is_left_alone() {
        let provider = Summarizer::saying("a summary");
        let history = conversation(40);

        assert_eq!(
            compact_if_needed(&session_with_window(provider.clone(), 1_000_000), &history, false)
                .unwrap(),
            None
        );
        assert!(provider.asked.lock().unwrap().is_empty());
    }

    /// What an empty chat costs. A meter reading zero over a window with
    /// ~4 000 tokens already committed is a gauge that is not connected to
    /// anything — and it reads as reassurance.
    #[test]
    fn a_conversation_that_has_not_started_already_costs_something() {
        let provider = Summarizer::saying("a summary");
        let usage = usage(&session_with_window(provider, 200_000), &[]);

        assert_eq!(usage.conversation, 0);
        assert!(usage.instructions > 0, "the prompt is free");
        assert!(usage.tools > 0, "the schemas are free");
        assert_eq!(usage.total, usage.instructions + usage.tools);
    }

    /// The three parts behave differently, and that is the reason for three:
    /// folding the conversation shortens one of them and leaves the others
    /// exactly where they were.
    #[test]
    fn only_the_conversation_moves_when_the_conversation_does() {
        let provider = Summarizer::saying("a summary");
        let session = session_with_window(provider, 200_000);

        let empty = usage(&session, &[]);
        let talking = usage(&session, &conversation(40));

        assert!(talking.conversation > empty.conversation);
        assert_eq!(talking.instructions, empty.instructions);
        assert_eq!(talking.tools, empty.tools);
        assert_eq!(talking.total, empty.total + talking.conversation);
    }

    /// The meter's scale and the threshold have to be the same threshold.
    /// A gauge whose red zone is not where compaction happens is worse than
    /// no gauge.
    #[test]
    fn the_mark_on_the_meter_is_where_a_pass_actually_starts() {
        let provider = Summarizer::saying("a summary");
        let history = conversation(40);
        let session = session_with_window(provider, 200_000);
        let at = usage(&session, &history).compacts_at.expect("a known window");

        assert!(!compaction::should_compact(at - 1, Some(200_000), &history));
        assert!(compaction::should_compact(at, Some(200_000), &history));
    }

    /// No window means a number with no scale. Inventing one would draw a
    /// ring that fills against a guess.
    #[test]
    fn without_a_window_there_is_a_total_and_no_scale() {
        let provider = Summarizer::saying("a summary");
        let usage = usage(&session(provider), &conversation(40));

        assert!(usage.total > 0);
        assert_eq!(usage.limit, None);
        assert_eq!(usage.compacts_at, None);
    }

    /// A typo in the settings field, read as a window of zero, would fill the
    /// ring and promise to fold at nothing. The threshold already treats it as
    /// "not configured"; the meter has to agree, or the two disagree about the
    /// same number.
    #[test]
    fn a_window_of_zero_is_no_window_here_too() {
        let provider = Summarizer::saying("a summary");
        let usage = usage(&session_with_window(provider, 0), &conversation(40));

        assert_eq!(usage.limit, None);
        assert_eq!(usage.compacts_at, None);
    }

    /// The failure this closes: the prompt and the tool schemas are the
    /// largest fixed item in every request and neither is a message, so the
    /// estimate could not see them. A window measured by the conversation
    /// alone reads as having room it does not have, and the pass that was
    /// supposed to happen before the wall happens after it.
    ///
    /// The window is worked out from the real numbers rather than written
    /// down: editing the prompt or a tool's description must not turn this
    /// into a test of a stale constant.
    #[test]
    fn the_prompt_and_the_schemas_are_charged_for() {
        let provider = Summarizer::saying("a summary");
        let history = conversation(40);
        let conversation_only = compaction::estimate_tokens(&history);
        let fixed = fixed_request_tokens();
        // Two separate omissions, each of which would leave the other
        // looking like a working estimate.
        assert!(
            fixed > compaction::estimate_text_tokens(prompt::INSTRUCTIONS),
            "the tool schemas were not counted"
        );
        assert!(
            fixed > compaction::estimate_tool_schema_tokens(&tool_definitions()),
            "the system prompt was not counted"
        );

        // Roomy for the conversation on its own, full once the request's own
        // weight is on the scale.
        let limit = (conversation_only as u64 * 100 / compaction::TRIGGER_PERCENT) as u32 + 1
            + fixed as u32;
        assert!(
            !compaction::should_compact(conversation_only, Some(limit), &history),
            "the window has to be roomy for the conversation alone, or this proves nothing"
        );

        assert!(
            compact_if_needed(&session_with_window(provider, limit), &history, false)
                .unwrap()
                .is_some(),
            "the fixed cost of the request was not counted"
        );
    }

    /// Asking for it outright skips the threshold — and nothing else. A
    /// conversation with nothing worth folding stays as it is whoever asked.
    #[test]
    fn asking_for_it_skips_the_threshold_but_not_the_plan() {
        let provider = Summarizer::saying("a summary");
        let session = session(provider.clone());

        assert!(compact_if_needed(&session, &conversation(40), true)
            .unwrap()
            .is_some());
        assert_eq!(compact_if_needed(&session, &conversation(4), true).unwrap(), None);
    }

    /// The pair rule of `plan_compaction`, seen from the outside: what comes
    /// back must be a history a provider would accept.
    #[test]
    fn the_shorter_history_never_opens_with_an_orphaned_tool_result() {
        let provider = Summarizer::saying("a summary");
        let mut history = conversation(30);
        history.push(LlmMessage {
            role: crate::domain::llm::LlmRole::Assistant,
            content: None,
            tool_call_id: None,
            tool_calls: vec![LlmToolCall {
                id: "c1".to_string(),
                name: "readFile".to_string(),
                arguments: "{}".to_string(),
            }],
            native_content: None,
        });
        for _ in 0..11 {
            history.push(LlmMessage {
                role: crate::domain::llm::LlmRole::Tool,
                content: Some("ok".to_string()),
                tool_call_id: Some("c1".to_string()),
                tool_calls: Vec::new(),
                native_content: None,
            });
        }

        let compacted = compact(&session(provider), &history, KEEP_LAST_MESSAGES)
            .unwrap()
            .expect("a shorter history");

        assert_ne!(compacted.history[1].role, crate::domain::llm::LlmRole::Tool);
    }
}


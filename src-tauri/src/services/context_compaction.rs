//! Folding the older part of a conversation into a summary, so the rest of it
//! still fits.
//!
//! The rules are in `domain::compaction`; what is here is the one thing they
//! cannot do, which is ask the model. Nothing decides here: the caller says
//! how much to keep and this either comes back with a shorter history or with
//! nothing at all.

use crate::domain::compaction::{self, SUMMARY_INSTRUCTIONS};
use crate::domain::llm::{ChatRequest, LlmError, LlmMessage};
use crate::infra::llm_debug_log;
use crate::services::llm_session::LlmSession;

/// A conversation, shorter than it was.
#[derive(Debug, Clone, PartialEq)]
pub struct Compacted {
    pub history: Vec<LlmMessage>,
    /// How many messages the summary stands for — what the transcript says
    /// happened.
    pub folded: usize,
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
        });
        for _ in 0..11 {
            history.push(LlmMessage {
                role: crate::domain::llm::LlmRole::Tool,
                content: Some("ok".to_string()),
                tool_call_id: Some("c1".to_string()),
                tool_calls: Vec::new(),
            });
        }

        let compacted = compact(&session(provider), &history, KEEP_LAST_MESSAGES)
            .unwrap()
            .expect("a shorter history");

        assert_ne!(compacted.history[1].role, crate::domain::llm::LlmRole::Tool);
    }
}

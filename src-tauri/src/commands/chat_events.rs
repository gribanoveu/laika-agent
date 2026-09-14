//! The one event name a turn reports on, and the adapter that puts it there.
//!
//! This is the only place the loop's reports become Tauri events. `services::
//! llm_chat` has no `AppHandle` and no idea anything is listening — which is
//! what lets the same loop serve the window, a test and a log.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::domain::turn::{ChatEventSink, ChatTurnEvent};

/// Everything one turn reports, on one ordered channel.
///
/// One channel rather than a topic per payload kind. Separate topics look
/// tidier and leave the ordering between them unstated — and a transcript is
/// built entirely out of that ordering. Here `seq`, `round` and `targetId`
/// travel with every event, so a listener applies each one to a named block
/// and can tell late, repeated or out-of-order arrivals apart.
pub const CHAT_TURN_EVENT: &str = "chat:turn-event";

/// One event on the wire: what the turn said, plus which turn said it.
///
/// Flattened, so `turnId`, `seq`, `round`, `targetId`, `type` and `payload`
/// all sit at the top level and the listener's type is one flat union rather
/// than a wrapper it has to unwrap first.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WithTurn {
    turn_id: String,
    #[serde(flatten)]
    event: ChatTurnEvent,
}

/// A sink that emits everything it is given for `turn_id`.
///
/// The turn id is a transport concern and stays one: `ChatTurnEvent` never
/// learns about it. It is needed because the channel is global — one per name,
/// not one per window or per request — so two turns running at once would
/// otherwise interleave their deltas character by character into a single
/// message, which is exactly as readable as it sounds.
/// Generic over the runtime only so the suite can drive it with Tauri's mock
/// one: production passes the ordinary `AppHandle` and never names the
/// parameter. Without it the only thing testable here is a struct literal.
pub fn chat_event_sink<R: Runtime>(app: &AppHandle<R>, turn_id: String) -> ChatEventSink {
    let app = app.clone();
    Arc::new(move |event: ChatTurnEvent| {
        let _ = app.emit(
            CHAT_TURN_EVENT,
            WithTurn {
                turn_id: turn_id.clone(),
                event,
            },
        );
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::command_exec::OutputStream;
    use crate::domain::turn::{ChatEventPayload, ToolCallEvent};
    use tauri::Listener;

    fn on_the_wire(event: ChatTurnEvent) -> serde_json::Value {
        serde_json::to_value(WithTurn {
            turn_id: "turn-1".to_string(),
            event,
        })
        .expect("an event serializes")
    }

    fn event(seq: u64, payload: ChatEventPayload) -> ChatTurnEvent {
        ChatTurnEvent {
            seq,
            round: 2,
            target_id: Some("round:2:text".to_string()),
            event: payload,
        }
    }

    /// The shape a listener parses. Flat on purpose: a wrapper would make
    /// every listener unwrap before it can switch on `type`.
    #[test]
    fn an_event_is_flat_and_carries_its_turn() {
        let wire = on_the_wire(event(
            7,
            ChatEventPayload::Delta {
                delta: "hi".to_string(),
            },
        ));

        assert_eq!(wire["turnId"], "turn-1");
        assert_eq!(wire["seq"], 7);
        assert_eq!(wire["round"], 2);
        assert_eq!(wire["targetId"], "round:2:text");
        assert_eq!(wire["type"], "delta");
        assert_eq!(wire["payload"]["delta"], "hi");
    }

    /// Through the real sink and a real (mock) app, not by building the
    /// payload by hand: the first version of this test asserted that two
    /// structs with different ids differ, which is true of any two structs and
    /// says nothing about the sink. A mutation that stamped one constant id on
    /// every turn passed it.
    ///
    /// Two turns at once share the channel, and only the id tells them apart —
    /// without it their deltas interleave into a single message.
    #[test]
    fn the_sink_stamps_its_own_turn_on_everything_it_emits() {
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));

        let heard = seen.clone();
        handle.listen(CHAT_TURN_EVENT, move |event| {
            heard.lock().unwrap().push(event.payload().to_string());
        });

        let first = chat_event_sink(&handle, "turn-1".to_string());
        let second = chat_event_sink(&handle, "turn-2".to_string());
        first(event(1, ChatEventPayload::RoundStarted));
        second(event(1, ChatEventPayload::RoundStarted));

        let heard = seen.lock().unwrap();
        let ids: Vec<String> = heard
            .iter()
            .map(|payload| {
                serde_json::from_str::<serde_json::Value>(payload).expect("json")["turnId"]
                    .as_str()
                    .expect("a turn id")
                    .to_string()
            })
            .collect();
        assert_eq!(ids, ["turn-1", "turn-2"]);
    }

    /// The name is half of a contract whose other half is in TypeScript, and
    /// nothing in Rust can check that half. Pinning the literal here does not
    /// make the rename safe — it makes it deliberate, and puts the reminder in
    /// the diff of whoever does it.
    #[test]
    fn the_channel_name_is_also_written_down_in_the_frontend() {
        assert_eq!(CHAT_TURN_EVENT, "chat:turn-event");
    }

    /// And the whole event survives the trip, not just the id.
    #[test]
    fn what_the_listener_receives_is_the_flat_event() {
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let seen: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));

        let heard = seen.clone();
        handle.listen(CHAT_TURN_EVENT, move |event| {
            *heard.lock().unwrap() = Some(event.payload().to_string());
        });

        chat_event_sink(&handle, "turn-1".to_string())(event(
            9,
            ChatEventPayload::Delta {
                delta: "hi".to_string(),
            },
        ));

        let payload = seen.lock().unwrap().clone().expect("the listener heard it");
        let wire: serde_json::Value = serde_json::from_str(&payload).expect("json");
        assert_eq!(wire["turnId"], "turn-1");
        assert_eq!(wire["seq"], 9);
        assert_eq!(wire["type"], "delta");
        assert_eq!(wire["payload"]["delta"], "hi");
    }

    /// Every payload kind is tagged by `type` with its body under `payload`,
    /// including the ones that are not a plain struct — a listener switches on
    /// one field and never has to guess.
    #[test]
    fn every_payload_kind_is_tagged_the_same_way() {
        let cases = [
            (
                ChatEventPayload::RoundStarted,
                "roundStarted",
                serde_json::Value::Null,
            ),
            (
                ChatEventPayload::ToolCall(ToolCallEvent {
                    id: "c1".to_string(),
                    name: "readFile".to_string(),
                    arguments: "{}".to_string(),
                }),
                "toolCall",
                serde_json::json!("c1"),
            ),
            (
                ChatEventPayload::CommandOutput {
                    id: "c1".to_string(),
                    stream: OutputStream::Stderr,
                    chunk: "boom\n".to_string(),
                },
                "commandOutput",
                serde_json::json!("c1"),
            ),
        ];

        for (payload, expected_type, expected_id) in cases {
            let wire = on_the_wire(event(1, payload));
            assert_eq!(wire["type"], expected_type);
            if !expected_id.is_null() {
                assert_eq!(wire["payload"]["id"], expected_id, "{expected_type}");
            }
        }
    }

    /// A unit-like payload still serializes with a `type` and nothing else to
    /// read — a listener that switches on `type` needs no special case.
    #[test]
    fn a_payload_with_no_body_is_still_tagged() {
        let wire = on_the_wire(event(1, ChatEventPayload::RoundStarted));
        assert_eq!(wire["type"], "roundStarted");
        assert!(wire.get("payload").is_none(), "{wire}");
    }
}

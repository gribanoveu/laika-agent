//! The event name the index reports on, and the adapter that puts it there.
//!
//! Same split as `commands::chat_events`: `services::workspace_index` reports
//! through an `IndexEventSink` and knows nothing about Tauri; this is the one
//! place those reports become events.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::domain::workspace_index::{IndexEvent, IndexEventSink};

/// Everything the index of an open folder reports, on one channel.
pub const INDEX_EVENT: &str = "workspace-index:event";

/// One event on the wire, with the folder it is about.
///
/// The folder travels with the event because a sync outlives the folder being
/// open: switch folders mid-sync and the old one's `syncFinished` still
/// arrives, and a listener that could not tell would show it for the new one.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ForFolder {
    root: String,
    #[serde(flatten)]
    event: IndexEvent,
}

pub fn index_event_sink<R: Runtime>(app: &AppHandle<R>, root: String) -> IndexEventSink {
    let app = app.clone();
    Arc::new(move |event: IndexEvent| {
        let _ = app.emit(INDEX_EVENT, ForFolder { root: root.clone(), event });
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Listener;

    #[test]
    fn the_listener_receives_the_flat_event_with_its_folder() {
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let heard = Arc::clone(&seen);
        handle.listen(INDEX_EVENT, move |event| {
            heard.lock().unwrap().push(serde_json::from_str::<serde_json::Value>(event.payload()).unwrap());
        });

        index_event_sink(&handle, "/a".into())(IndexEvent::EmbeddingProgress { done: 1, total: 2 });
        index_event_sink(&handle, "/b".into())(IndexEvent::Failed { error: "no".into() });

        assert_eq!(
            *seen.lock().unwrap(),
            [
                serde_json::json!({ "root": "/a", "kind": "embeddingProgress", "done": 1, "total": 2 }),
                serde_json::json!({ "root": "/b", "kind": "failed", "error": "no" }),
            ]
        );
    }

    /// The other half of the name is in TypeScript; see the same test in
    /// `chat_events`.
    #[test]
    fn the_channel_name_is_pinned() {
        assert_eq!(INDEX_EVENT, "workspace-index:event");
    }
}

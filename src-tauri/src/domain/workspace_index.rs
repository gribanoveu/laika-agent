//! What indexing a repository reports as it goes. The service reports through
//! an [`IndexEventSink`]; the command layer is the only place these become
//! Tauri events (`AGENTS.md`, "Reporting outward crosses a port").

use std::sync::Arc;

use serde::Serialize;

/// One enum for everything a sync reports, rather than a callback per kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum IndexEvent {
    SyncStarted,
    /// The keyword index is current. Search by keyword works from here on,
    /// before a single vector exists — which on a first sync is most of the
    /// wait.
    KeywordsReady {
        indexed: usize,
        unchanged: usize,
        removed: usize,
        /// Files the index would not take. Each has a reason in
        /// `IndexStatus::skipped`; the count is what a status line shows.
        skipped: usize,
    },
    EmbeddingProgress { done: usize, total: usize },
    SyncFinished {
        embedded: usize,
        /// Why search by meaning is not current, when it is not — a missing
        /// model, say. Keyword search is unaffected.
        embedding_error: Option<String>,
    },
    /// The index is not current and will not become so by itself: the walk
    /// or the store failed, or the folder cannot be watched. The next change
    /// on disk retries a failed sync; a failed watch stays failed until the
    /// folder is opened again.
    Failed { error: String },
}

pub type IndexEventSink = Arc<dyn Fn(IndexEvent) + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape the frontend will read — one field to switch on, camelCase
    /// everywhere, including inside the variants.
    #[test]
    fn events_serialize_as_tagged_camel_case() {
        let json = serde_json::to_value(IndexEvent::SyncFinished { embedded: 2, embedding_error: None }).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "syncFinished", "embedded": 2, "embeddingError": null }));
        let json = serde_json::to_value(IndexEvent::EmbeddingProgress { done: 1, total: 3 }).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "embeddingProgress", "done": 1, "total": 3 }));
    }
}

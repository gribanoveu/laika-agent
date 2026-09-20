//! Saved conversations: one JSON file per chat under `<app dir>/chats`.
//!
//! A file rather than a database because of what this actually is — a folder
//! of conversations, read when one is opened and written once at the end of a
//! turn. SQLite would buy indexed listing, which starts to matter at a scale
//! (thousands of chats in one project) this never reaches, and would cost a
//! dependency, a schema and its migrations.
//!
//! **Revisit when** listing is slow enough to notice, or a chat has to be
//! searched by content rather than opened by name — both are index problems,
//! and the index layer (stage 5) brings the dependency anyway.
//!
//! One file per chat also decides what damage costs. A single store is a
//! single point of loss: upstream once refused to open a database written by
//! a newer build and thereby hid a user's entire history behind one number.
//! Here a chat this build cannot read is one row missing from the list, and
//! the rest open normally.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::domain::chat_export;
use crate::domain::chat_record::{
    self, ChatError, ChatRecord, ChatSummary, CHAT_SCHEMA_VERSION,
};
use crate::domain::llm::LlmMessage;
use crate::domain::tools::Task;
use crate::infra::app_dir;

const DIR: &str = "chats";

fn dir() -> Result<PathBuf, ChatError> {
    Ok(app_dir::dir().map_err(ChatError::AppDir)?.join(DIR))
}

fn path(id: &str) -> Result<PathBuf, ChatError> {
    chat_record::check_id(id)?;
    Ok(dir()?.join(format!("{id}.json")))
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Chats belonging to one workspace, most recently updated first — the order
/// the sidebar draws them in.
///
/// A file that cannot be read is skipped, not propagated. The list is how a
/// user reaches every *other* conversation, and one damaged file must not be
/// able to empty it.
pub fn list(workspace: &str) -> Result<Vec<ChatSummary>, ChatError> {
    let dir = dir()?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        // Nothing saved yet is an empty list, not a failure.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(ChatError::Read(e)),
    };

    let mut summaries: Vec<(ChatSummary, String)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .filter_map(|text| chat_record::parse(&text).ok())
        .filter(|record| record.workspace == workspace)
        .map(|record| (ChatSummary::from(&record), record.id))
        .collect();

    // Ties broken by id so the order is stable: two chats saved in the same
    // millisecond otherwise swap places between listings.
    summaries.sort_by(|a, b| {
        b.0.updated_at
            .cmp(&a.0.updated_at)
            .then_with(|| a.1.cmp(&b.1))
    });
    Ok(summaries.into_iter().map(|(summary, _)| summary).collect())
}

pub fn load(id: &str) -> Result<ChatRecord, ChatError> {
    let path = path(id)?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ChatError::NotFound(id.to_string()))
        }
        Err(e) => return Err(ChatError::Read(e)),
    };
    chat_record::parse(&text)
}

/// Writes the conversation whole, keeping the moment it started.
///
/// Whole because a conversation is edited from both ends — a compaction
/// rewrites the middle, a retry replaces the tail — so an append log would
/// need its own rules for what supersedes what. At one write per turn the
/// simpler thing is also fast enough.
pub fn save(
    id: &str,
    workspace: &str,
    messages: &[LlmMessage],
    blocks: &Value,
    todos: &[Task],
    plan: Option<&str>,
    branched_from: Option<&str>,
) -> Result<ChatSummary, ChatError> {
    let path = path(id)?;
    let existing = load(id).ok();

    let record = ChatRecord {
        schema_version: CHAT_SCHEMA_VERSION,
        id: id.to_string(),
        workspace: workspace.to_string(),
        title: chat_record::derive_title(messages),
        created_at: existing.as_ref().map_or_else(now, |old| old.created_at),
        updated_at: now(),
        messages: messages.to_vec(),
        blocks: blocks.clone(),
        todos: todos.to_vec(),
        plan: plan.map(str::to_string),
        branched_from: branched_from.map(str::to_string),
    };

    let text = serde_json::to_string(&record).map_err(ChatError::Parse)?;
    app_dir::write_private(&path, text.as_bytes()).map_err(ChatError::Write)?;
    Ok(ChatSummary::from(&record))
}

/// Writes one chat as a Markdown transcript, wherever the user chose to put
/// it — an ordinary file of theirs, not one of ours, so it is written plainly
/// rather than with the app directory's private permissions.
pub fn export(id: &str, path: &Path) -> Result<(), ChatError> {
    let record = load(id)?;
    fs::write(path, chat_export::to_markdown(&record)).map_err(|e| ChatError::Write(e.to_string()))
}

pub fn delete(id: &str) -> Result<(), ChatError> {
    match fs::remove_file(path(id)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(ChatError::NotFound(id.to_string()))
        }
        Err(e) => Err(ChatError::Read(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::with_app_dir;

    fn blocks(text: &str) -> Value {
        serde_json::json!([{ "kind": "user", "id": "user:0", "text": text }])
    }

    fn save_one(id: &str, workspace: &str, said: &str) -> ChatSummary {
        save(
            id,
            workspace,
            &[LlmMessage::user(said)],
            &blocks(said),
            &[],
            Some("# Plan"),
            None,
        )
        .unwrap()
    }

    #[test]
    fn a_saved_chat_comes_back_whole() {
        with_app_dir("chat-store-round-trip", || {
            save_one("one", "/repo", "why does it drop the token?");

            let record = load("one").unwrap();
            assert_eq!(record.workspace, "/repo");
            assert_eq!(record.title, "why does it drop the token?");
            assert_eq!(record.messages, vec![LlmMessage::user("why does it drop the token?")]);
            assert_eq!(record.blocks, blocks("why does it drop the token?"));
            assert_eq!(record.plan.as_deref(), Some("# Plan"));
        });
    }

    /// The transcript goes where the user pointed, not into the app's own
    /// folder — the whole point of an export is that it leaves.
    #[test]
    fn a_chat_is_exported_as_markdown_where_it_was_asked_for() {
        with_app_dir("chat-store-export", || {
            save_one("one", "/repo", "why does it drop the token?");
            let path = dir().unwrap().join("transcript.md");

            export("one", &path).unwrap();

            let text = fs::read_to_string(&path).unwrap();
            assert!(text.starts_with("# why does it drop the token?"), "{text}");
            assert!(text.contains("## User"), "{text}");
        });
    }

    #[test]
    fn exporting_a_chat_that_is_not_there_says_so() {
        with_app_dir("chat-store-export-missing", || {
            let path = dir().unwrap().join("transcript.md");
            assert!(matches!(export("nope", &path), Err(ChatError::NotFound(_))));
        });
    }

    #[test]
    fn nothing_saved_yet_lists_as_nothing() {
        with_app_dir("chat-store-empty", || {
            assert!(list("/repo").unwrap().is_empty());
        });
    }

    /// The sidebar is per project. A chat about another folder appearing in
    /// this one's list is worse than no list.
    #[test]
    fn a_chat_belongs_to_the_folder_it_was_about() {
        with_app_dir("chat-store-scope", || {
            save_one("one", "/repo/a", "first");
            save_one("two", "/repo/b", "second");

            let ids: Vec<String> = list("/repo/a").unwrap().into_iter().map(|c| c.id).collect();
            assert_eq!(ids, ["one"]);
        });
    }

    #[test]
    fn the_list_is_most_recent_first() {
        with_app_dir("chat-store-order", || {
            save_one("older", "/repo", "first");
            save_one("newer", "/repo", "second");
            // Same millisecond is likely here; make the order the one under test.
            let mut record = load("newer").unwrap();
            record.updated_at += 1000;
            app_dir::write_private(
                &path("newer").unwrap(),
                serde_json::to_string(&record).unwrap().as_bytes(),
            )
            .unwrap();

            let ids: Vec<String> = list("/repo").unwrap().into_iter().map(|c| c.id).collect();
            assert_eq!(ids, ["newer", "older"]);
        });
    }

    /// Resaving is the normal case — every turn does it — and it must not
    /// reset when the conversation began.
    #[test]
    fn saving_again_keeps_the_moment_the_chat_started() {
        with_app_dir("chat-store-created", || {
            save_one("one", "/repo", "first");
            let started = load("one").unwrap().created_at;

            std::thread::sleep(std::time::Duration::from_millis(5));
            save(
                "one",
                "/repo",
                &[LlmMessage::user("first"), LlmMessage::assistant("done")],
                &blocks("first"),
                &[],
                None,
                None,
            )
            .unwrap();

            let again = load("one").unwrap();
            assert_eq!(again.created_at, started);
            assert!(again.updated_at >= started);
            assert_eq!(again.messages.len(), 2);
        });
    }

    /// One unreadable file is one row missing, never an empty sidebar.
    #[test]
    fn a_damaged_chat_does_not_hide_the_others() {
        with_app_dir("chat-store-damaged", || {
            save_one("good", "/repo", "readable");
            save_one("bad", "/repo", "unreadable");
            fs::write(path("bad").unwrap(), "{ not json").unwrap();

            let ids: Vec<String> = list("/repo").unwrap().into_iter().map(|c| c.id).collect();
            assert_eq!(ids, ["good"]);
            assert!(matches!(load("bad"), Err(ChatError::Parse(_))));
        });
    }

    /// A chat written by a newer build stays where it is, untouched and
    /// unlisted, rather than being reported as damage or quietly overwritten.
    #[test]
    fn a_chat_from_a_newer_build_is_left_alone() {
        with_app_dir("chat-store-newer", || {
            save_one("future", "/repo", "from tomorrow");
            let mut value: Value =
                serde_json::from_str(&fs::read_to_string(path("future").unwrap()).unwrap())
                    .unwrap();
            value["schemaVersion"] = serde_json::json!(CHAT_SCHEMA_VERSION + 1);
            let raw = value.to_string();
            fs::write(path("future").unwrap(), &raw).unwrap();

            assert!(list("/repo").unwrap().is_empty());
            assert!(matches!(load("future"), Err(ChatError::UnsupportedVersion(_))));
            assert_eq!(fs::read_to_string(path("future").unwrap()).unwrap(), raw);
        });
    }

    /// The sidebar marks a branch by this, so it has to reach the listing.
    #[test]
    fn a_branch_says_where_it_came_from() {
        with_app_dir("chat-store-branch", || {
            save_one("one", "/repo", "first");
            save("two", "/repo", &[LlmMessage::user("first")], &blocks("first"), &[], None, Some("one")).unwrap();

            let listed = list("/repo").unwrap();
            let from = |id: &str| listed.iter().find(|c| c.id == id).unwrap().branched_from.clone();
            assert_eq!(from("one"), None);
            assert_eq!(from("two").as_deref(), Some("one"));
            assert_eq!(load("two").unwrap().branched_from.as_deref(), Some("one"));
        });
    }

    #[test]
    fn a_deleted_chat_is_gone() {
        with_app_dir("chat-store-delete", || {
            save_one("one", "/repo", "first");
            delete("one").unwrap();

            assert!(list("/repo").unwrap().is_empty());
            assert!(matches!(load("one"), Err(ChatError::NotFound(_))));
            assert!(matches!(delete("one"), Err(ChatError::NotFound(_))));
        });
    }

    /// The id arrives from the window and becomes a file name.
    #[test]
    fn an_id_that_is_not_an_id_never_reaches_the_filesystem() {
        with_app_dir("chat-store-bad-id", || {
            assert!(matches!(load("../settings"), Err(ChatError::BadId(_))));
            assert!(matches!(
                save("../settings", "/repo", &[], &Value::Null, &[], None, None),
                Err(ChatError::BadId(_))
            ));
        });
    }
}

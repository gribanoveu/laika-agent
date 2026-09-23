//! Saved conversations: one row per chat in `<app dir>/chats.db`.
//!
//! The listing columns (`workspace`, `title`, `updated_at`, `branched_from`)
//! are real columns, so the sidebar is one indexed query and never parses a
//! transcript. The record itself is stored whole as JSON in `body`, in the
//! `domain::chat_record` format — that file stays the contract.
//!
//! No database-wide version and no migrations. Upstream once refused to open
//! a database written by a newer build and thereby hid a user's entire
//! history behind one number. Here the version is per row: a chat this build
//! cannot read is one row missing from the list, left untouched, and the rest
//! open normally. A connection per call, as in `tool_call_log`: one write per
//! turn is not a hot path.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::domain::chat_export;
use crate::domain::chat_record::{
    self, ChatError, ChatRecord, ChatSummary, CHAT_SCHEMA_VERSION,
};
use crate::domain::llm::LlmMessage;
use crate::domain::tools::Task;
use crate::infra::app_dir;

const FILE: &str = "chats.db";

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 3000;
CREATE TABLE IF NOT EXISTS chats (
  id             TEXT PRIMARY KEY,
  workspace      TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  title          TEXT NOT NULL,
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL,
  branched_from  TEXT,
  body           TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS chats_workspace ON chats(workspace, updated_at DESC);
";

fn store(e: rusqlite::Error) -> ChatError {
    ChatError::Store(e.to_string())
}

fn open() -> Result<Connection, ChatError> {
    let path = app_dir::ensure().map_err(ChatError::AppDir)?.join(FILE);
    let conn = Connection::open(&path).map_err(store)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    conn.execute_batch(SCHEMA).map_err(store)?;
    Ok(conn)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Chats belonging to one workspace, most recently updated first — the order
/// the sidebar draws them in. Ties broken by id so the order is stable: two
/// chats saved in the same millisecond otherwise swap places between listings.
/// Rows from a newer build are not listed.
pub fn list(workspace: &str) -> Result<Vec<ChatSummary>, ChatError> {
    let conn = open()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, updated_at, branched_from FROM chats
             WHERE workspace = ?1 AND schema_version <= ?2
             ORDER BY updated_at DESC, id",
        )
        .map_err(store)?;
    let rows = stmt
        .query_map(params![workspace, CHAT_SCHEMA_VERSION], |row| {
            Ok(ChatSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                updated_at: row.get(2)?,
                branched_from: row.get(3)?,
            })
        })
        .map_err(store)?;
    rows.collect::<Result<_, _>>().map_err(store)
}

pub fn load(id: &str) -> Result<ChatRecord, ChatError> {
    chat_record::check_id(id)?;
    let body: Option<String> = open()?
        .query_row("SELECT body FROM chats WHERE id = ?1", params![id], |row| row.get(0))
        .optional()
        .map_err(store)?;
    chat_record::parse(&body.ok_or_else(|| ChatError::NotFound(id.to_string()))?)
}

/// Whether a chat with this id is stored, readable or not.
pub fn exists(id: &str) -> Result<bool, ChatError> {
    chat_record::check_id(id)?;
    open()?
        .query_row("SELECT 1 FROM chats WHERE id = ?1", params![id], |_| Ok(()))
        .optional()
        .map(|row| row.is_some())
        .map_err(store)
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
    chat_record::check_id(id)?;
    let conn = open()?;
    let created_at: Option<i64> = conn
        .query_row("SELECT created_at FROM chats WHERE id = ?1", params![id], |row| row.get(0))
        .optional()
        .map_err(store)?;

    let record = ChatRecord {
        schema_version: CHAT_SCHEMA_VERSION,
        id: id.to_string(),
        workspace: workspace.to_string(),
        title: chat_record::derive_title(messages),
        created_at: created_at.unwrap_or_else(now),
        updated_at: now(),
        messages: messages.to_vec(),
        blocks: blocks.clone(),
        todos: todos.to_vec(),
        plan: plan.map(str::to_string),
        branched_from: branched_from.map(str::to_string),
    };

    let body = serde_json::to_string(&record).map_err(ChatError::Parse)?;
    conn.execute(
        "INSERT OR REPLACE INTO chats
           (id, workspace, schema_version, title, created_at, updated_at, branched_from, body)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            record.id,
            record.workspace,
            record.schema_version,
            record.title,
            record.created_at,
            record.updated_at,
            record.branched_from,
            body,
        ],
    )
    .map_err(store)?;
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
    chat_record::check_id(id)?;
    match open()?.execute("DELETE FROM chats WHERE id = ?1", params![id]).map_err(store)? {
        0 => Err(ChatError::NotFound(id.to_string())),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::with_app_dir;

    fn blocks(text: &str) -> Value {
        serde_json::json!([{ "kind": "user", "id": "user:0", "text": text }])
    }

    fn body(id: &str) -> String {
        open().unwrap().query_row("SELECT body FROM chats WHERE id = ?1", [id], |r| r.get(0)).unwrap()
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
            let path = app_dir::ensure().unwrap().join("transcript.md");

            export("one", &path).unwrap();

            let text = fs::read_to_string(&path).unwrap();
            assert!(text.starts_with("# why does it drop the token?"), "{text}");
            assert!(text.contains("## User"), "{text}");
        });
    }

    #[test]
    fn exporting_a_chat_that_is_not_there_says_so() {
        with_app_dir("chat-store-export-missing", || {
            let path = app_dir::ensure().unwrap().join("transcript.md");
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
            open()
                .unwrap()
                .execute("UPDATE chats SET updated_at = updated_at + 1000 WHERE id = 'newer'", [])
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

    /// A damaged transcript fails to open on its own; the rest still list and open.
    #[test]
    fn a_damaged_chat_does_not_hide_the_others() {
        with_app_dir("chat-store-damaged", || {
            save_one("good", "/repo", "readable");
            save_one("bad", "/repo", "unreadable");
            open().unwrap().execute("UPDATE chats SET body = '{ not json' WHERE id = 'bad'", []).unwrap();

            let ids: Vec<String> = list("/repo").unwrap().into_iter().map(|c| c.id).collect();
            assert!(ids.contains(&"good".to_string()), "{ids:?}");
            assert!(load("good").is_ok());
            assert!(matches!(load("bad"), Err(ChatError::Parse(_))));
        });
    }

    /// A chat written by a newer build stays where it is, untouched and
    /// unlisted, rather than being reported as damage or quietly overwritten.
    #[test]
    fn a_chat_from_a_newer_build_is_left_alone() {
        with_app_dir("chat-store-newer", || {
            save_one("future", "/repo", "from tomorrow");
            let mut value: Value = serde_json::from_str(&body("future")).unwrap();
            value["schemaVersion"] = serde_json::json!(CHAT_SCHEMA_VERSION + 1);
            let raw = value.to_string();
            open()
                .unwrap()
                .execute(
                    "UPDATE chats SET schema_version = ?1, body = ?2 WHERE id = 'future'",
                    params![CHAT_SCHEMA_VERSION + 1, raw],
                )
                .unwrap();

            assert!(list("/repo").unwrap().is_empty());
            assert!(matches!(load("future"), Err(ChatError::UnsupportedVersion(_))));
            assert_eq!(body("future"), raw);
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
            assert!(!exists("one").unwrap());
        });
    }

    /// The id arrives from the window; one that is not an id is refused before
    /// any query runs.
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

//! The index on disk: one SQLite file per repository, holding everything
//! needed to reload the chunk index without rescanning the working tree and to
//! tell, file by file, what has changed since last time.
//!
//! It stores no vectors — those live beside it in their own file, one per
//! embedding model. The relational tables here are ids, byte offsets and
//! hashes.
//!
//! The exception is `chunks_fts`, the full-text index behind keyword search.
//! A full-text index *is* a copy of the text: there is no way to rank by term
//! statistics without one, and the alternative — reading and lowercasing every
//! chunk in the repository on every query — costs far more than the disk this
//! takes.
//!
//! ## Everything here is derived, and that changes what a failure means
//!
//! A chat file is the user's own writing: losing one loses something nobody
//! can reproduce, which is why `infra::chat_store` never deletes a file it
//! cannot read and why a newer format there is left strictly alone
//! (`docs/07-upstream-findings.md`, "чужой урок"). Nothing in this file is
//! like that. Every row can be rebuilt from the working tree, so a store that
//! does not match what this build expects is thrown away and built again —
//! including one written by a *newer* build, which a downgrade makes ordinary.
//! Rebuilding costs minutes; carrying a schema this build misreads costs
//! answers that are quietly wrong.
//!
//! That is also why there are no migrations. Upstream carries two, both for
//! databases already on real machines before a column existed. This store has
//! never shipped; a migration for a state that never happened is a branch
//! nobody can test.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

use crate::domain::chunk_index::CHUNK_VERSION;
use crate::domain::repo_index::{FileId, FileMetadata, INDEX_VERSION, Language};

const DB_FILE_NAME: &str = "chunks.db";

/// The two cheap keys under which the expensive versions are remembered. A
/// mismatch on either means every row is meaningless, vectors included.
const META_CHUNK_VERSION: &str = "chunk_version";
const META_INDEX_VERSION: &str = "index_version";

/// Version of the store's *derived* content — what can be recomputed from the
/// source files alone without asking a model anything: a chunk's
/// `qualified_name` and its row in the full-text index.
///
/// Deliberately separate from `CHUNK_VERSION`/`INDEX_VERSION`. Those two
/// invalidate the whole store and re-embedding a repository costs real time
/// and, on a paid model, real money. Nothing here feeds `chunk_hash`, so
/// rebuilding this is one read per file and every vector stays valid. Bump it
/// when the tokenizer changes or when chunks start being named differently.
const DERIVED_VERSION: &str = "1";
const META_DERIVED_VERSION: &str = "derived_version";

const SCHEMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
  file_id    TEXT PRIMARY KEY,
  file_hash  BLOB NOT NULL,
  size_bytes INTEGER NOT NULL,
  mtime_secs INTEGER NOT NULL,
  language   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chunks (
  chunk_id       TEXT PRIMARY KEY,
  file_id        TEXT NOT NULL REFERENCES files(file_id) ON DELETE CASCADE,
  language       TEXT NOT NULL,
  kind           TEXT NOT NULL,
  start_byte     INTEGER NOT NULL,
  end_byte       INTEGER NOT NULL,
  file_hash      BLOB NOT NULL,
  chunk_hash     BLOB NOT NULL,
  qualified_name TEXT,
  ordinal        INTEGER NOT NULL,
  -- This chunk's row in `chunks_fts`. A virtual table takes no foreign key,
  -- so the cascade above does not reach it; keeping the rowid here lets a
  -- delete find its full-text row through `idx_chunks_file_id` instead of
  -- scanning the whole index.
  fts_rowid      INTEGER
);
CREATE INDEX IF NOT EXISTS idx_chunks_file_id ON chunks(file_id);

-- `unicode61` case-folds Cyrillic but does not stem it, which is why the
-- query builder cuts longer terms to a stem and searches them as prefixes.
-- `prefix = '4 5 6'` covers exactly the lengths that produces, so those
-- lookups hit an index instead of walking the term list — the two have to
-- stay in step (`domain::search_query`, F-5.7).
-- `qualified_name` is indexed beside the text and weighted above it at query
-- time, so a query that is an identifier lands on the thing carrying the
-- name rather than on prose mentioning it.
CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
  chunk_id UNINDEXED,
  qualified_name,
  text,
  tokenize = 'unicode61 remove_diacritics 2',
  prefix = '4 5 6'
);

-- Written only once a vector has actually landed in the model's vector file.
-- Deliberately a separate write from `chunks.chunk_hash`: the gap between
-- them is what makes an interrupted sync resumable rather than a store that
-- claims work it never finished.
CREATE TABLE IF NOT EXISTS embeddings (
  chunk_id   TEXT NOT NULL REFERENCES chunks(chunk_id) ON DELETE CASCADE,
  model_id   TEXT NOT NULL,
  chunk_hash BLOB NOT NULL,
  PRIMARY KEY (chunk_id, model_id)
);

-- What a language indexer found, kept so a cold start can reuse the symbols
-- of a file whose hash is unchanged instead of parsing the whole repository
-- again. No primary key of its own: nothing refers to a symbol by id, only
-- by the file it came from.
CREATE TABLE IF NOT EXISTS symbols (
  file_id    TEXT NOT NULL REFERENCES files(file_id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  start_line INTEGER NOT NULL,
  end_line   INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_symbols_file_id ON symbols(file_id);
"#;

#[derive(Debug, Error)]
pub enum IndexStoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[source] std::io::Error),
    #[error("index store lock poisoned")]
    LockPoisoned,
}

/// One connection per repository, shared by everything that reads or writes
/// that repository's index.
pub struct IndexStore {
    conn: Mutex<Connection>,
    dir: PathBuf,
}

impl IndexStore {
    /// Opens the store in `dir`, creating it if needed, and guarantees that
    /// what comes back matches this build.
    ///
    /// The guard is here rather than in a caller on purpose: a store that
    /// opened is a store that can be trusted, and there is no order of calls
    /// to get wrong. A mismatch on [`CHUNK_VERSION`] or [`INDEX_VERSION`]
    /// empties it — see the module docs for why that is safe here and would
    /// not be anywhere the user's own writing lives.
    pub fn open(dir: &Path) -> Result<Self, IndexStoreError> {
        std::fs::create_dir_all(dir).map_err(IndexStoreError::Io)?;
        let conn = Connection::open(dir.join(DB_FILE_NAME))?;
        conn.execute_batch(SCHEMA_SQL)?;

        let store = Self {
            conn: Mutex::new(conn),
            dir: dir.to_path_buf(),
        };
        store.enforce_versions()?;
        Ok(store)
    }

    /// Empties the store unless it was written by a build that chunks and
    /// indexes the same way this one does, then records this build's
    /// versions. An empty store is left alone: there is nothing to throw
    /// away, and stamping it here is what makes the next open cheap.
    fn enforce_versions(&self) -> Result<(), IndexStoreError> {
        let chunk = CHUNK_VERSION.to_string();
        let index = INDEX_VERSION.to_string();
        let matches = self.read_meta(META_CHUNK_VERSION)?.as_deref() == Some(chunk.as_str())
            && self.read_meta(META_INDEX_VERSION)?.as_deref() == Some(index.as_str());
        if !matches {
            self.wipe()?;
            self.write_meta(META_CHUNK_VERSION, &chunk)?;
            self.write_meta(META_INDEX_VERSION, &index)?;
        }
        Ok(())
    }

    /// Where this model's vectors live. Per model, so switching models does
    /// not silently read one model's vectors as another's — they would be
    /// the right shape and the wrong meaning.
    pub fn vectors_path(&self, model_id: &str) -> PathBuf {
        self.dir.join(format!("vectors-{model_id}.usearch"))
    }

    /// Whether the cheap half has to be recomputed: chunk names and the
    /// full-text rows, everything derivable from the files themselves.
    ///
    /// Answered separately from the wipe above because the answer is
    /// different in kind. This costs one read per file and keeps every
    /// vector; the other costs the whole embedding run.
    pub fn derived_needs_backfill(&self) -> Result<bool, IndexStoreError> {
        Ok(self.read_meta(META_DERIVED_VERSION)?.as_deref() != Some(DERIVED_VERSION))
    }

    pub fn mark_derived_current(&self) -> Result<(), IndexStoreError> {
        self.write_meta(META_DERIVED_VERSION, DERIVED_VERSION)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, IndexStoreError> {
        self.conn.lock().map_err(|_| IndexStoreError::LockPoisoned)
    }

    pub fn read_meta(&self, key: &str) -> Result<Option<String>, IndexStoreError> {
        let conn = self.lock()?;
        Ok(conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn write_meta(&self, key: &str, value: &str) -> Result<(), IndexStoreError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_meta(&self, key: &str) -> Result<(), IndexStoreError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM meta WHERE key = ?1", params![key])?;
        Ok(())
    }

    /// Every row, gone. The full-text table is emptied by hand: it is a
    /// virtual table, so no foreign key reaches it and no cascade will.
    pub fn wipe(&self) -> Result<(), IndexStoreError> {
        let conn = self.lock()?;
        conn.execute_batch(
            "DELETE FROM embeddings;
             DELETE FROM chunks;
             DELETE FROM chunks_fts;
             DELETE FROM symbols;
             DELETE FROM files;
             DELETE FROM meta;",
        )?;
        Ok(())
    }

    /// Records what these files look like now. Everything hanging off a file
    /// — its chunks, their vectors, its symbols — is replaced separately, so
    /// this only moves the line the next sync compares against.
    pub fn upsert_files(&self, files: &[FileMetadata]) -> Result<(), IndexStoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        for file in files {
            // Before the epoch, or a clock that went backwards: the file is
            // not skippable, so it reads as "modified at the epoch" and is
            // compared by hash like everything else.
            let mtime_secs = file
                .modified_at
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs();
            tx.execute(
                "INSERT INTO files (file_id, file_hash, size_bytes, mtime_secs, language)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(file_id) DO UPDATE SET
                   file_hash  = excluded.file_hash,
                   size_bytes = excluded.size_bytes,
                   mtime_secs = excluded.mtime_secs,
                   language   = excluded.language",
                params![
                    file.relative_path,
                    file.hash.as_bytes().to_vec(),
                    file.size_bytes as i64,
                    mtime_secs as i64,
                    language_to_str(file.language),
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Forgets these files and, through the cascade, their chunks, vectors
    /// and symbols. The full-text rows are dropped first: the cascade cannot
    /// reach a virtual table, and rows left behind there would keep matching
    /// searches for a file that is gone.
    pub fn delete_files(&self, file_ids: &[FileId]) -> Result<(), IndexStoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        for file_id in file_ids {
            purge_fts_rows(&tx, file_id)?;
            tx.execute("DELETE FROM files WHERE file_id = ?1", params![file_id.0])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Every file the store knows about, for diffing against the working
    /// tree. A row whose language this build no longer has a name for is
    /// skipped rather than guessed at — it will be re-read as whatever it is
    /// now, which is the same work the diff was about to schedule anyway.
    pub fn load_all_files(&self) -> Result<HashMap<FileId, FileMetadata>, IndexStoreError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT file_id, file_hash, size_bytes, mtime_secs, language FROM files",
        )?;
        let rows = stmt.query_map([], |row| {
            let path: String = row.get(0)?;
            let hash: Vec<u8> = row.get(1)?;
            let size_bytes: i64 = row.get(2)?;
            let mtime_secs: i64 = row.get(3)?;
            let language: String = row.get(4)?;
            Ok((path, hash, size_bytes, mtime_secs, language))
        })?;

        let mut files = HashMap::new();
        for row in rows {
            let (path, hash, size_bytes, mtime_secs, language) = row?;
            let (Some(language), Some(hash)) = (str_to_language(&language), hash_from_bytes(&hash))
            else {
                continue;
            };
            files.insert(
                FileId(path.clone()),
                FileMetadata {
                    relative_path: path,
                    size_bytes: size_bytes.max(0) as u64,
                    modified_at: UNIX_EPOCH + Duration::from_secs(mtime_secs.max(0) as u64),
                    hash,
                    language,
                },
            );
        }
        Ok(files)
    }
}

/// Drops a file's rows from the full-text index, found through `chunks` (and
/// so through its index on `file_id`) rather than by scanning a table that
/// does not index the column being searched on.
fn purge_fts_rows(tx: &rusqlite::Transaction<'_>, file_id: &FileId) -> Result<(), IndexStoreError> {
    let rowids: Vec<i64> = {
        let mut stmt = tx
            .prepare("SELECT fts_rowid FROM chunks WHERE file_id = ?1 AND fts_rowid IS NOT NULL")?;
        let rows = stmt.query_map(params![file_id.0], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for rowid in rowids {
        tx.execute("DELETE FROM chunks_fts WHERE rowid = ?1", params![rowid])?;
    }
    Ok(())
}

/// `None` for anything that is not 32 bytes. Upstream panics here on the
/// grounds that the column is always a hash — which is true of every row this
/// code writes, and not of a file somebody's backup tool restored half of.
fn hash_from_bytes(bytes: &[u8]) -> Option<blake3::Hash> {
    let array: [u8; 32] = bytes.try_into().ok()?;
    Some(blake3::Hash::from(array))
}

/// The name a language is stored under. Written out rather than derived from
/// `Debug`, so renaming a variant in Rust cannot silently orphan every row
/// already written under its old name.
fn language_to_str(language: Language) -> &'static str {
    match language {
        Language::PlainText => "plaintext",
        Language::Json => "json",
        Language::Yaml => "yaml",
        Language::Markdown => "markdown",
        Language::Rust => "rust",
        Language::TypeScript => "typescript",
        Language::Tsx => "tsx",
        Language::JavaScript => "javascript",
        Language::Python => "python",
        Language::Go => "go",
        Language::Java => "java",
    }
}

fn str_to_language(value: &str) -> Option<Language> {
    Language::ALL
        .iter()
        .copied()
        .find(|language| language_to_str(*language) == value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    fn store(label: &str) -> (IndexStore, PathBuf) {
        let dir = temp_dir(label);
        (IndexStore::open(&dir).expect("a store"), dir)
    }

    fn file(path: &str, content: &str) -> FileMetadata {
        FileMetadata {
            relative_path: path.to_string(),
            size_bytes: content.len() as u64,
            modified_at: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            hash: blake3::hash(content.as_bytes()),
            language: crate::domain::repo_index::detect_language(path),
        }
    }

    /// Inserts a chunk and its full-text row directly, so the cascade and the
    /// purge can be exercised before the chunk writer exists.
    fn plant_chunk(store: &IndexStore, file_id: &str, chunk_id: &str, text: &str) {
        let conn = store.lock().unwrap();
        conn.execute(
            "INSERT INTO chunks_fts (chunk_id, qualified_name, text) VALUES (?1, '', ?2)",
            params![chunk_id, text],
        )
        .unwrap();
        let rowid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO chunks (chunk_id, file_id, language, kind, start_byte, end_byte,
                                 file_hash, chunk_hash, qualified_name, ordinal, fts_rowid)
             VALUES (?1, ?2, 'rust', 'file', 0, 1, ?3, ?3, NULL, 0, ?4)",
            params![chunk_id, file_id, blake3::hash(b"x").as_bytes().to_vec(), rowid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO embeddings (chunk_id, model_id, chunk_hash) VALUES (?1, 'm', ?2)",
            params![chunk_id, blake3::hash(b"x").as_bytes().to_vec()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO symbols (file_id, name, start_line, end_line, start_byte, end_byte)
             VALUES (?1, 'thing', 0, 1, 0, 1)",
            params![file_id],
        )
        .unwrap();
    }

    fn count(store: &IndexStore, table: &str) -> i64 {
        let conn = store.lock().unwrap();
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))
            .unwrap()
    }

    // ------------------------------------------------------------- opening

    #[test]
    fn opening_twice_keeps_what_was_written() {
        let dir = temp_dir("store-reopen");
        {
            let store = IndexStore::open(&dir).unwrap();
            store.upsert_files(&[file("src/main.rs", "fn main() {}")]).unwrap();
        }
        let reopened = IndexStore::open(&dir).unwrap();

        assert_eq!(reopened.load_all_files().unwrap().len(), 1);
    }

    /// A store written by a build that chunks differently describes byte
    /// ranges that no longer mean anything. Keeping it would return snippets
    /// cut in the wrong places, which reads as the file having changed.
    #[test]
    fn a_store_from_a_different_chunker_is_emptied() {
        let dir = temp_dir("store-stale");
        {
            let store = IndexStore::open(&dir).unwrap();
            store.upsert_files(&[file("src/main.rs", "fn main() {}")]).unwrap();
            store.write_meta(META_CHUNK_VERSION, "999").unwrap();
        }
        let reopened = IndexStore::open(&dir).unwrap();

        assert!(reopened.load_all_files().unwrap().is_empty());
        assert_eq!(
            reopened.read_meta(META_CHUNK_VERSION).unwrap().as_deref(),
            Some(CHUNK_VERSION.to_string().as_str()),
            "the store was emptied but not re-stamped, so it empties again every open"
        );
    }

    #[test]
    fn a_store_from_a_different_indexer_is_emptied_too() {
        let dir = temp_dir("store-stale-index");
        {
            let store = IndexStore::open(&dir).unwrap();
            store.upsert_files(&[file("a.rs", "x")]).unwrap();
            store.write_meta(META_INDEX_VERSION, "999").unwrap();
        }
        assert!(IndexStore::open(&dir).unwrap().load_all_files().unwrap().is_empty());
    }

    /// Nothing here is the user's writing — every row is rebuildable from the
    /// working tree — so a database from a newer build is thrown away rather
    /// than preserved. The opposite call is right for the chat store, and the
    /// difference is what the file holds, not what shape the version is.
    #[test]
    fn a_store_from_a_newer_build_is_rebuilt_rather_than_kept() {
        let dir = temp_dir("store-newer");
        {
            let store = IndexStore::open(&dir).unwrap();
            store.upsert_files(&[file("a.rs", "x")]).unwrap();
            store.write_meta(META_CHUNK_VERSION, "99999").unwrap();
            store.write_meta(META_INDEX_VERSION, "99999").unwrap();
        }
        let reopened = IndexStore::open(&dir).unwrap();

        assert!(reopened.load_all_files().unwrap().is_empty());
        assert_eq!(
            reopened.read_meta(META_CHUNK_VERSION).unwrap().as_deref(),
            Some(CHUNK_VERSION.to_string().as_str()),
            "the newer version was left in place, so this build empties the store on every open"
        );
    }

    /// The whole point of two numbers. The cheap one must not be able to
    /// trigger the expensive rebuild — re-embedding a repository costs hours
    /// and, on a paid model, money.
    #[test]
    fn the_cheap_version_never_empties_the_store() {
        let dir = temp_dir("store-derived");
        {
            let store = IndexStore::open(&dir).unwrap();
            store.upsert_files(&[file("a.rs", "x")]).unwrap();
            store.mark_derived_current().unwrap();
            store.write_meta(META_DERIVED_VERSION, "999").unwrap();
        }
        let reopened = IndexStore::open(&dir).unwrap();

        assert_eq!(reopened.load_all_files().unwrap().len(), 1, "the store was emptied");
        assert!(reopened.derived_needs_backfill().unwrap(), "the backfill was not asked for");
    }

    #[test]
    fn a_fresh_store_asks_for_the_cheap_backfill_and_stops_once_it_is_done() {
        let (store, _dir) = store("store-derived-fresh");

        assert!(store.derived_needs_backfill().unwrap());
        store.mark_derived_current().unwrap();
        assert!(!store.derived_needs_backfill().unwrap());
    }

    // ---------------------------------------------------------------- meta

    #[test]
    fn meta_round_trips_and_the_last_write_wins() {
        let (store, _dir) = store("store-meta");

        assert_eq!(store.read_meta("absent").unwrap(), None);
        store.write_meta("k", "one").unwrap();
        store.write_meta("k", "two").unwrap();
        assert_eq!(store.read_meta("k").unwrap().as_deref(), Some("two"));
        store.delete_meta("k").unwrap();
        assert_eq!(store.read_meta("k").unwrap(), None);
    }

    // --------------------------------------------------------------- files

    #[test]
    fn a_file_round_trips_with_everything_the_diff_compares() {
        let (store, _dir) = store("store-files");
        let original = file("src/App.tsx", "export const App = () => null;");
        store.upsert_files(&[original.clone()]).unwrap();

        let loaded = store.load_all_files().unwrap();
        assert_eq!(loaded.get(&FileId("src/App.tsx".into())), Some(&original));
    }

    #[test]
    fn re_reading_a_file_replaces_its_row_rather_than_adding_one() {
        let (store, _dir) = store("store-files-upsert");
        store.upsert_files(&[file("a.rs", "before")]).unwrap();
        store.upsert_files(&[file("a.rs", "after")]).unwrap();

        let loaded = store.load_all_files().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[&FileId("a.rs".into())].hash, blake3::hash(b"after"));
    }

    /// Every language has to survive the round trip, or files of whichever
    /// one does not come back as unknown and get re-read forever.
    #[test]
    fn every_language_round_trips() {
        for &language in Language::ALL {
            assert_eq!(
                str_to_language(language_to_str(language)),
                Some(language),
                "{language:?} does not survive the store"
            );
        }
    }

    /// A build that dropped a language must not resurrect its rows as some
    /// other language — the file is simply read again.
    #[test]
    fn a_language_this_build_does_not_know_is_skipped() {
        let (store, _dir) = store("store-unknown-language");
        store.upsert_files(&[file("a.rs", "x")]).unwrap();
        {
            let conn = store.lock().unwrap();
            conn.execute("UPDATE files SET language = 'cobol'", []).unwrap();
        }

        assert!(store.load_all_files().unwrap().is_empty());
    }

    /// A truncated hash column would otherwise panic on the way in. It is a
    /// cache on somebody's disk; half a row is a thing that happens.
    #[test]
    fn a_damaged_hash_is_skipped_rather_than_panicking() {
        let (store, _dir) = store("store-damaged-hash");
        store.upsert_files(&[file("a.rs", "x")]).unwrap();
        {
            let conn = store.lock().unwrap();
            conn.execute("UPDATE files SET file_hash = X'0011'", []).unwrap();
        }

        assert!(store.load_all_files().unwrap().is_empty());
    }

    // ------------------------------------------------------------ deleting

    #[test]
    fn deleting_a_file_takes_its_chunks_vectors_and_symbols_with_it() {
        let (store, _dir) = store("store-cascade");
        store.upsert_files(&[file("a.rs", "x"), file("b.rs", "y")]).unwrap();
        plant_chunk(&store, "a.rs", "a.rs#0-1", "alpha");
        plant_chunk(&store, "b.rs", "b.rs#0-1", "beta");

        store.delete_files(&[FileId("a.rs".into())]).unwrap();

        assert_eq!(count(&store, "files"), 1);
        assert_eq!(count(&store, "chunks"), 1, "the cascade missed chunks");
        assert_eq!(count(&store, "embeddings"), 1, "the cascade missed embeddings");
        assert_eq!(count(&store, "symbols"), 1, "the cascade missed symbols");
    }

    /// The full-text table takes no foreign key, so nothing cascades into it.
    /// A row left there keeps matching searches for a file that is gone —
    /// a result pointing at nothing, which is worse than no result.
    #[test]
    fn deleting_a_file_also_removes_it_from_the_full_text_index() {
        let (store, _dir) = store("store-cascade-fts");
        store.upsert_files(&[file("a.rs", "x"), file("b.rs", "y")]).unwrap();
        plant_chunk(&store, "a.rs", "a.rs#0-1", "alpha");
        plant_chunk(&store, "b.rs", "b.rs#0-1", "beta");

        store.delete_files(&[FileId("a.rs".into())]).unwrap();

        assert_eq!(count(&store, "chunks_fts"), 1, "the full-text row outlived its file");
        let conn = store.lock().unwrap();
        let left: String = conn
            .query_row("SELECT chunk_id FROM chunks_fts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(left, "b.rs#0-1", "the wrong file's full-text row was removed");
    }

    #[test]
    fn wiping_leaves_nothing_behind_in_any_table() {
        let (store, _dir) = store("store-wipe");
        store.upsert_files(&[file("a.rs", "x")]).unwrap();
        plant_chunk(&store, "a.rs", "a.rs#0-1", "alpha");

        store.wipe().unwrap();

        for table in ["files", "chunks", "chunks_fts", "embeddings", "symbols", "meta"] {
            assert_eq!(count(&store, table), 0, "{table} survived the wipe");
        }
    }

    // ------------------------------------------------------------- vectors

    /// One file per model. Reading one model's vectors as another's would
    /// give the right shape and the wrong meaning — nothing would fail, the
    /// answers would just be nonsense.
    #[test]
    fn each_model_gets_its_own_vector_file() {
        let (store, dir) = store("store-vectors");

        assert_eq!(
            store.vectors_path("potion-int8"),
            dir.join("vectors-potion-int8.usearch")
        );
        assert_ne!(store.vectors_path("a"), store.vectors_path("b"));
    }
}

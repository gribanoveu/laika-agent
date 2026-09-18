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

use crate::domain::chunk_index::{CHUNK_VERSION, Chunk, ChunkId, ChunkKind, ChunkMetadata};
use crate::domain::repo_index::{FileId, FileMetadata, INDEX_VERSION, Language, Symbol};

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

    /// Replaces everything this file was chunked into.
    ///
    /// Delete then insert, rather than a diff: a file that changed at all
    /// usually shifts every byte range after the edit, so the chunks that
    /// survive unchanged are the ones before it and nothing is saved by
    /// finding them. Dropping the chunk rows takes their vectors with them
    /// through the cascade, which is correct — a vector describes a byte
    /// range that no longer exists.
    ///
    /// The full-text rows are purged first for the reason they always are:
    /// no cascade reaches a virtual table.
    pub fn replace_chunks_for_file(
        &self,
        file_id: &FileId,
        chunks: &[Chunk],
    ) -> Result<(), IndexStoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        purge_fts_rows(&tx, file_id)?;
        tx.execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id.0])?;
        for chunk in chunks {
            let meta = &chunk.metadata;
            let fts_rowid =
                insert_fts_row(&tx, &meta.id, meta.qualified_name.as_deref(), &chunk.text)?;
            tx.execute(
                "INSERT INTO chunks (chunk_id, file_id, language, kind, start_byte, end_byte,
                                     file_hash, chunk_hash, qualified_name, ordinal, fts_rowid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    meta.id.0,
                    meta.file_id.0,
                    language_to_str(meta.language),
                    chunk_kind_to_str(meta.kind),
                    meta.start_byte,
                    meta.end_byte,
                    meta.file_hash.as_bytes().to_vec(),
                    meta.hash.as_bytes().to_vec(),
                    meta.qualified_name,
                    meta.ordinal,
                    fts_rowid,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Rewrites only the cheap half of this file's chunks — each one's name
    /// and its full-text row — leaving the identity and hash columns alone
    /// and, above all, leaving `embeddings` alone.
    ///
    /// This is what [`derived_needs_backfill`](Self::derived_needs_backfill)
    /// asks for. A store whose chunk rows are correct and whose vectors took
    /// an hour to compute needs neither thrown away because the tokenizer
    /// changed; [`replace_chunks_for_file`](Self::replace_chunks_for_file)
    /// would cascade those vectors away for nothing.
    ///
    /// A chunk that does not match a stored row is dropped rather than
    /// inserted: the file has been edited since the last sync, so it rebuilt
    /// to different byte ranges and different ids. Inserting the full-text
    /// row anyway would leave one nothing points at, which nothing could ever
    /// find again to delete. The sync re-chunks such a file through the
    /// ordinary path immediately afterwards.
    pub fn replace_derived_for_file(
        &self,
        file_id: &FileId,
        chunks: &[Chunk],
    ) -> Result<(), IndexStoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        purge_fts_rows(&tx, file_id)?;
        for chunk in chunks {
            let meta = &chunk.metadata;
            let fts_rowid =
                insert_fts_row(&tx, &meta.id, meta.qualified_name.as_deref(), &chunk.text)?;
            let linked = tx.execute(
                "UPDATE chunks SET qualified_name = ?1, fts_rowid = ?2 WHERE chunk_id = ?3",
                params![meta.qualified_name, fts_rowid, meta.id.0],
            )?;
            if linked == 0 {
                tx.execute("DELETE FROM chunks_fts WHERE rowid = ?1", params![fts_rowid])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every chunk the store knows about, without its text — that is read
    /// from the file when a result needs a snippet, so the index does not
    /// hold a second copy of the repository outside the full-text table.
    ///
    /// A row this build cannot read is skipped, not fatal, for the same
    /// reason as in `load_all_files`: the file it belongs to is re-read and
    /// re-chunked, which the diff was going to do anyway. One unreadable row
    /// blinding the whole index would turn a stale column into an index that
    /// will not load.
    pub fn load_all_chunks(&self) -> Result<Vec<ChunkMetadata>, IndexStoreError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT chunk_id, file_id, language, kind, start_byte, end_byte,
                    file_hash, chunk_hash, qualified_name, ordinal
             FROM chunks",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, u32>(4)?,
                row.get::<_, u32>(5)?,
                row.get::<_, Vec<u8>>(6)?,
                row.get::<_, Vec<u8>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, u32>(9)?,
            ))
        })?;

        let mut chunks = Vec::new();
        for row in rows {
            let (id, file_id, language, kind, start, end, file_hash, hash, name, ordinal) = row?;
            let (Some(language), Some(kind), Some(file_hash), Some(hash)) = (
                str_to_language(&language),
                str_to_chunk_kind(&kind),
                hash_from_bytes(&file_hash),
                hash_from_bytes(&hash),
            ) else {
                continue;
            };
            chunks.push(ChunkMetadata {
                id: ChunkId(id),
                file_id: FileId(file_id),
                language,
                kind,
                start_byte: start,
                end_byte: end,
                file_hash,
                hash,
                qualified_name: name,
                ordinal,
            });
        }
        Ok(chunks)
    }

    /// Replaces what an indexer found in this file.
    ///
    /// Kept so a cold start can reuse the symbols of a file whose hash has
    /// not changed instead of parsing the whole repository again — which on a
    /// large tree is the difference between an index that is ready and one
    /// that is still thinking.
    pub fn replace_symbols_for_file(
        &self,
        file_id: &FileId,
        symbols: &[Symbol],
    ) -> Result<(), IndexStoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM symbols WHERE file_id = ?1", params![file_id.0])?;
        for symbol in symbols {
            tx.execute(
                "INSERT INTO symbols (file_id, name, start_line, end_line, start_byte, end_byte)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    file_id.0,
                    symbol.name,
                    symbol.start_line,
                    symbol.end_line,
                    symbol.start_byte,
                    symbol.end_byte,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Every stored symbol, grouped by the file it came from.
    pub fn load_all_symbols(&self) -> Result<HashMap<FileId, Vec<Symbol>>, IndexStoreError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT file_id, name, start_line, end_line, start_byte, end_byte FROM symbols",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                FileId(row.get(0)?),
                Symbol {
                    name: row.get(1)?,
                    start_line: row.get(2)?,
                    end_line: row.get(3)?,
                    start_byte: row.get(4)?,
                    end_byte: row.get(5)?,
                },
            ))
        })?;

        let mut out: HashMap<FileId, Vec<Symbol>> = HashMap::new();
        for row in rows {
            let (file_id, symbol) = row?;
            out.entry(file_id).or_default().push(symbol);
        }
        Ok(out)
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

/// Inserts one chunk into the full-text index, returning the rowid the caller
/// stores in `chunks.fts_rowid` so this row can be found again to delete it.
fn insert_fts_row(
    tx: &rusqlite::Transaction<'_>,
    chunk_id: &ChunkId,
    qualified_name: Option<&str>,
    text: &str,
) -> Result<i64, IndexStoreError> {
    tx.execute(
        "INSERT INTO chunks_fts (chunk_id, qualified_name, text) VALUES (?1, ?2, ?3)",
        params![chunk_id.0, qualified_name.unwrap_or(""), text],
    )?;
    Ok(tx.last_insert_rowid())
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
/// Same rule as the language names: written out, so renaming a variant cannot
/// orphan rows stored under its old name.
fn chunk_kind_to_str(kind: ChunkKind) -> &'static str {
    match kind {
        ChunkKind::Section => "section",
        ChunkKind::File => "file",
    }
}

fn str_to_chunk_kind(value: &str) -> Option<ChunkKind> {
    match value {
        "section" => Some(ChunkKind::Section),
        "file" => Some(ChunkKind::File),
        _ => None,
    }
}

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

    // -------------------------------------------------------------- chunks

    fn chunk(file: &str, start: u32, end: u32, name: Option<&str>, text: &str) -> Chunk {
        let file_hash = blake3::hash(b"file");
        Chunk {
            metadata: ChunkMetadata {
                id: ChunkId(format!("{file}#{start}-{end}")),
                file_id: FileId(file.to_string()),
                language: Language::Markdown,
                kind: if name.is_some() { ChunkKind::Section } else { ChunkKind::File },
                start_byte: start,
                end_byte: end,
                file_hash,
                hash: crate::domain::chunk_index::chunk_hash(file_hash, start, end),
                qualified_name: name.map(str::to_string),
                ordinal: 0,
            },
            text: text.to_string(),
        }
    }

    fn fts_text(store: &IndexStore) -> Vec<String> {
        let conn = store.lock().unwrap();
        let mut stmt = conn.prepare("SELECT text FROM chunks_fts ORDER BY rowid").unwrap();
        let rows = stmt.query_map([], |row| row.get(0)).unwrap();
        rows.collect::<Result<Vec<String>, _>>().unwrap()
    }

    #[test]
    fn a_chunk_round_trips_with_every_column_the_index_needs() {
        let (store, _dir) = store("store-chunks");
        store.upsert_files(&[file("a.md", "# One\nbody\n")]).unwrap();
        let written = chunk("a.md", 0, 11, Some("One"), "# One\nbody\n");

        store.replace_chunks_for_file(&FileId("a.md".into()), &[written.clone()]).unwrap();

        let loaded = store.load_all_chunks().unwrap();
        assert_eq!(loaded, vec![written.metadata]);
    }

    /// A file that changed shifts every byte range after the edit, so the
    /// chunks are replaced wholesale rather than diffed. The old rows must
    /// not survive alongside the new ones under different ids.
    #[test]
    fn re_chunking_a_file_replaces_its_chunks_rather_than_adding_to_them() {
        let (store, _dir) = store("store-chunks-replace");
        store.upsert_files(&[file("a.md", "x")]).unwrap();
        let id = FileId("a.md".into());

        store
            .replace_chunks_for_file(&id, &[chunk("a.md", 0, 5, None, "first"), chunk("a.md", 5, 9, None, "more")])
            .unwrap();
        store.replace_chunks_for_file(&id, &[chunk("a.md", 0, 6, None, "second")]).unwrap();

        assert_eq!(store.load_all_chunks().unwrap().len(), 1);
        assert_eq!(fts_text(&store), vec!["second"], "the old full-text rows outlived their chunks");
    }

    /// The text goes into the full-text index and nowhere else: a snippet is
    /// read from the file when a result needs one, so the relational tables
    /// hold no second copy of the repository.
    #[test]
    fn the_text_reaches_the_full_text_index_and_the_name_with_it() {
        let (store, _dir) = store("store-chunks-fts");
        store.upsert_files(&[file("a.md", "x")]).unwrap();
        store
            .replace_chunks_for_file(
                &FileId("a.md".into()),
                &[chunk("a.md", 0, 9, Some("Guide > Install"), "run the installer")],
            )
            .unwrap();

        let conn = store.lock().unwrap();
        let (name, text): (String, String) = conn
            .query_row("SELECT qualified_name, text FROM chunks_fts", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(name, "Guide > Install");
        assert_eq!(text, "run the installer");
    }

    /// Chunks for a file the store has never seen are refused rather than
    /// stored unattached — rows that no file owns are never re-read and never
    /// deleted, and would keep matching searches forever.
    #[test]
    fn chunks_for_an_unknown_file_are_refused() {
        let (store, _dir) = store("store-chunks-orphan");
        assert!(
            store
                .replace_chunks_for_file(&FileId("ghost.md".into()), &[chunk("ghost.md", 0, 1, None, "x")])
                .is_err()
        );
    }

    #[test]
    fn a_chunk_row_this_build_cannot_read_is_skipped_rather_than_fatal() {
        let (store, _dir) = store("store-chunks-unreadable");
        store.upsert_files(&[file("a.md", "x")]).unwrap();
        store
            .replace_chunks_for_file(
                &FileId("a.md".into()),
                &[chunk("a.md", 0, 1, None, "one"), chunk("a.md", 1, 2, None, "two")],
            )
            .unwrap();
        {
            let conn = store.lock().unwrap();
            conn.execute("UPDATE chunks SET kind = 'method' WHERE start_byte = 0", []).unwrap();
        }

        let loaded = store.load_all_chunks().unwrap();
        assert_eq!(loaded.len(), 1, "one unreadable row blinded the whole index");
        assert_eq!(loaded[0].start_byte, 1);
    }

    #[test]
    fn every_chunk_kind_round_trips() {
        for kind in [ChunkKind::Section, ChunkKind::File] {
            assert_eq!(str_to_chunk_kind(chunk_kind_to_str(kind)), Some(kind), "{kind:?}");
        }
    }

    // ----------------------------------------------------- the cheap rebuild

    /// The whole reason the cheap version exists. Re-naming chunks and
    /// rebuilding the full-text index must not touch the vectors, which cost
    /// an hour to compute and describe byte ranges that did not move.
    #[test]
    fn rebuilding_the_derived_half_keeps_the_vectors() {
        let (store, _dir) = store("store-derived-keeps");
        store.upsert_files(&[file("a.md", "x")]).unwrap();
        let id = FileId("a.md".into());
        store.replace_chunks_for_file(&id, &[chunk("a.md", 0, 9, None, "body")]).unwrap();
        {
            let conn = store.lock().unwrap();
            conn.execute(
                "INSERT INTO embeddings (chunk_id, model_id, chunk_hash) VALUES ('a.md#0-9', 'm', X'00')",
                [],
            )
            .unwrap();
        }

        store
            .replace_derived_for_file(&id, &[chunk("a.md", 0, 9, Some("Guide"), "body")])
            .unwrap();

        assert_eq!(count(&store, "embeddings"), 1, "the vectors were thrown away");
        assert_eq!(
            store.load_all_chunks().unwrap()[0].qualified_name.as_deref(),
            Some("Guide"),
            "the name was not rewritten"
        );
    }

    /// The same path through `replace_chunks_for_file` would have cascaded
    /// them away — which is exactly why there are two methods.
    #[test]
    fn the_ordinary_path_does_throw_the_vectors_away() {
        let (store, _dir) = store("store-derived-contrast");
        store.upsert_files(&[file("a.md", "x")]).unwrap();
        let id = FileId("a.md".into());
        store.replace_chunks_for_file(&id, &[chunk("a.md", 0, 9, None, "body")]).unwrap();
        {
            let conn = store.lock().unwrap();
            conn.execute(
                "INSERT INTO embeddings (chunk_id, model_id, chunk_hash) VALUES ('a.md#0-9', 'm', X'00')",
                [],
            )
            .unwrap();
        }

        store.replace_chunks_for_file(&id, &[chunk("a.md", 0, 9, None, "body")]).unwrap();

        assert_eq!(count(&store, "embeddings"), 0);
    }

    /// A file edited since the last sync rebuilds to different byte ranges,
    /// so its chunks no longer match what is stored. Inserting the full-text
    /// row anyway would leave one nothing points at — unfindable, and so
    /// undeletable, matching searches forever.
    #[test]
    fn a_rebuilt_chunk_that_matches_nothing_leaves_no_orphan_behind() {
        let (store, _dir) = store("store-derived-orphan");
        store.upsert_files(&[file("a.md", "x")]).unwrap();
        let id = FileId("a.md".into());
        store.replace_chunks_for_file(&id, &[chunk("a.md", 0, 9, None, "body")]).unwrap();

        store
            .replace_derived_for_file(&id, &[chunk("a.md", 0, 40, Some("Moved"), "body moved")])
            .unwrap();

        assert_eq!(count(&store, "chunks_fts"), 0, "an unreachable full-text row was left behind");
        assert_eq!(count(&store, "chunks"), 1, "the chunk row itself was touched");
    }

    // ------------------------------------------------------------- symbols

    fn symbol(name: &str, start: u32) -> Symbol {
        Symbol {
            name: name.to_string(),
            start_line: 0,
            end_line: 1,
            start_byte: start,
            end_byte: start + 1,
        }
    }

    #[test]
    fn symbols_round_trip_grouped_by_their_file() {
        let (store, _dir) = store("store-symbols");
        store.upsert_files(&[file("a.md", "x"), file("b.md", "y")]).unwrap();
        store
            .replace_symbols_for_file(&FileId("a.md".into()), &[symbol("One", 0), symbol("Two", 10)])
            .unwrap();
        store
            .replace_symbols_for_file(&FileId("b.md".into()), &[symbol("Other", 0)])
            .unwrap();

        let loaded = store.load_all_symbols().unwrap();
        assert_eq!(loaded[&FileId("a.md".into())], vec![symbol("One", 0), symbol("Two", 10)]);
        assert_eq!(loaded[&FileId("b.md".into())].len(), 1);
    }

    /// Re-parsing a file replaces what was found in it. Symbols have no id of
    /// their own, so a stale row cannot be told from a fresh one afterwards.
    #[test]
    fn re_parsing_a_file_replaces_its_symbols() {
        let (store, _dir) = store("store-symbols-replace");
        store.upsert_files(&[file("a.md", "x")]).unwrap();
        let id = FileId("a.md".into());

        store.replace_symbols_for_file(&id, &[symbol("Old", 0), symbol("Gone", 5)]).unwrap();
        store.replace_symbols_for_file(&id, &[symbol("New", 0)]).unwrap();

        assert_eq!(store.load_all_symbols().unwrap()[&id], vec![symbol("New", 0)]);
    }

    /// A file with nothing in it has no symbols, and saying so has to clear
    /// what the previous parse found — otherwise deleting the last heading
    /// from a document leaves it in the index forever.
    #[test]
    fn a_file_that_lost_its_symbols_has_them_removed() {
        let (store, _dir) = store("store-symbols-empty");
        store.upsert_files(&[file("a.md", "x")]).unwrap();
        let id = FileId("a.md".into());

        store.replace_symbols_for_file(&id, &[symbol("Heading", 0)]).unwrap();
        store.replace_symbols_for_file(&id, &[]).unwrap();

        assert!(store.load_all_symbols().unwrap().is_empty());
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

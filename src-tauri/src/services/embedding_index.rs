//! Vectors for the chunks the lexical index already holds, and search over
//! them by meaning.
//!
//! ## No vector library
//!
//! Upstream used `usearch` — an HNSW graph in C++, built through `cxx` — with
//! the vectors in a file of its own beside the SQLite store. Here the vectors
//! are rows of the store (`IndexStore::upsert_embeddings`), loaded into memory
//! once, and a search is a scan of all of them.
//!
//! A scan sounds like the thing to avoid and is not, at this size: a dot
//! product of 256 bytes against a 256-float query per chunk. Measured
//! (release, Apple silicon, one thread): **11 ms a query over 100 000
//! chunks** — a repository four times the size of upstream's own, which is
//! 7 797 chunks. This one is 2 013. In exchange there is no C++
//! toolchain in the Windows build, no graph to rebuild after a delete, no
//! second file to keep in step with the store, and recall is exact rather
//! than approximate.
//!
//! And the int8 vectors rank like the exact ones: over twelve queries on two
//! repositories, 9.7–10 of the top 10 were the same as with `f32` vectors.
//!
//! `ponytail:` a flat scan, linear in the chunk count — ~110 ms at a million.
//! Past that, or under a tighter query budget, an approximate index earns its
//! keep.
//!
//! ## What gets a vector
//!
//! Every chunk. Upstream kept a second axis — `Language::is_lexical_only` —
//! because each vector cost a call to a paid remote model, so only prose was
//! embedded. With the bundled model a whole repository embeds in well under a
//! second, and code is exactly what a coding agent searches, so the axis has
//! nothing left to decide.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use thiserror::Error;

use crate::domain::chunk_index::{ChunkId, ChunkMetadata};
use crate::domain::embeddings::{EmbeddingError, EmbeddingProvider, QuantizedVector};
use crate::domain::repo_index::FileId;
use crate::infra::index_store::{IndexStore, IndexStoreError};

/// Texts per call to the model, and per transaction in the store. Also how
/// often progress is reported.
const EMBED_BATCH: usize = 64;

#[derive(Debug, Error)]
pub enum EmbedSyncError {
    #[error(transparent)]
    Model(#[from] EmbeddingError),
    #[error(transparent)]
    Store(#[from] IndexStoreError),
    #[error("the model returned {got} vectors for {expected} texts")]
    WrongCount { expected: usize, got: usize },
}

/// What one sync did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct EmbedStats {
    pub embedded: usize,
    pub unchanged: usize,
    pub removed: usize,
    /// Files whose bytes no longer match the chunks cut from them — edited
    /// after the lexical sync. Their chunks are left for the next round,
    /// after the lexical sync has re-cut them; embedding the new text under
    /// the old ranges would store a vector for text the chunk does not hold.
    pub stale_files: usize,
}

/// One model's vectors, in memory, mirroring the store.
pub struct EmbeddingIndex {
    model_id: String,
    dimensions: usize,
    entries: HashMap<ChunkId, (blake3::Hash, QuantizedVector)>,
}

impl EmbeddingIndex {
    /// Everything `model_id` has already embedded, from the store.
    pub fn load(store: &IndexStore, model_id: &str, dimensions: usize) -> Result<Self, IndexStoreError> {
        let entries = store
            .load_all_embeddings(model_id, dimensions)?
            .into_iter()
            .map(|(id, hash, vector)| (id, (hash, vector)))
            .collect();
        Ok(Self { model_id: model_id.to_string(), dimensions, entries })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Embeds every chunk of the store that has no vector for its current
    /// hash, and forgets vectors of chunks that are gone.
    ///
    /// Each batch is written to the store before the next is embedded, so an
    /// interrupted sync keeps what it finished and the next one picks up the
    /// rest. That is the opposite of upstream, which marked vectors as done
    /// batch by batch but saved them only at the end (B-11).
    ///
    /// `on_progress(done, total)` after every batch, over the chunks that
    /// needed embedding this time.
    pub fn sync(
        &mut self,
        root: &Path,
        store: &IndexStore,
        provider: &dyn EmbeddingProvider,
        on_progress: &mut dyn FnMut(usize, usize),
    ) -> Result<EmbedStats, EmbedSyncError> {
        let chunks = store.load_all_chunks()?;
        let mut stats = EmbedStats::default();

        // Vectors of chunks that no longer exist. The store dropped their rows
        // already, through the cascade from `chunks`; this is the in-memory
        // half.
        let current: HashSet<&ChunkId> = chunks.iter().map(|c| &c.id).collect();
        let before = self.entries.len();
        self.entries.retain(|id, _| current.contains(id));
        stats.removed = before - self.entries.len();

        // Grouped by file so each file is read and hashed once, not once per
        // chunk.
        let mut pending: HashMap<&FileId, Vec<&ChunkMetadata>> = HashMap::new();
        for chunk in &chunks {
            if self.entries.get(&chunk.id).is_some_and(|(hash, _)| *hash == chunk.hash) {
                stats.unchanged += 1;
            } else {
                pending.entry(&chunk.file_id).or_default().push(chunk);
            }
        }

        let mut texts: Vec<(&ChunkMetadata, String)> = Vec::new();
        for (file_id, file_chunks) in pending {
            let Ok(bytes) = fs::read(root.join(&file_id.0)) else {
                stats.stale_files += 1;
                continue;
            };
            if blake3::hash(&bytes) != file_chunks[0].file_hash {
                stats.stale_files += 1;
                continue;
            }
            for chunk in file_chunks {
                let text = bytes
                    .get(chunk.start_byte as usize..chunk.end_byte as usize)
                    .and_then(|slice| std::str::from_utf8(slice).ok());
                if let Some(text) = text {
                    texts.push((chunk, text.to_string()));
                }
            }
        }

        let total = texts.len();
        for batch in texts.chunks(EMBED_BATCH) {
            let inputs: Vec<&str> = batch.iter().map(|(_, text)| text.as_str()).collect();
            let vectors = provider.embed(&inputs)?;
            if vectors.len() != batch.len() {
                return Err(EmbedSyncError::WrongCount { expected: batch.len(), got: vectors.len() });
            }
            let rows: Vec<(ChunkId, blake3::Hash, QuantizedVector)> = batch
                .iter()
                .zip(vectors)
                .map(|((chunk, _), vector)| (chunk.id.clone(), chunk.hash, QuantizedVector::quantize(&vector.0)))
                .collect();
            store.upsert_embeddings(&self.model_id, &rows)?;
            stats.embedded += rows.len();
            for (id, hash, vector) in rows {
                self.entries.insert(id, (hash, vector));
            }
            on_progress(stats.embedded, total);
        }
        Ok(stats)
    }

    /// The `top_k` chunks nearest `query`, best first, with their cosine
    /// similarity — higher is better, like every other tier's score.
    ///
    /// `query` must be a unit vector from the same model; a query of the
    /// wrong width finds nothing rather than a ranking of garbage.
    pub fn search(&self, query: &[f32], top_k: usize) -> Vec<(ChunkId, f32)> {
        if query.len() != self.dimensions || top_k == 0 {
            return Vec::new();
        }
        let mut scored: Vec<(f32, &ChunkId)> =
            self.entries.iter().map(|(id, (_, vector))| (vector.dot(query), id)).collect();
        let by_score_desc = |a: &(f32, &ChunkId), b: &(f32, &ChunkId)| b.0.total_cmp(&a.0).then_with(|| a.1 .0.cmp(&b.1 .0));
        if scored.len() > top_k {
            scored.select_nth_unstable_by(top_k - 1, by_score_desc);
            scored.truncate(top_k);
        }
        scored.sort_by(by_score_desc);
        scored.into_iter().map(|(score, id)| (id.clone(), score)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chunk_index::ChunkBuildOptions;
    use crate::domain::embeddings::Embedding;
    use crate::services::repo_index;
    use crate::testing::temp_dir;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::path::PathBuf;

    const DIMS: usize = 32;

    /// Bag of words hashed into 32 buckets, at unit length: texts sharing
    /// words point the same way. Enough meaning to rank by, none of the
    /// weights. Counts its calls and can be told to fail from a given one.
    struct FakeModel {
        calls: AtomicUsize,
        fail_from_call: Option<usize>,
    }

    impl FakeModel {
        fn new() -> Self {
            Self { calls: AtomicUsize::new(0), fail_from_call: None }
        }
        fn vector(text: &str) -> Vec<f32> {
            let mut v = vec![0.0_f32; DIMS];
            for word in text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
                let bucket = blake3::hash(word.to_lowercase().as_bytes()).as_bytes()[0] as usize % DIMS;
                v[bucket] += 1.0;
            }
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            v.iter().map(|x| x / norm).collect()
        }
    }

    impl EmbeddingProvider for FakeModel {
        fn embed(&self, texts: &[&str]) -> Result<Vec<Embedding>, EmbeddingError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_from_call.is_some_and(|from| call >= from) {
                return Err(EmbeddingError::Invalid("the fake model gave up".into()));
            }
            Ok(texts.iter().map(|t| Embedding(Self::vector(t))).collect())
        }
        fn dimensions(&self) -> usize {
            DIMS
        }
    }

    struct Fixture {
        root: PathBuf,
        store: IndexStore,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            Self {
                root: temp_dir(&format!("{label}-repo")),
                store: IndexStore::open(&temp_dir(&format!("{label}-store"))).unwrap(),
            }
        }
        fn write(&self, path: &str, content: &str) {
            fs::write(self.root.join(path), content).unwrap();
        }
        fn lexical(&self) {
            repo_index::sync(&self.root, &self.store, &ChunkBuildOptions::default()).unwrap();
        }
        fn index(&self) -> EmbeddingIndex {
            EmbeddingIndex::load(&self.store, "fake", DIMS).unwrap()
        }
        fn embed(&self, index: &mut EmbeddingIndex, model: &FakeModel) -> Result<EmbedStats, EmbedSyncError> {
            index.sync(&self.root.canonicalize().unwrap(), &self.store, model, &mut |_, _| {})
        }
    }

    fn top_file(index: &EmbeddingIndex, query: &str) -> String {
        let hits = index.search(&FakeModel::vector(query), 1);
        hits[0].0 .0.split('#').next().unwrap().to_string()
    }

    // ------------------------------------------------------------ syncing

    #[test]
    fn a_first_sync_embeds_every_chunk_and_a_second_embeds_none() {
        let f = Fixture::new("embed-first");
        f.write("store.rs", "fn open_store() {}\nfn close_store() {}\n");
        f.write("notes.md", "# Keyring\n\nfallback file\n");
        f.lexical();
        let mut index = f.index();
        let model = FakeModel::new();

        let first = f.embed(&mut index, &model).unwrap();
        assert_eq!(first.embedded, f.store.load_all_chunks().unwrap().len());
        assert_eq!(index.len(), first.embedded);

        let calls = model.calls.load(Ordering::SeqCst);
        let second = f.embed(&mut index, &model).unwrap();
        assert_eq!((second.embedded, second.unchanged), (0, first.embedded));
        assert_eq!(model.calls.load(Ordering::SeqCst), calls, "the model was asked again for nothing");
    }

    /// The reason for hashes: an edit re-embeds the chunks of that file and
    /// nothing else.
    #[test]
    fn an_edit_re_embeds_only_that_files_chunks() {
        let f = Fixture::new("embed-edit");
        f.write("a.rs", "fn alpha() {}\n");
        f.write("b.rs", "fn beta() {}\nfn gamma() {}\n");
        f.lexical();
        let mut index = f.index();
        let model = FakeModel::new();
        f.embed(&mut index, &model).unwrap();

        // Same length on purpose: the chunk keeps its id (`a.rs#0-14`) and
        // only its hash says it changed. A longer edit would mint a new id and
        // be embedded as a new chunk, never testing the hash at all.
        std::thread::sleep(std::time::Duration::from_millis(1100)); // a new mtime second
        f.write("a.rs", "fn omega() {}\n");
        f.lexical();
        let stats = f.embed(&mut index, &model).unwrap();

        assert_eq!(stats.embedded, 1);
        assert_eq!(stats.unchanged, 2);
    }

    #[test]
    fn a_deleted_files_vectors_are_forgotten() {
        let f = Fixture::new("embed-delete");
        f.write("keep.rs", "fn keep() {}\n");
        f.write("gone.rs", "fn unique_vanishing_word() {}\n");
        f.lexical();
        let mut index = f.index();
        f.embed(&mut index, &FakeModel::new()).unwrap();

        fs::remove_file(f.root.join("gone.rs")).unwrap();
        f.lexical();
        let stats = f.embed(&mut index, &FakeModel::new()).unwrap();

        assert_eq!(stats.removed, 1);
        assert_eq!(index.len(), 1);
        assert_eq!(f.index().len(), 1, "the store kept the vector of a deleted file");
    }

    /// Edited after the lexical sync: the stored ranges describe bytes that
    /// are no longer there. Embedding the new text under them would store a
    /// vector for text the chunk does not hold.
    #[test]
    fn a_file_changed_since_it_was_chunked_is_left_for_the_next_round() {
        let f = Fixture::new("embed-stale");
        f.write("a.rs", "fn alpha() {}\n");
        f.lexical();
        f.write("a.rs", "fn totally_different() {}\n");

        let mut index = f.index();
        let stats = f.embed(&mut index, &FakeModel::new()).unwrap();

        assert_eq!((stats.embedded, stats.stale_files), (0, 1));
        assert!(index.is_empty());
    }

    /// B-11, fixed: every finished batch is in the store before the next is
    /// asked for. A sync that dies halfway keeps the first half, and the next
    /// one embeds only the rest.
    #[test]
    fn an_interrupted_sync_keeps_what_it_finished() {
        let f = Fixture::new("embed-interrupted");
        let body: String = (0..150).map(|i| format!("fn handler_{i}() {{}}\n")).collect();
        f.write("many.rs", &body);
        f.lexical();
        let total = f.store.load_all_chunks().unwrap().len();
        assert!(total > EMBED_BATCH, "the fixture must span batches");

        let failing = FakeModel { calls: AtomicUsize::new(0), fail_from_call: Some(1) };
        assert!(f.embed(&mut f.index(), &failing).is_err());
        assert_eq!(f.index().len(), EMBED_BATCH, "the finished batch was not kept");

        let mut resumed = f.index();
        let stats = f.embed(&mut resumed, &FakeModel::new()).unwrap();
        assert_eq!((stats.embedded, stats.unchanged), (total - EMBED_BATCH, EMBED_BATCH));
    }

    #[test]
    fn a_model_that_returns_the_wrong_count_is_an_error() {
        struct Short;
        impl EmbeddingProvider for Short {
            fn embed(&self, texts: &[&str]) -> Result<Vec<Embedding>, EmbeddingError> {
                Ok(texts.iter().skip(1).map(|_| Embedding(vec![1.0; DIMS])).collect())
            }
            fn dimensions(&self) -> usize {
                DIMS
            }
        }
        let f = Fixture::new("embed-short");
        f.write("a.rs", "fn a() {}\nfn b() {}\n");
        f.lexical();

        let result = f.index().sync(&f.root.canonicalize().unwrap(), &f.store, &Short, &mut |_, _| {});
        assert!(matches!(result, Err(EmbedSyncError::WrongCount { expected: 2, got: 1 })), "{result:?}");
    }

    #[test]
    fn progress_counts_up_to_what_needed_embedding() {
        let f = Fixture::new("embed-progress");
        let body: String = (0..100).map(|i| format!("fn f{i}() {{}}\n")).collect();
        f.write("a.rs", &body);
        f.lexical();

        let mut seen = Vec::new();
        f.index()
            .sync(&f.root.canonicalize().unwrap(), &f.store, &FakeModel::new(), &mut |done, total| seen.push((done, total)))
            .unwrap();

        assert_eq!(seen, [(64, 100), (100, 100)]);
    }

    // ----------------------------------------------------------- searching

    #[test]
    fn search_ranks_by_meaning_best_first() {
        let f = Fixture::new("embed-search");
        f.write("keyring.md", "# Keyring\n\nthe keyring fallback file stores the master key\n");
        f.write("compact.md", "# Compaction\n\nthe history is compacted when the context window fills\n");
        f.lexical();
        let mut index = f.index();
        f.embed(&mut index, &FakeModel::new()).unwrap();

        assert_eq!(top_file(&index, "context window history"), "compact.md");
        assert_eq!(top_file(&index, "master key fallback"), "keyring.md");

        let hits = index.search(&FakeModel::vector("context window history"), 5);
        assert!(hits.windows(2).all(|w| w[0].1 >= w[1].1), "not best first: {hits:?}");
        assert!(hits[0].1 > 0.0, "a similarity, higher is better");
    }

    /// What the store holds is what a restart searches.
    #[test]
    fn a_reloaded_index_searches_the_same() {
        let f = Fixture::new("embed-reload");
        f.write("a.md", "# Alpha\n\nkeyring master key\n");
        f.write("b.md", "# Beta\n\ncontext window history\n");
        f.lexical();
        let mut index = f.index();
        f.embed(&mut index, &FakeModel::new()).unwrap();

        let query = FakeModel::vector("context window");
        assert_eq!(f.index().search(&query, 2), index.search(&query, 2));
    }

    #[test]
    fn search_returns_at_most_top_k_and_nothing_for_a_wrong_width() {
        let f = Fixture::new("embed-topk");
        let body: String = (0..20).map(|i| format!("fn f{i}() {{}}\n")).collect();
        f.write("a.rs", &body);
        f.lexical();
        let mut index = f.index();
        f.embed(&mut index, &FakeModel::new()).unwrap();

        assert_eq!(index.search(&FakeModel::vector("f1"), 3).len(), 3);
        assert!(index.search(&[1.0; 8], 3).is_empty());
        assert!(index.search(&FakeModel::vector("f1"), 0).is_empty());
    }

    /// The partial sort must pick the same top as a full sort would.
    #[test]
    fn the_top_k_is_the_true_top_k() {
        let f = Fixture::new("embed-true-top");
        let body: String = (0..200).map(|i| format!("fn word{} other{}() {{}}\n", i % 7, i % 11)).collect();
        f.write("a.rs", &body);
        f.lexical();
        let mut index = f.index();
        f.embed(&mut index, &FakeModel::new()).unwrap();

        let query = FakeModel::vector("word3 other5");
        let all = index.search(&query, usize::MAX);
        assert_eq!(index.search(&query, 10), all[..10].to_vec());
    }
}

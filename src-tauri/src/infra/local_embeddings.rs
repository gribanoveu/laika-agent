//! The bundled Model2Vec model: `potion-multilingual-128M`, quantized to int8
//! and cut down to Russian and English (F-5.8b).
//!
//! A *static* model: a vector per token, averaged. No network, no ONNX
//! runtime, no GPU — embedding a chunk is a tokenizer pass and a lookup, which
//! is what makes indexing a whole repository on a laptop cheap enough to do on
//! open. Multilingual because questions here arrive in Russian about code
//! written in English.
//!
//! The weights ship with the app (`tauri.conf.json` → `bundle.resources`) and
//! live in the repository under Git LFS; `scripts/embedding-model.md` says how
//! they are built and checked.
//!
//! ## Only Russian and English — a deliberate limit
//!
//! The upstream model's vocabulary is 500 353 tokens in a hundred languages.
//! `scripts/prune-embedding-vocab.py` keeps the 271 538 whose letters are all
//! ASCII or Cyrillic, and the matching rows of the table. For text in
//! Russian, English and code **nothing changes**: a dropped token contains a
//! letter from another script, so it can never match such text, and the
//! tokenizer picks exactly the same pieces. Checked over two repositories,
//! 10 957 fragments: identical tokenization everywhere except the six that
//! contained `é`, `à`, `µ`, `Σ` or `世界`. Those characters now embed as
//! nothing. Going back to the full model is the build script without the
//! prune step, the directory name below, and a new `LOCAL_MODEL_ID`.
//!
//! ## Cost — measured, not estimated (release, macOS, 2026-09-18)
//!
//! | | full model | Russian + English |
//! |---|---|---|
//! | on disk | 146 MB | 80 MB |
//! | resident once loaded | ~1.07 GB | **~520 MB** |
//! | load | 0.7 s | 0.42 s |
//!
//! Resident is the tokenizer — a Unigram prefix tree with a node per character
//! of every token, about as large as the table — plus the table, which
//! `model2vec-rs` widens from int8 to `f32` (rows × 256 × 4 bytes). Upstream's
//! notes said 512 MB for the full model: that was the table alone.
//!
//! Embedding is cheap once loaded: all 1 911 chunks of this repository in
//! 150 ms.
//!
//! So the model is **loaded on first use and unloaded when idle**: after
//! [`DEFAULT_IDLE_UNLOAD`] with no embedding, a background thread drops it and
//! hands the freed memory back to the OS. The next embed pays the 0.7 s load
//! again. A project nobody searches semantically never pays at all.
//!
//! Dropping is not enough on its own. The tokenizer is millions of small
//! allocations, and the allocator keeps freed small blocks for reuse: measured
//! on the full model, a dropped one still left **375 MB** resident.
//! [`release_freed_memory`] asks the allocator to return them — down to ~50 MB.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use model2vec_rs::model::StaticModel;

use crate::domain::embeddings::{Embedding, EmbeddingError, EmbeddingProvider};

pub const DIMENSIONS: usize = 256;
const MODEL_DIR_NAME: &str = "potion-multilingual-128M-int8-ru-en";
const LFS_POINTER_PREFIX: &[u8] = b"version https://git-lfs.github.com/spec/v1";

/// Tokens per text before the rest is cut off — `model2vec`'s own default,
/// and the one `parity.json` was computed with. A static model has no
/// position limit, so this is about meaning, not capacity: the mean of four
/// thousand token vectors says less than the mean of the first five hundred.
/// A median chunk here is ~470 bytes and well inside it.
const MAX_TOKENS: usize = 512;
const BATCH_SIZE: usize = 512;

/// Where the bundled model is: under the app's resource directory in a built
/// app, and under this crate in development and tests.
///
/// The resource directory comes from the caller — Tauri knows it, `infra`
/// does not ask Tauri. Upstream guessed it from `current_exe()` with three
/// layouts in mind, none of them Linux's (`/usr/lib/<app>`).
pub fn bundled_model_dir(resource_dir: Option<&Path>) -> PathBuf {
    resource_dir
        .map(|dir| dir.join("models").join(MODEL_DIR_NAME))
        .filter(|dir| dir.join("model.safetensors").is_file())
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("models").join(MODEL_DIR_NAME))
}

/// How long the model stays loaded after its last use. Long enough that a
/// sync followed by a few searches loads it once; short enough that a window
/// left open overnight is not holding a gigabyte.
pub const DEFAULT_IDLE_UNLOAD: Duration = Duration::from_secs(10 * 60);

/// The bundled model behind a lazy, self-unloading slot.
///
/// **Construct one per process** — the workspace index owns it. Each instance
/// has its own slot, so two would be able to hold two copies.
pub struct LocalEmbeddings {
    dir: PathBuf,
    cell: IdleCell<StaticModel>,
}

impl LocalEmbeddings {
    /// Nothing is read until the first [`embed`](EmbeddingProvider::embed).
    /// Constructing this on every file save is free — which is what "must not
    /// lose" item 2 of stage 5 asks of the incremental tick.
    pub fn new(dir: PathBuf, idle: Duration) -> Self {
        Self { dir, cell: IdleCell::new(idle) }
    }

    /// Whether the weights are in memory right now.
    pub fn is_loaded(&self) -> bool {
        self.cell.is_loaded()
    }
}

/// A value loaded on first use and dropped after `idle` without use.
///
/// Generic only so its tests can run on a number instead of a gigabyte of
/// weights; the model is the one thing that lives in it.
struct IdleCell<T> {
    idle: Duration,
    slot: Arc<Mutex<Slot<T>>>,
}

struct Slot<T> {
    value: Option<Arc<T>>,
    last_used: Option<Instant>,
    reaper_running: bool,
}

impl<T: Send + Sync + 'static> IdleCell<T> {
    fn new(idle: Duration) -> Self {
        Self {
            idle,
            slot: Arc::new(Mutex::new(Slot { value: None, last_used: None, reaper_running: false })),
        }
    }

    fn is_loaded(&self) -> bool {
        self.slot.lock().is_ok_and(|slot| slot.value.is_some())
    }

    /// The value, loading it if needed and starting the reaper that will drop
    /// it. A failed load is not remembered: the next call tries again, so a
    /// missing model starts working after `git lfs pull` without a restart.
    ///
    /// The lock is held only to swap the `Arc`, never while the caller uses
    /// the value: an unload during an embed drops the slot's reference, and
    /// the embed finishes on its own.
    fn get_or_load(&self, load: impl FnOnce() -> Result<T, EmbeddingError>) -> Result<Arc<T>, EmbeddingError> {
        let mut slot = self.slot.lock().map_err(|_| EmbeddingError::Invalid("model slot poisoned".into()))?;
        let value = match &slot.value {
            Some(value) => Arc::clone(value),
            None => {
                let value = Arc::new(load()?);
                slot.value = Some(Arc::clone(&value));
                value
            }
        };
        slot.last_used = Some(Instant::now());
        if !slot.reaper_running {
            slot.reaper_running = true;
            spawn_reaper(Arc::downgrade(&self.slot), self.idle);
        }
        Ok(value)
    }
}

/// Wakes a few times per idle period and unloads once the value has not been
/// used for a whole one. Holds the slot weakly, so dropping the cell ends the
/// thread at its next wake; exits after an unload, and the next load starts a
/// new one.
fn spawn_reaper<T: Send + Sync + 'static>(slot: Weak<Mutex<Slot<T>>>, idle: Duration) {
    let tick = (idle / 4).max(Duration::from_millis(10));
    thread::spawn(move || loop {
        thread::sleep(tick);
        let Some(slot) = slot.upgrade() else { return };
        let Ok(mut guard) = slot.lock() else { return };
        if guard.last_used.is_some_and(|at| at.elapsed() >= idle) {
            guard.value = None;
            guard.reaper_running = false;
            drop(guard);
            release_freed_memory();
            return;
        }
    });
}

/// Asks the allocator to hand freed memory back to the OS.
///
/// Without it, an unloaded model left 375 MB resident — the tokenizer's small
/// blocks, kept by the allocator for reuse. Best effort: a no-op where there
/// is no such call. On Windows the process heap has none that decommits, and
/// the unload returns only what was allocated in large blocks (the 512 MB
/// table), not the tokenizer.
fn release_freed_memory() {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
        }
        // SAFETY: a null zone means "all zones" and a zero goal means "as much
        // as possible"; the call only returns free pages and touches no live
        // allocation.
        unsafe {
            malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
        }
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        extern "C" {
            fn malloc_trim(pad: usize) -> std::ffi::c_int;
        }
        // SAFETY: glibc's own call; trims free memory from the heap top and
        // arenas, and touches no live allocation.
        unsafe {
            malloc_trim(0);
        }
    }
}

impl EmbeddingProvider for LocalEmbeddings {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Embedding>, EmbeddingError> {
        let model = self.cell.get_or_load(|| load_model(&self.dir))?;
        let sentences: Vec<String> = texts.iter().map(|text| (*text).to_string()).collect();
        Ok(model
            .encode_with_args(&sentences, Some(MAX_TOKENS), BATCH_SIZE)
            .into_iter()
            .map(Embedding)
            .collect())
    }

    fn dimensions(&self) -> usize {
        DIMENSIONS
    }
}

fn load_model(dir: &Path) -> Result<StaticModel, EmbeddingError> {
    let tokenizer = read_model_file(&dir.join("tokenizer.json"))?;
    let weights = read_model_file(&dir.join("model.safetensors"))?;
    let config = read_model_file(&dir.join("config.json"))?;
    let model = StaticModel::from_bytes(&tokenizer, &weights, &config, None)
        .map_err(|e| EmbeddingError::Invalid(e.to_string()))?;

    // The crate exposes no dimension getter, and every vector already stored
    // is `DIMENSIONS` wide. Weights swapped for another model's must be
    // refused here, not discovered as a length mismatch deep in the vector
    // store.
    let width = model.encode_single("dimension check").len();
    if width != DIMENSIONS {
        return Err(EmbeddingError::Invalid(format!(
            "vectors are {width} wide, the index expects {DIMENSIONS}"
        )));
    }
    Ok(model)
}

fn read_model_file(path: &Path) -> Result<Vec<u8>, EmbeddingError> {
    if !path.is_file() {
        return Err(EmbeddingError::NotFound(path.to_path_buf()));
    }
    let bytes = fs::read(path).map_err(|e| EmbeddingError::Unreadable {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    if bytes.starts_with(LFS_POINTER_PREFIX) {
        return Err(EmbeddingError::LfsPointer(path.to_path_buf()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (norm(a) * norm(b)).max(1e-12)
    }

    /// One loaded model for the whole suite: a gigabyte per test would be the
    /// slowest thing in it.
    fn model() -> &'static LocalEmbeddings {
        static SHARED: std::sync::OnceLock<LocalEmbeddings> = std::sync::OnceLock::new();
        SHARED.get_or_init(|| LocalEmbeddings::new(bundled_model_dir(None), DEFAULT_IDLE_UNLOAD))
    }

    fn embed(provider: &LocalEmbeddings, text: &str) -> Vec<f32> {
        provider.embed(&[text]).unwrap().remove(0).0
    }

    /// Rust and Python must agree on the vectors, or an index built by one is
    /// searched with the other's idea of meaning. `parity.json` was written
    /// by `scripts/build-embedding-model.py` together with the weights.
    ///
    /// Upstream returned early — passing — when the weights were missing or
    /// an LFS pointer. Here that fails: a green run that checked nothing is
    /// how a clone without `git lfs pull` ships an app that cannot index.
    #[test]
    fn rust_matches_the_python_parity_fixture() {
        let raw = fs::read_to_string(bundled_model_dir(None).join("parity.json")).unwrap();
        let fixture: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let phrases: Vec<&str> = fixture["phrases"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        let expected: Vec<Vec<f32>> = fixture["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_array().unwrap().iter().map(|n| n.as_f64().unwrap() as f32).collect())
            .collect();

        let got = model().embed(&phrases).unwrap_or_else(|e| panic!("the bundled model must load in tests: {e}"));
        assert_eq!(got.len(), expected.len());
        for ((phrase, got), expected) in phrases.iter().zip(&got).zip(&expected) {
            assert_eq!(got.0.len(), DIMENSIONS);
            let similarity = cosine(&got.0, expected);
            assert!(similarity >= 0.999, "{phrase:?}: cosine {similarity}");
            // Cosine cannot see length, and the fixture's vectors are unit
            // length. A store that scores by dot product would rank by norm
            // if normalisation were lost.
            let norm = got.0.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((norm - 1.0).abs() < 1e-3, "{phrase:?}: norm {norm}");
        }
    }

    /// The limit is part of the shipped model, not a property of the script
    /// that made it: a rebuild that forgot the prune step would ship twice the
    /// memory and nobody would notice until a user did.
    #[test]
    fn the_shipped_vocabulary_is_russian_and_english_only() {
        let raw = fs::read_to_string(bundled_model_dir(None).join("tokenizer.json")).unwrap();
        let tokenizer: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let vocab: Vec<&str> = tokenizer["model"]["vocab"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry[0].as_str())
            .collect();

        // The full vocabulary is 500 353 tokens; the cut one 271 538.
        assert!((200_000..300_000).contains(&vocab.len()), "{} tokens: was the prune step skipped?", vocab.len());
        for foreign in ["世界", "é", "Σ", "ß"] {
            assert!(!vocab.iter().any(|piece| piece.contains(foreign)), "{foreign:?} is still in the vocabulary");
        }
        for kept in ["▁context", "▁контекст", "{"] {
            assert!(vocab.contains(&kept), "{kept:?} was cut");
        }
    }

    /// What the model is for, in the direction this app needs: a Russian
    /// question lands nearer the English code it is about than English code
    /// it is not about.
    #[test]
    fn a_russian_question_is_nearest_the_code_it_is_about() {
        let provider = model();
        let question = embed(&provider, "как сжимается история, когда переполняется контекст");
        let about = embed(&provider, "compact the conversation history when the context window is full");
        let unrelated = embed(&provider, "render the settings dialog with a dropdown for the theme");

        assert!(cosine(&question, &about) > cosine(&question, &unrelated));
    }

    #[test]
    fn texts_come_back_in_order_one_vector_each() {
        let provider = model();
        let texts = ["alpha", "keyring fallback", ""];
        let vectors = provider.embed(&texts).unwrap();

        assert_eq!(vectors.len(), 3);
        assert_eq!(vectors[1].0, embed(&provider, "keyring fallback"));
    }

    // ------------------------------------------------------ the lifecycle

    /// Constructing is free: the incremental tick builds one on every save,
    /// and a project without semantic search must never read the weights.
    #[test]
    fn nothing_is_loaded_until_something_is_embedded() {
        let provider = LocalEmbeddings::new(temp_dir("model-lazy"), DEFAULT_IDLE_UNLOAD);
        assert!(!provider.is_loaded());
        // An empty directory: had construction loaded, it would have failed.
    }

    fn wait_until_unloaded<T: Send + Sync + 'static>(cell: &IdleCell<T>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while cell.is_loaded() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// The decision this module carries: a gigabyte is held while it is being
    /// used, and let go once it is not. The next use loads it again.
    #[test]
    fn an_idle_value_is_dropped_and_comes_back_on_the_next_use() {
        let loads = std::sync::atomic::AtomicUsize::new(0);
        let load = || {
            loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(42)
        };
        let cell = IdleCell::new(Duration::from_millis(50));

        assert_eq!(*cell.get_or_load(load).unwrap(), 42);
        assert!(cell.is_loaded());
        wait_until_unloaded(&cell);
        assert!(!cell.is_loaded(), "still loaded after the idle period");

        cell.get_or_load(load).unwrap();
        assert_eq!(loads.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(cell.is_loaded());

        // ...and the second load is dropped too: the reaper that ended with
        // the first unload must be replaced, or a reload is resident forever.
        wait_until_unloaded(&cell);
        assert!(!cell.is_loaded(), "the second load was never unloaded");
    }

    /// Use resets the clock: a value in steady use is loaded once and never
    /// dropped between two uses.
    #[test]
    fn a_value_in_use_is_loaded_once_and_stays() {
        let loads = std::sync::atomic::AtomicUsize::new(0);
        let cell = IdleCell::new(Duration::from_millis(120));
        for _ in 0..10 {
            cell.get_or_load(|| {
                loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
            .unwrap();
            thread::sleep(Duration::from_millis(30));
        }
        assert_eq!(loads.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// A missing model is an error on every embed — not a remembered one
    /// that outlives `git lfs pull`.
    #[test]
    fn a_failed_load_is_tried_again() {
        let cell: IdleCell<u8> = IdleCell::new(DEFAULT_IDLE_UNLOAD);
        let missing = || Err(EmbeddingError::NotFound(PathBuf::from("tokenizer.json")));
        assert!(cell.get_or_load(missing).is_err());
        assert!(!cell.is_loaded());
        assert_eq!(*cell.get_or_load(|| Ok(7)).unwrap(), 7);
    }

    /// Dropping the cell ends its reaper instead of leaving a thread that
    /// holds the value alive.
    #[test]
    fn a_dropped_cell_takes_its_value_with_it() {
        let cell = IdleCell::new(Duration::from_secs(3600));
        let value = cell.get_or_load(|| Ok(vec![0u8; 16])).unwrap();
        let weak = Arc::downgrade(&value);
        drop(value);
        drop(cell);
        assert!(weak.upgrade().is_none());
    }

    // ------------------------------------------------------- the files

    /// The one-command fix gets its own message.
    #[test]
    fn an_lfs_pointer_is_named_as_one() {
        let dir = temp_dir("model-lfs");
        fs::write(dir.join("tokenizer.json"), "version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 1\n").unwrap();

        let error = load_model(&dir).unwrap_err();
        assert!(matches!(error, EmbeddingError::LfsPointer(_)), "{error:?}");
        assert!(error.to_string().contains("git lfs pull"));
    }

    #[test]
    fn a_missing_model_says_which_file() {
        let dir = temp_dir("model-missing");
        let error = load_model(&dir).unwrap_err();
        assert!(matches!(error, EmbeddingError::NotFound(ref path) if path.ends_with("tokenizer.json")), "{error:?}");
    }

    #[test]
    fn files_that_are_not_a_model_are_refused() {
        let dir = temp_dir("model-garbage");
        for name in ["tokenizer.json", "model.safetensors", "config.json"] {
            fs::write(dir.join(name), "not a model").unwrap();
        }
        assert!(matches!(load_model(&dir), Err(EmbeddingError::Invalid(_))));
    }

    /// A built app finds the weights under its resources; anything else —
    /// dev, tests, a resource directory without them — the crate's copy.
    #[test]
    fn the_model_is_looked_for_under_the_resources_first() {
        let resources = temp_dir("model-resources");
        let bundled = resources.join("models").join(MODEL_DIR_NAME);
        fs::create_dir_all(&bundled).unwrap();
        fs::write(bundled.join("model.safetensors"), "x").unwrap();

        assert_eq!(bundled_model_dir(Some(&resources)), bundled);
        let fallback = Path::new(env!("CARGO_MANIFEST_DIR")).join("models").join(MODEL_DIR_NAME);
        assert_eq!(bundled_model_dir(Some(&temp_dir("model-empty"))), fallback);
        assert_eq!(bundled_model_dir(None), fallback);
    }
}

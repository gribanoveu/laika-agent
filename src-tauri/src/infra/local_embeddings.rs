//! The bundled Model2Vec model, `potion-multilingual-128M` quantized to int8.
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
//! ## Cost — measured, not estimated
//!
//! 146 MB on disk. **~1.3 GB resident once loaded** (release build, macOS,
//! 2026-09-18): the `bge-m3` tokenizer is ~450 MB in `tokenizers`' structures
//! (~600 MB at its peak while parsing), `model2vec-rs` widens the int8 table
//! to `f32` for another 512 MB (~500k rows × 256 × 4 bytes), and the allocator
//! keeps ~150 MB of the load's temporaries. Upstream's notes said 512 MB; that
//! was the table alone. Loading takes ~0.7 s.
//!
//! Embedding is cheap once loaded: all 1 911 chunks of this repository in
//! 150 ms.
//!
//! Loaded at most once per process, and only when something is actually
//! embedded — a project nobody searches semantically never pays it. There is
//! no unload: the model sits in a `OnceLock` for the life of the process.
//! Holding it in an `Arc` that is dropped when indexing goes idle is the
//! upgrade if the resident cost turns out to matter
//! (`docs/06-port-plan.md`, F-5.8).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use model2vec_rs::model::StaticModel;

use crate::domain::embeddings::{Embedding, EmbeddingError, EmbeddingProvider};

pub const DIMENSIONS: usize = 256;
const MODEL_DIR_NAME: &str = "potion-multilingual-128M-int8";
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

pub struct LocalEmbeddings {
    model: &'static StaticModel,
}

static MODEL: OnceLock<Result<StaticModel, EmbeddingError>> = OnceLock::new();

impl LocalEmbeddings {
    /// The model, loaded on first use and kept for the life of the process.
    ///
    /// One per process on purpose: two copies would be a gigabyte. The first
    /// caller's `dir` wins; there is only one bundled model, so a second
    /// caller asking for a different directory is a bug, not a use case.
    pub fn load(dir: &Path) -> Result<Self, EmbeddingError> {
        match MODEL.get_or_init(|| load_model(dir)) {
            Ok(model) => Ok(Self { model }),
            Err(error) => Err(error.clone()),
        }
    }
}

impl EmbeddingProvider for LocalEmbeddings {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Embedding>, EmbeddingError> {
        let sentences: Vec<String> = texts.iter().map(|text| (*text).to_string()).collect();
        Ok(self
            .model
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

    fn model() -> LocalEmbeddings {
        LocalEmbeddings::load(&bundled_model_dir(None))
            .unwrap_or_else(|e| panic!("the bundled model must load in tests: {e}"))
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

        let got = model().embed(&phrases).unwrap();
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

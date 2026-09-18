//! Turning text into vectors — the one model this app ships, and the seam a
//! test puts a fake behind.
//!
//! ## What upstream's 383 lines were, and why ~60 are left
//!
//! Upstream carries a second, remote provider: an OpenAI-compatible
//! `/embeddings` endpoint with its base URL, model name, dimensions, a pinned
//! CA certificate, extra request headers, a TLS-verification switch, presets,
//! a merged config and an API key in the keychain. The owner's decision for
//! this port (`docs/06-port-plan.md`, stage 5) is **the bundled model only**:
//! code and questions about it never leave the machine to be indexed. Every
//! type that existed to configure or resolve that choice has nothing to
//! choose between, and is gone with it.
//!
//! What stays is the trait. Not for a second provider — for tests: the sync
//! that fills the index must be exercised on hundreds of chunks without
//! loading half a gigabyte of weights into every test.

use std::path::PathBuf;

use thiserror::Error;

/// The bundled model's identity, and the namespace its vectors are stored
/// under (`IndexStore::vectors_path`, the `embeddings.model_id` column).
///
/// Change it whenever the weights change in a way that moves vectors: old
/// vectors then sit under a name nothing asks for, and are rebuilt rather than
/// silently compared against new ones.
///
/// `-ru-en` because the vocabulary is cut to Russian and English (F-5.8b).
/// Vectors of Russian, English and code are the same as the full model's, but
/// not for text with other scripts — so it is a different model, and says so.
pub const LOCAL_MODEL_ID: &str = "local-potion-multilingual-128m-int8-ru-en";

/// One text's vector.
#[derive(Debug, Clone, PartialEq)]
pub struct Embedding(pub Vec<f32>);

/// Why the model could not be used. Every variant is about the *files*: once
/// loaded, a static model cannot fail to embed.
///
/// Plain data, so `Clone`: a caller that reports it and keeps it can do both.
#[derive(Debug, Clone, Error)]
pub enum EmbeddingError {
    #[error("the embedding model is missing: {0} not found")]
    NotFound(PathBuf),
    /// The file is a Git LFS pointer — a clone without `git lfs pull`. Named
    /// separately because the fix is one command, and "failed to parse" would
    /// send whoever reads it looking for a corrupt download.
    #[error("{0} is a Git LFS pointer, not the file: run `git lfs pull`")]
    LfsPointer(PathBuf),
    #[error("could not read {path}: {reason}")]
    Unreadable { path: PathBuf, reason: String },
    #[error("the embedding model's files are invalid: {0}")]
    Invalid(String),
}

pub trait EmbeddingProvider: Send + Sync {
    /// One vector per text, in order.
    fn embed(&self, texts: &[&str]) -> Result<Vec<Embedding>, EmbeddingError>;
    fn dimensions(&self) -> usize;
}

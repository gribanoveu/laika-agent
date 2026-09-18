# model2vec-rs: `[UNK]` is averaged into embeddings for Unigram tokenizers

**Crate:** `model2vec-rs` 0.2.1 (latest on crates.io as of 2026-09-18); the same behaviour is
on `main` at the time of writing.
**Affected models:** every model whose tokenizer is a Unigram model — among them the
multilingual `minishlab/potion-multilingual-128M` (XLM-R / `bge-m3` tokenizer).
**Severity:** low to moderate. Vectors are silently different from the Python reference
implementation for any text that contains a character outside the vocabulary.

## Summary

`StaticModel` removes the unknown token from the token ids before mean pooling, as the
Python `model2vec` package does. To find the id of that token it asks the tokenizer model
for its `unk_token`. Unigram models do not have an `unk_token`; they store an `unk_id`.
`model2vec-rs` finds nothing, sets `unk_token_id` to `None`, and the filter never runs. The
`[UNK]` row is then averaged into the embedding like any other token.

The Python implementation handles the Unigram case explicitly and does remove `[UNK]`. So the
two implementations produce different vectors for the same model and the same text.

## Reproduction

`Cargo.toml`:

```toml
[package]
name = "m2v-repro"
version = "0.1.0"
edition = "2021"

[dependencies]
model2vec-rs = { version = "=0.2.1", default-features = false, features = ["local-only"] }
tokenizers = { version = "0.21", default-features = false, features = ["fancy-regex"] }
serde_json = "1"
```

`src/main.rs`:

```rust
use model2vec_rs::model::StaticModel;
use tokenizers::Tokenizer;

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let n = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (n(a) * n(b))
}

fn main() {
    // A local copy of minishlab/potion-multilingual-128M.
    let dir = std::env::args().nth(1).expect("model dir");
    let dir = std::path::Path::new(&dir);
    let tok_bytes = std::fs::read(dir.join("tokenizer.json")).unwrap();

    let spec: serde_json::Value = serde_json::from_slice(&tok_bytes).unwrap();
    println!("model.type      = {}", spec["model"]["type"]);
    println!("model.unk_token = {}", spec["model"]["unk_token"]);
    println!("model.unk_id    = {}", spec["model"]["unk_id"]);

    let tokenizer = Tokenizer::from_bytes(&tok_bytes).unwrap();
    let enc = tokenizer.encode("hello\u{1F9FF} world", false).unwrap();
    println!("tokens {:?} ids {:?}", enc.get_tokens(), enc.get_ids());

    let model = StaticModel::from_pretrained(dir, None, None, None).unwrap();
    let base = model.encode_single("hello world");
    let with_unk = model.encode_single("hello\u{1F9FF} world");
    println!("cosine = {:.6}", cosine(&base, &with_unk));
}
```

Output (macOS arm64, Rust 1.95, release build):

```text
model.type      = "Unigram"
model.unk_token = null
model.unk_id    = 1
tokens ["▁hello", "🧿", "▁world"] ids [250643, 1, 8997]
cosine = 0.924631
```

`🧿` (U+1F9FF) is not in the vocabulary, so it becomes `[UNK]` with id 1. After `[UNK]` is
removed, the ids are `[250643, 8997]`, the same as for `"hello world"`.

**Expected:** `cosine = 1.000000` — the two texts pool the same rows.
**Actual:** `cosine = 0.924631` — the `[UNK]` row is part of the mean.

A run of adjacent unknown characters becomes a single `[UNK]`, so `"hello🧿🧿🧿🧿 world"` gives
the same 0.924631.

## Root cause

In 0.2.1, `StaticModel::compute_metadata` (`src/model.rs`) looks the unknown token up by name
in the serialized tokenizer:

```rust
let spec: Value = serde_json::to_value(tokenizer).context("failed to serialize tokenizer")?;
let unk_token = spec
    .get("model")
    .and_then(|m| m.get("unk_token"))
    .and_then(Value::as_str);
let unk_token_id = if let Some(tok) = unk_token {
    let id = tokenizer
        .token_to_id(tok)
        .ok_or_else(|| anyhow!("unk_token '{tok}' not found in vocabulary"))?;
    Some(id as usize)
} else {
    None
};
```

`tokenizers` serializes a Unigram model as `type`, `unk_id`, `vocab` and `byte_fallback`, with
no `unk_token` field. So `unk_token_id` is `None`, and this filter in `encode_with_args` never
runs:

```rust
if let Some(unk_id) = self.unk_token_id {
    token_ids.retain(|&id| id as usize != unk_id);
}
```

On `main` the lookup is written against `ModelWrapper`. It handles BPE, WordPiece and
WordLevel, and returns `None` for Unigram on purpose:

```rust
let unk_token = match tokenizer.get_model() {
    ModelWrapper::BPE(model) => model.unk_token.as_deref(),
    ModelWrapper::WordPiece(model) => Some(model.unk_token.as_str()),
    ModelWrapper::WordLevel(model) => Some(model.unk_token.as_str()),
    ModelWrapper::Unigram(_) => None,
};
```

The result is the same.

## The Python reference does remove it

In `model2vec/model.py`, `StaticModel.__init__` sets
`self.unk_token_id = _get_unk_token_id(self.tokenizer)`, which covers Unigram explicitly:

```python
def _get_unk_token_id(tokenizer: Tokenizer) -> int | None:
    """Get the unk token id."""
    model = tokenizer.model
    # Wordpiece + BPE + Word level
    if hasattr(model, "unk_token"):
        token = model.unk_token
        if token is None:
            return None
        return tokenizer.token_to_id(token)
    # Unigram
    return json.loads(tokenizer.to_str())["model"].get("unk_id")
```

`tokenize` then removes that id from every sequence. For Unigram models the two
implementations therefore diverge. We read this from the Python source and did not run it;
the Rust behaviour above was measured.

## Impact

- **Parity.** For any text with a character outside the vocabulary, Rust and Python produce
  different vectors from the same weights. An index built with one and queried with the other
  compares slightly different embeddings.
- **Quality.** `[UNK]` carries no meaning, but its row pulls every affected text toward the
  same point. How much depends on the text length: one `[UNK]` among two tokens moved the
  vector to cosine 0.92 above. Long texts are barely affected.
- **Frequency.** With the full multilingual vocabulary (500k tokens), unknown characters are
  rare: mostly emoji and uncommon symbols. With a reduced vocabulary it happens constantly. We
  cut `potion-multilingual-128M` down to Russian and English. On two real repositories
  (10 970 text fragments of code and documentation), 17 fragments came out different from the
  same text with the unknown characters removed. The worst cosine was 0.9956.

## Suggested fix

Handle Unigram the way the Python implementation does and read `unk_id` from the serialized
model. We found no public accessor for a Unigram model's `unk_id` in `tokenizers` 0.21;
serializing is also what the Python code does.

```rust
let unk_token_id = match tokenizer.get_model() {
    ModelWrapper::BPE(model) => model.unk_token.as_deref().and_then(|t| tokenizer.token_to_id(t)),
    ModelWrapper::WordPiece(model) => tokenizer.token_to_id(&model.unk_token),
    ModelWrapper::WordLevel(model) => tokenizer.token_to_id(&model.unk_token),
    ModelWrapper::Unigram(_) => serde_json::to_value(tokenizer.get_model())
        .ok()
        .and_then(|spec| spec.get("unk_id")?.as_u64()),
}
.map(|id| id as usize);
```

(The existing error when a named `unk_token` is missing from the vocabulary can stay on the
BPE / WordPiece / WordLevel arms.)

A regression test that fails today:

```rust
#[test]
fn unigram_unknown_token_is_not_pooled() {
    let model = StaticModel::from_pretrained("minishlab/potion-multilingual-128M", None, None, None).unwrap();
    assert_eq!(
        model.encode_single("hello\u{1F9FF} world"),
        model.encode_single("hello world"),
    );
}
```

## Workaround

Until this is fixed, a caller can drop the unknown id before pooling. Reimplementing the pooling
is short: tokenize with `add_special_tokens = false`, filter out `unk_id` (read as above), take
the first `max_length` ids, sum their rows and L2-normalize. We did this downstream. We also
keep the int8 table as int8 instead of widening it to `f32`, which cut resident memory for this
model by about 200 MB. That is a separate matter from this bug, and we can report it separately
if it is of interest.

## Environment

- `model2vec-rs` 0.2.1, `tokenizers` 0.21.4 (`default-features = false`, `fancy-regex`)
- Rust 1.95.0, macOS 15 (arm64), release build
- Model: `minishlab/potion-multilingual-128M`, int8-quantized with `model2vec` itself
  (`StaticModel.from_pretrained(..., quantize_to="int8")`); the tokenizer is unchanged by
  quantization.

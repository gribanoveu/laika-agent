# Bundled local embedding model

The index embeds with Model2Vec `potion-multilingual-128M`, quantized to int8 and **cut
down to Russian and English**. It is the only embedding model: nothing is sent to a
remote service to be indexed (`docs/06-port-plan.md`, stage 5). Files live in
`src-tauri/models/potion-multilingual-128M-int8-ru-en/`, are packaged with the app through
`bundle.resources` in `tauri.conf.json`, and are loaded by
`src-tauri/src/infra/local_embeddings.rs`.

`model.safetensors` and `tokenizer.json` are Git LFS objects — `git lfs pull` after a
clone. Without it the tests fail and say so (`EmbeddingError::LfsPointer`), rather than
passing having checked nothing.

## Only Russian and English

`scripts/prune-embedding-vocab.py` keeps the 271 538 of 500 353 vocabulary entries whose
letters are all ASCII or Cyrillic, and the matching rows. **Lossless for Russian, English
and code**: a dropped token holds a letter from another script and cannot match such
text, so the tokenizer picks the same pieces. Checked on two repositories (10 957
fragments): the only differences were the six fragments containing `é`, `à`, `µ`, `Σ`,
`世界` — those characters now embed as nothing. The parity test compares the cut model
with the *full* model's Python vectors and passes.

**To return to the full model:** run `build-embedding-model.py` alone, point
`MODEL_DIR_NAME` in `local_embeddings.rs` and `bundle.resources` in `tauri.conf.json` at
`potion-multilingual-128M-int8`, and change `LOCAL_MODEL_ID`. The full weights are also in
git history at commit `4882a63`.

## Cost (measured 2026-09-18, release, macOS)

| | full | **Russian + English (shipped)** |
|---|---|---|
| On disk / in the installer | 146 MB | 80 MB |
| Resident once loaded | ~1.07 GB | **~520 MB** |
| Load | 0.7 s | 0.42 s |

Resident is the tokenizer (a Unigram prefix tree, about as large as the table) plus the
table, which `model2vec-rs` widens from int8 to `f32`. The model is unloaded after 10
minutes idle and the memory handed back (`local_embeddings.rs`). Upstream's notes said
~512 MB for the full model: that was the table alone.

## Regenerate

Needs Python `model2vec` and a Hugging Face download of
`minishlab/potion-multilingual-128M`:

```bash
pip install model2vec
python3 scripts/build-embedding-model.py
python3 scripts/prune-embedding-vocab.py
git rm -r src-tauri/models/potion-multilingual-128M-int8   # ship only the cut one
```

The first writes the full model — `model.safetensors`, `tokenizer.json`, `config.json`,
`modules.json`, `README.md` (the model card, MIT) and `parity.json` — and the second the
Russian + English one next to it (standard library only).

**The download is not pinned to a revision** — see the script's docstring. Pin it
before the next rebuild.

Do not turn on `dimensionality=128` in the script without re-running search quality
on a real repository: it halves the table's 512 MB, not the tokenizer's share.

After regenerating, confirm:

1. Cosine ≥ 0.999 on the phrases in `parity.json`:
   `cd src-tauri && cargo test rust_matches_the_python_parity_fixture`.
2. Search quality on a real repository.
3. Installer size and `cargo build --release` link time.

If the new weights move vectors, change `LOCAL_MODEL_ID` in
`src-tauri/src/domain/embeddings.rs`: old vectors then sit under a name nothing asks
for and are rebuilt, rather than compared against new ones.

# Bundled local embedding model

`Local` uses Model2Vec `potion-multilingual-128M` quantized to int8. Files live in
`src-tauri/models/potion-multilingual-128M-int8/` and are packaged with the app
(no Hugging Face download at runtime).

## Regenerate

Needs Python `model2vec` (and a Hugging Face download of `minishlab/potion-multilingual-128M`):

```bash
pip install model2vec
python3 scripts/build-embedding-model.py
```

That writes `model.safetensors`, `tokenizer.json`, `config.json`, and `parity.json`.
`safetensors` and `tokenizer.json` are Git LFS pointers — `git lfs pull` after clone.

Do **not** turn on `dimensionality=128` in the script unless you re-run quality
checks on the documentation corpus (Russian queries, top-5 vs remote bge-m3)
and the Rust/Python cosine gate in `local.rs` (`rust_matches_python_parity_fixture`).

After regenerating, confirm:

1. Cosine ≥ 0.999 on the phrases in `parity.json` (`cd src-tauri && cargo test rust_matches_python_parity_fixture`).
2. Search quality on a real repo (expect some drop vs full bge-m3; BM25 still runs).
3. `cargo build --release` link time and installer size — the weights are ~128 MB on disk, ~512 MB RSS after first local encode.
4. Release workflow with `lfs: true` still produces macOS/Windows installers.

A model id change (`local-potion-multilingual-128m-int8`) is required if the
baked weights change in a way that invalidates old vectors; the previous
namespace then stays on disk as an unused orphan.

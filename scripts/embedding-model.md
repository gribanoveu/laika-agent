# Bundled local embedding model

The index embeds with Model2Vec `potion-multilingual-128M`, quantized to int8. It is
the only embedding model: nothing is sent to a remote service to be indexed
(`docs/06-port-plan.md`, stage 5). Files live in
`src-tauri/models/potion-multilingual-128M-int8/`, are packaged with the app through
`bundle.resources` in `tauri.conf.json`, and are loaded by
`src-tauri/src/infra/local_embeddings.rs`.

`model.safetensors` and `tokenizer.json` are Git LFS objects — `git lfs pull` after a
clone. Without it the tests fail and say so (`EmbeddingError::LfsPointer`), rather than
passing having checked nothing.

## Cost (measured 2026-09-18, release, macOS)

| | |
|---|---|
| On disk / in the installer | 146 MB |
| Resident once loaded | **~1.3 GB** — tokenizer ~450 MB, `f32` table 512 MB, allocator ~150 MB |
| Load time | ~0.7 s |
| Embedding this repository (1 911 chunks) | 150 ms |

Upstream's notes said ~512 MB resident; that is the table alone.

## Regenerate

Needs Python `model2vec` and a Hugging Face download of
`minishlab/potion-multilingual-128M`:

```bash
pip install model2vec
python3 scripts/build-embedding-model.py
```

That writes `model.safetensors`, `tokenizer.json`, `config.json`, `modules.json`,
`README.md` (the model card, MIT) and `parity.json`.

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

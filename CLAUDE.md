# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Architecture, layering rules, IPC conventions, error handling and UI rules live in
[`AGENTS.md`](./AGENTS.md) — read it, it is the primary source:

@AGENTS.md

What follows is only what `AGENTS.md` does not cover.

## This repository is mid-port

The agent core is being ported here from Alfa Atlas
(`/Users/eugene/Downloads/docflow-tauri/docflow`) one feature at a time.
[`docs/06-port-plan.md`](docs/06-port-plan.md) is the running checklist: what is in,
what is next, and — for each ported module — what had to change and what must not be
lost. Read it before adding backend code, and update it when a feature lands.

Until the port reaches stage 3, `src/mock/data.ts` stands in for the backend and there
are no `#[tauri::command]`s.

## Tests

Frontend tests run on **Bun's own runner** (not vitest/jest), from `src/__tests__/`:

```bash
bun test                                  # all frontend tests
bun test src/__tests__/Dropdown.test.tsx  # one file
bun test -t "Escape"                      # one test by name pattern
```

`bunfig.toml` preloads `src/__tests__/setup/happydom.ts`, which registers the DOM
globals and auto-unmounts after each test. A test that renders components works
only through `bun test` — running the file with `bun run` gives it no DOM.
`tsconfig.json` excludes `src/__tests__/**`: Bun type-checks those itself, and `tsc`
does not know `bun:test`.

Rust tests are inline `#[cfg(test)] mod tests` blocks, ported together with the code
they cover:

```bash
cd src-tauri && cargo test              # all
cd src-tauri && cargo test resolve      # by name substring
```

Tests that load the bundled embedding model (`infra/local_embeddings.rs`) are
`#[ignore]`d: they cost seconds and most of a gigabyte per run, which a mutation run
pays once per mutant. Run them after touching the model, its loading, or embedding:

```bash
cd src-tauri && cargo test local_embeddings -- --ignored
```

The default run still checks that the weights are real files and not Git LFS pointers.

Search quality has its own bench, ignored by default because it indexes real
repositories (`src-tauri/bench/search-queries.json` lists them and the questions).
Run it after touching ranking in `services/code_search.rs`:

```bash
cd src-tauri && cargo test --release search_bench -- --ignored --nocapture
```

## Checks before done

```bash
bun run tsc --noEmit && bun test
```

```bash
cd src-tauri && cargo check && cargo test
```

## Dev server

`bun run tauri dev` runs the full app. The Vite dev server alone is registered in
`.claude/launch.json` as `vite-dev` on port 1420 — useful for previewing UI, but
every `invoke()` fails there because there is no Tauri backend.

Port 1420 is `strictPort` — if it is taken, the dev server exits instead of
picking another, and `tauri dev` then fails with a blank window.

## Build notes

- `scripts/clean-build-cache.sh` reclaims Rust target space.
- `scripts/embedding-model.md` describes the bundled local embedding model: its cost,
  how to rebuild it with `scripts/build-embedding-model.py`, and what to check after.
  The weights are Git LFS objects — `git lfs pull` after a clone, or the model tests
  fail with a message saying so.

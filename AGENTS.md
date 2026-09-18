# AGENTS.md

Context for AI coding agents (Claude Code and others) working in this repository.

## Project - original app

**Alfa Atlas** — a documentation editor that works directly with Git repositories.

- Original codebase: /Users/eugene/Downloads/docflow-tauri/docflow
- Identifier: `com.eugene.alfa-atlas`
- Stack: Tauri v2, React + TypeScript frontend, Rust backend
- Package manager: **bun** — always use `bun`/`bunx`, never `npm`/`pnpm`/`yarn`

## Project - this app

**Atlas desktop** - a cli agent like claude code desktop 

- Identifier: `com.eugene.atlas-desktop`
- Stack: Tauri v2, React + TypeScript frontend, Rust backend
- Package manager: **bun** — always use `bun`/`bunx`, never `npm`/`pnpm`/`yarn`

Tech stack
Frontend: React 19 + TypeScript, Vite build, plain CSS files per component (no CSS-in-JS/Tailwind), lucide-react for icons. No global state library (Redux/Zustand/Context store) — state lives in custom hooks (`src/hooks/*`, ~80 of them) composed directly into components (e.g. `useLlmChat`, `useLlmSetup`).

`App.tsx` is the composition root: it calls ~55 of those hooks and passes their results down as props. That is where cross-cutting state actually lives, and it is why the file is ~1300 lines — adding a hook that more than one panel needs usually means editing it. Before adding state there, check whether the hook can own it privately, or whether an existing hook already exposes it. React Context is used in exactly one place (`AsciiDocPreview/AscPreviewContext.tsx`, scoped to that subtree) — it is a deliberate local exception for preview rendering, not a pattern to spread.

Backend: Tauri v2 (Rust), ureq (blocking HTTP client, not reqwest) for LLM provider calls, tauri::async_runtime::spawn_blocking to run them off the async runtime. Streaming deltas and other progress reach the frontend as tauri::Emitter events, emitted in commands/ only — services report through sinks (see Architecture).

## Setup & commands

```bash
bun install                 # install frontend deps
bun run tauri dev           # run the app with hot reload
bun run tauri build         # production bundle
bun run tsc --noEmit        # type-check frontend only
cd src-tauri && cargo check # fast Rust check while iterating
cd src-tauri && cargo test  # run Rust tests
cd src-tauri && cargo add <crate>   # add a Rust dependency
bun add <pkg>                       # add a JS dependency
```

Run `bun run tsc --noEmit` and `cargo check` before considering a change done. If tests exist for the touched area, run them too — don't assume green.

## Architecture

The app is layered; keep new code in the right layer instead of adding logic to the boundary.

```
src/                      # React frontend
├── components/           # render only — no fetching, no business rules
├── hooks/                 # frontend application layer: call invoke() wrappers, hold state
└── lib/                    # two things: one typed wrapper per Tauri command, plus shared pure helpers

src-tauri/src/
├── commands/              # boundary: thin #[tauri::command] fns — validate input, call a service, map errors to String
├── services/               # application layer: use-case orchestration
├── domain/                  # pure types + business rules + typed error enums (thiserror), no I/O, no framework types
└── infra/                    # git2 / filesystem / network — concrete implementations of domain traits
```

Dependency direction points inward: `commands → services → domain`, and `infra` implements traits that `domain`/`services` define — never the reverse. Don't reach for `git2` or `tauri::` types from inside `domain/` or `services/`.

**Reporting outward crosses a port, never an `AppHandle`.** A service that needs to tell the UI something takes a sink — `Arc<dyn Fn(Event) + Send + Sync>`, with the event type in `domain/` — and the command layer is the only place that turns those into Tauri events. The one that exists here today is `domain::turn::ChatEventSink`, adapted in `commands/chat_events.rs`; `domain::command_exec::CommandSink` is the same shape one layer down. The index layer's is `domain::workspace_index::IndexEventSink` (one `IndexEvent` enum: sync started, keywords ready, embedding progress, sync finished, failed), taken by `services::index_sync::RepoIndexer::sync` and `services::workspace_index::WorkspaceIndex::open`, adapted in `commands/workspace_events.rs`. When a service reports more than one kind of thing, use one enum rather than several callbacks. `tauri::async_runtime` used purely as a thread pool (`spawn_blocking`) is fine in `services/` — that's a runtime, not the UI.

Don't pre-build all four layers for something trivial. Introduce a trait boundary when there's a real second implementation (e.g. a test double) or a use-case spanning multiple infra calls — not speculatively.

See [`AI_HARNESS.md`](AI_HARNESS.md) for the AI-agent tool-access infrastructure (`domain/ai_access.rs`, `domain/ai_tools.rs`, `services/ai_tools/`). It is fully wired: `services::llm_chat` runs the tool-calling loop against it, and the assistant panel drives it from the UI.

## Errors

- Model failures as data as deep into the stack as possible: `thiserror` enums in `domain/`, not `String`.
- Flatten to `String` (or a small serializable DTO) only inside `commands/`, at the IPC boundary — that's the one place stringly-typed errors are acceptable.
- No `unwrap()`/`expect()` outside of tests and truly-unreachable invariants.

## IPC conventions

- Every `#[tauri::command]` gets a matching typed wrapper in `src/lib/` — components/hooks call the wrapper, never `invoke()` directly.
- New commands must be registered in `generate_handler![]` in `lib.rs` (`main.rs` only calls `run()`), and any new plugin/API surface needs a corresponding entry in `src-tauri/capabilities/*.json` — a command that "does nothing" at runtime usually means a missing capability entry, not a missing registration.
- Long-running git operations (clone, fetch) run as `async` commands or via `spawn_blocking`, not on the IPC event loop.

## Filesystem & git

- Prefer `git2` over shelling out to `git` for programmatic operations (diff, blame, branch listing); shell out only for actions you don't want to reimplement, isolated to one module.
- Validate/canonicalize any path coming from the frontend against the opened repo root before touching it.
- Use `std::path::Path`/`PathBuf` throughout — no manual path string concatenation (the app targets macOS/Windows/Linux).

## Style

- Prefer immutable data and explicit, exhaustive error handling (`match` over ignoring `Err`).
- Keep abstractions proportionate to actual need — don't add a layer, trait, or generic parameter for a hypothetical future case.
- Match existing patterns in the file/module you're editing before introducing a new one.

### UI

**Don't use a browser control where the app already draws its own.** The app renders its interactive widgets itself, so a native one arrives with the platform's look — a macOS `<select>` among hand-styled panels reads as something pasted in from another program, and it ignores the theme tokens everything else is built from.

The pattern for a dropdown is a `<button>` trigger plus a menu of `role="option"` buttons, dismissed by an outside `pointerdown` or `Escape`. Existing implementations to copy: `.method-select*` (`HttpRequestBuilder`), `.oas-select*` (`OpenApiExplorer`), `.assistant-mode-*` (`AssistantConversation`). The same applies to anything else the platform would draw its own way — `<input type="checkbox">` is styled through `.settings-check`, dialogs are the app's own modal shell rather than `alert`/`confirm`.

Colours, spacing and fonts come from the tokens in `src/styles/tokens.css` (`--bg-*`, `--text-*`, `--border`, `--accent`, `--font-ui*`). A literal hex or pixel font size in a component is a bug: it will not follow the user's theme or font-size preference.

A component used from more than one place carries its own styles rather than borrowing a neighbour's — CSS is bundled globally, so borrowing appears to work right up until the neighbour is deleted.

## Commit / PR expectations

- Keep commits scoped to one layer or one concern where practical.
- Run the relevant checks above before marking work done; mention in the summary which checks were run.

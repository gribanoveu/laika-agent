// Registers happy-dom's globals (document, window, …) before any test runs,
// so `@testing-library/react` can render. Loaded via `bunfig.toml`'s
// `[test] preload` — Bun's own runner has no DOM of its own.
//
// Pure-function tests in this directory neither need nor notice it.
import { GlobalRegistrator } from "@happy-dom/global-registrator";
import { afterEach } from "bun:test";

GlobalRegistrator.register();

// Unmount whatever the finished test rendered. Registered here rather than
// repeated in every file because forgetting it is silent until it isn't: a
// leaked render keeps its effects — and its global event listeners — alive
// for the rest of the file, so one dispatched `window` event reaches every
// instance the file ever created, not just the one under test.
//
// Imported dynamically, after `register()`: a static import is hoisted above
// it, and `@testing-library/react` wants the DOM globals at module-load time.
const { cleanup } = await import("@testing-library/react");

afterEach(cleanup);

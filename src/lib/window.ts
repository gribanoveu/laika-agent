import { getCurrentWindow } from "@tauri-apps/api/window";

// The window is undecorated and transparent (tauri.conf.json), so the app's own
// titlebar drives it. Components call these wrappers, never the API directly.
// Outside Tauri (plain `bun run dev`) there is no window to drive — no-op instead
// of throwing, so the UI stays previewable in a browser.
const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const minimizeWindow = () => inTauri() && getCurrentWindow().minimize();
export const toggleMaximizeWindow = () => inTauri() && getCurrentWindow().toggleMaximize();
export const closeWindow = () => inTauri() && getCurrentWindow().close();
export const startWindowDrag = () => inTauri() && getCurrentWindow().startDragging();

import { getCurrentWindow } from "@tauri-apps/api/window";

// The window is undecorated and transparent (tauri.conf.json), so the app's own
// titlebar drives it. Components call these wrappers, never the API directly.
// Outside Tauri (plain `bun run dev`) there is no window to drive — no-op instead
// of throwing, so the UI stays previewable in a browser.
const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

// On macOS the window keeps its native frame with the title bar hidden
// (tauri.macos.conf.json): the OS draws the traffic lights, the corners and the
// edges a resize grabs — an undecorated window there leaves a pixel-thin edge.
export const nativeFrame = inTauri() && navigator.userAgent.includes("Mac");

export const minimizeWindow = () => inTauri() && getCurrentWindow().minimize();
export const toggleMaximizeWindow = () => inTauri() && getCurrentWindow().toggleMaximize();
export const closeWindow = () => inTauri() && getCurrentWindow().close();
export const startWindowDrag = () => inTauri() && getCurrentWindow().startDragging();

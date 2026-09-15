import { open } from "@tauri-apps/plugin-dialog";

/**
 * The platform's folder picker.
 *
 * The one place the app does *not* draw its own dialog: a file chooser is the
 * system's, with the user's sidebar, their recent places and their idea of
 * where things live. Drawing our own would be a worse file browser wearing
 * the right colours.
 *
 * `null` when the user cancelled — and outside the app, where there is no
 * picker to open.
 */
export async function pickFolder(): Promise<string | null> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return null;
  const chosen = await open({ directory: true, multiple: false, title: "Open folder" });
  return typeof chosen === "string" ? chosen : null;
}

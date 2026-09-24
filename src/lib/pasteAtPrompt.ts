import type { Terminal } from "@xterm/xterm";

/**
 * Text waiting to be pasted into a terminal, by its id, until a screen of that
 * terminal pastes it. It outlives any one screen: StrictMode, a tab switch or
 * a pane moved to the other dock draws the screen anew, and a paste still
 * waiting for its prompt must reach the next one rather than go with the old.
 */
const queued = new Map<number, string>();
/** The screens drawn now, each told when something is queued. */
const screens = new Set<() => void>();

/** Asks for `text` at terminal `id`'s prompt: now if a screen shows it, else when one does. */
export function queuePaste(id: number, text: string) {
  queued.set(id, text);
  screens.forEach((flush) => flush());
}

/**
 * Pastes what is queued for terminal `id` into its screen `term`, as a user's
 * paste — never run: in bracketed paste mode a newline in it is text, not
 * Enter. Returns what stops it.
 *
 * The paste waits for the prompt. Typed ahead of it, the terminal echoes the
 * text before the shell draws its prompt, and zsh then draws it a second time
 * after the prompt. zsh and bash turn bracketed paste on at each prompt, so
 * that mode is the sign the prompt is up. A shell that never turns it on
 * (cmd.exe) gets the paste once its output has been quiet for `quietMs`, as
 * Cmd+V would give it.
 * ponytail: a shell that starts silently for longer than `quietMs` and has no
 * bracketed paste gets the text early; a prompt-pattern check if that shows up.
 */
export function pasteAtPrompt(term: Terminal, id: number, quietMs = 1000): () => void {
  let quiet: ReturnType<typeof setTimeout> | undefined;

  const flush = (force = false) => {
    const text = queued.get(id);
    if (text === undefined) return;
    clearTimeout(quiet);
    if (!force && !term.modes.bracketedPasteMode) {
      quiet = setTimeout(() => flush(true), quietMs);
      return;
    }
    queued.delete(id);
    term.paste(text);
    term.focus();
  };
  const parsed = term.onWriteParsed(() => flush());
  screens.add(flush);
  flush();

  return () => {
    clearTimeout(quiet);
    parsed.dispose();
    screens.delete(flush);
  };
}

import { useEffect, type RefObject } from "react";
import { Terminal, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { openUrl } from "@tauri-apps/plugin-opener";
import { terminalAttach, terminalResize, terminalWrite } from "../lib/terminal";
import "@xterm/xterm/css/xterm.css";

const ANSI = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "brightBlack",
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
  "brightWhite",
] as const;

/**
 * The terminal's look, from the theme's tokens: xterm.js draws its own cells
 * and takes colours as values, not as CSS. The font is whatever the element's
 * CSS resolves `--font-mono` and `--fs-sm` to.
 */
function look(element: HTMLElement) {
  const root = getComputedStyle(document.documentElement);
  const token = (name: string) => root.getPropertyValue(name).trim();
  const theme: ITheme = {
    background: token("--bg-panel"),
    foreground: token("--text"),
    cursor: token("--text"),
    cursorAccent: token("--bg-panel"),
    selectionBackground: token("--bg-active"),
  };
  for (const name of ANSI) theme[name] = token(`--term-${name.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`);
  const css = getComputedStyle(element);
  return { theme, fontFamily: css.fontFamily, fontSize: parseFloat(css.fontSize) || 12 };
}

/**
 * Draws terminal `id` into `container`: what it wrote so far, then what it
 * writes; what is typed goes to its shell, and its size follows the element's.
 * Detached when it unmounts — the shell runs on in the backend.
 */
export function useTerminalScreen(id: number, container: RefObject<HTMLDivElement | null>) {
  useEffect(() => {
    const element = container.current;
    if (!element) return;
    const term = new Terminal({ ...look(element), cursorBlink: true, scrollback: 5000 });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon((_, uri) => void openUrl(uri)));
    term.open(element);

    const input = term.onData((data) => void terminalWrite(id, data));
    const resized = term.onResize(({ cols, rows }) => void terminalResize(id, cols, rows));
    // Hidden or not laid out yet, there is nothing to measure.
    const refit = () => {
      if (element.clientWidth > 0 && element.clientHeight > 0) fit.fit();
    };
    refit();
    // Fitting to the size xterm.js already had says nothing; the shell may
    // still be at the size the last screen gave it.
    void terminalResize(id, term.cols, term.rows);
    const sized = new ResizeObserver(refit);
    sized.observe(element);
    // A theme or font-size change is an attribute on <html>.
    const restyled = new MutationObserver(() => {
      const next = look(element);
      term.options.theme = next.theme;
      term.options.fontFamily = next.fontFamily;
      term.options.fontSize = next.fontSize;
      refit();
    });
    restyled.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "style"] });

    let live = true;
    let detach: (() => void) | undefined;
    terminalAttach(id, (bytes) => term.write(bytes)).then(
      (off) => (live ? (detach = off) : off()),
      (e) => term.write(`\r\n${String(e)}\r\n`),
    );

    // Opened from the pane — its "+", a tab — or with nothing else focused:
    // typing goes here. Not taken from the composer the user is typing in.
    const focused = document.activeElement;
    if (!focused || focused === document.body || element.closest(".aside")?.contains(focused)) term.focus();

    return () => {
      live = false;
      detach?.();
      input.dispose();
      resized.dispose();
      sized.disconnect();
      restyled.disconnect();
      term.dispose();
    };
  }, [id, container]);
}

import { createHighlighterCore, type ThemedToken } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import { bundledLanguages, type BundledLanguage } from "shiki/langs";

/** Fence tags models write that are not Shiki's own ids. */
const ALIASES: Record<string, string> = {
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  console: "shell",
  ts: "typescript",
  js: "javascript",
  jsx: "javascript",
  py: "python",
  rb: "ruby",
  rs: "rust",
  "c++": "cpp",
  cs: "csharp",
  kt: "kotlin",
  kts: "kotlin",
  yml: "yaml",
  docker: "dockerfile",
  make: "makefile",
  md: "markdown",
  h: "c",
  hpp: "cpp",
  mjs: "javascript",
  cjs: "javascript",
  mts: "typescript",
  cts: "typescript",
};

export function resolveLanguage(raw: string | null): BundledLanguage | null {
  if (!raw) return null;
  const name = raw.trim().toLowerCase();
  const id = ALIASES[name] ?? name;
  return id in bundledLanguages ? (id as BundledLanguage) : null;
}

/** A file's language by its extension, or by its name for the ones that have
 * none (`Dockerfile`, `Makefile`). */
export function languageOf(path: string): BundledLanguage | null {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const dot = name.lastIndexOf(".");
  return (dot > 0 ? resolveLanguage(name.slice(dot + 1)) : null) ?? resolveLanguage(name);
}

/** Lines of a fence body, without the newline that closes it. */
export function splitLines(source: string): string[] {
  return source.replace(/\n$/, "").split("\n");
}

// The JavaScript regex engine, not Oniguruma: that one is WebAssembly, and the
// window's CSP allows no `wasm-unsafe-eval` (docs/08-data-policy.md). A grammar
// the engine cannot run throws, and the block stays plain.
// `shiki/core`, not `shiki`: the full entry brings that engine into the
// bundle even unused. Grammars are split into chunks loaded on first use.
let highlighter: ReturnType<typeof createHighlighterCore> | null = null;

/** Colours for `source`, one token list per line, or `null` for an unknown
 * language or a grammar that failed — callers show the text uncoloured.
 * Each token carries both themes as `--shiki-light`/`--shiki-dark`; the
 * stylesheet picks one by the app's `data-theme`. */
export async function highlight(source: string, raw: string | null): Promise<ThemedToken[][] | null> {
  const lang = resolveLanguage(raw);
  if (!lang) return null;
  try {
    highlighter ??= createHighlighterCore({
      themes: [import("shiki/themes/light-plus.mjs"), import("shiki/themes/dark-plus.mjs")],
      langs: [],
      engine: createJavaScriptRegexEngine({ forgiving: true }),
    });
    const shiki = await highlighter;
    await shiki.loadLanguage(bundledLanguages[lang]);
    return shiki.codeToTokens(source.replace(/\n$/, ""), {
      lang,
      themes: { light: "light-plus", dark: "dark-plus" },
      defaultColor: false,
    }).tokens;
  } catch {
    return null;
  }
}

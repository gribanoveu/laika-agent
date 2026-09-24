import { createHighlighterCore } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import { bundledLanguages, type BundledLanguage } from "shiki/langs";

/** A coloured run of a line: its text and both themes' colours as CSS variables. */
export type Token = { content: string; htmlStyle?: Record<string, string> };

// The JavaScript regex engine, not Oniguruma: that one is WebAssembly, and the
// window's CSP allows no `wasm-unsafe-eval` (docs/08-data-policy.md). A grammar
// the engine cannot run throws, and the block stays plain.
// `shiki/core`, not `shiki`: the full entry brings that engine into the
// bundle even unused. Grammars are split into chunks loaded on first use.
let highlighter: ReturnType<typeof createHighlighterCore> | null = null;

/**
 * Colours for `source`, one token list per line, or `null` for a grammar that
 * failed. Runs in the highlight worker — tokenizing a large file takes the
 * better part of a second — or, where there is none, on the page.
 */
export async function tokenize(source: string, lang: BundledLanguage): Promise<Token[][] | null> {
  try {
    highlighter ??= createHighlighterCore({
      themes: [import("shiki/themes/light-plus.mjs"), import("shiki/themes/dark-plus.mjs")],
      langs: [],
      engine: createJavaScriptRegexEngine({ forgiving: true }),
    });
    const shiki = await highlighter;
    await shiki.loadLanguage(bundledLanguages[lang]);
    const { tokens } = shiki.codeToTokens(source.replace(/\n$/, ""), {
      lang,
      themes: { light: "light-plus", dark: "dark-plus" },
      defaultColor: false,
    });
    // Only what is drawn crosses back from the worker.
    return tokens.map((line) => line.map(({ content, htmlStyle }) => ({ content, htmlStyle: htmlStyle as Record<string, string> })));
  } catch {
    return null;
  }
}

/// <reference lib="webworker" />
import type { BundledLanguage } from "shiki/langs";
import { tokenize } from "./shikiTokens";

// Shiki off the page: a message per block or file, answered with its tokens.
self.onmessage = async ({ data }: MessageEvent<{ id: number; source: string; lang: BundledLanguage }>) => {
  self.postMessage({ id: data.id, tokens: await tokenize(data.source, data.lang) });
};

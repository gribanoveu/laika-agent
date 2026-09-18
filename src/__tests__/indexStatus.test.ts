import { describe, expect, test } from "bun:test";
import type { IndexEvent } from "../lib/chat";
import { applyIndexEvent, describeIndex, fromSnapshot, type IndexState } from "../lib/indexStatus";

const root = "/repo";
const run = (...events: IndexEvent[]) =>
  events.reduce<IndexState | undefined>((state, event) => applyIndexEvent(state, event), undefined)!;

describe("the index status", () => {
  test("a sync goes from reading, through embedding, to ready", () => {
    const events: IndexEvent[] = [
      { root, kind: "syncStarted" },
      { root, kind: "keywordsReady", indexed: 10, unchanged: 0, removed: 0, skipped: 2 },
      { root, kind: "embeddingProgress", done: 64, total: 256 },
    ];
    expect(describeIndex(run(events[0])).label).toBe("Indexing…");
    expect(describeIndex(run(...events)).label).toBe("Indexing 25%");

    const done = run(...events, { root, kind: "syncFinished", embedded: 256, embeddingError: null });
    expect(describeIndex(done)).toMatchObject({ label: "Indexed · 2 skipped", tone: "ok" });
    expect(describeIndex(done).detail).toContain("2 files not indexed");
  });

  test("without the model, search is said to be by words only, and why", () => {
    const state = run({ root, kind: "syncFinished", embedded: 0, embeddingError: "model.safetensors is missing" });
    const shown = describeIndex(state);
    expect(shown).toMatchObject({ label: "Words only", tone: "warn" });
    expect(shown.detail).toContain("model.safetensors is missing");
  });

  test("a failure shows until the next sync starts", () => {
    const failed = run({ root, kind: "failed", error: "the folder cannot be watched" });
    expect(describeIndex(failed)).toMatchObject({ label: "Index failed", tone: "error" });
    expect(describeIndex(failed).detail).toContain("cannot be watched");

    const again = applyIndexEvent(failed, { root, kind: "syncStarted" });
    expect(again.error).toBeNull();
    expect(describeIndex(again).tone).toBe("busy");
  });

  test("a new sync keeps what the last one found until it knows better", () => {
    const ready = run(
      { root, kind: "keywordsReady", indexed: 1, unchanged: 0, removed: 0, skipped: 3 },
      { root, kind: "syncFinished", embedded: 40, embeddingError: null },
    );
    const next = applyIndexEvent(ready, { root, kind: "syncStarted" });
    expect([next.embedded, next.skipped]).toEqual([40, 3]);
  });

  test("a snapshot reads as the sync it describes", () => {
    const busy = fromSnapshot({ root, syncing: true, embedded: 0, skipped: 0, embeddingError: null });
    expect(describeIndex(busy).label).toBe("Indexing…");
    const ready = fromSnapshot({ root, syncing: false, embedded: 9, skipped: 1, embeddingError: null });
    expect(describeIndex(ready).label).toBe("Indexed · 1 skipped");
  });
});

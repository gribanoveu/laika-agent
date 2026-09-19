import type { IndexEvent, IndexSnapshot } from "./chat";

/** What the window knows about one folder's index. */
export type IndexState = {
  phase: "syncing" | "embedding" | "ready" | "failed";
  done: number;
  total: number;
  embedded: number;
  /** Files the index would not take — binary, too large, unreadable. */
  skipped: number;
  /** Why search by meaning is off; search by words still works. */
  embeddingError: string | null;
  /** Why the index is not current at all. */
  error: string | null;
};

const EMPTY: IndexState = {
  phase: "syncing",
  done: 0,
  total: 0,
  embedded: 0,
  skipped: 0,
  embeddingError: null,
  error: null,
};

export function fromSnapshot(snapshot: IndexSnapshot): IndexState {
  return {
    ...EMPTY,
    phase: snapshot.syncing ? "syncing" : "ready",
    embedded: snapshot.embedded,
    skipped: snapshot.skipped,
    embeddingError: snapshot.embeddingError,
  };
}

export function applyIndexEvent(state: IndexState | undefined, event: IndexEvent): IndexState {
  const s = state ?? EMPTY;
  switch (event.kind) {
    case "syncStarted":
      return { ...s, phase: "syncing", done: 0, total: 0, error: null };
    case "keywordsReady":
      return { ...s, skipped: event.skipped };
    case "embeddingProgress":
      return { ...s, phase: "embedding", done: event.done, total: event.total };
    case "syncFinished":
      return { ...s, phase: "ready", embedded: event.embedded, embeddingError: event.embeddingError };
    case "failed":
      return { ...s, phase: "failed", error: event.error };
  }
}

/** One short label for the title bar, and the detail behind it. */
export function describeIndex(state: IndexState): {
  label: string;
  detail: string;
  tone: "busy" | "ok" | "warn" | "error";
} {
  const skipped =
    state.skipped > 0 ? `${state.skipped} file${state.skipped === 1 ? "" : "s"} not indexed (binary, too large or unreadable).` : "";
  const lines = (...parts: string[]) => parts.filter(Boolean).join("\n");
  switch (state.phase) {
    case "syncing":
      return { label: "Indexing…", detail: "Reading the folder for search.", tone: "busy" };
    case "embedding": {
      const percent = state.total > 0 ? Math.floor((state.done / state.total) * 100) : 0;
      return {
        label: `Indexing ${percent}%`,
        detail: lines("Search by words works already; search by meaning is being built.", skipped),
        tone: "busy",
      };
    }
    case "failed":
      return { label: "Index failed", detail: lines(state.error ?? "", skipped), tone: "error" };
    case "ready":
      if (state.embeddingError) {
        return {
          label: "Words only",
          detail: lines(`Search by meaning is unavailable: ${state.embeddingError}`, skipped),
          tone: "warn",
        };
      }
      return {
        label: "Indexed",
        detail: lines(`${state.embedded} passages searchable by meaning.`, skipped),
        tone: "ok",
      };
  }
}

import { useEffect, useState } from "react";
import { indexStatus, onIndexEvent } from "../lib/chat";
import { applyIndexEvent, fromSnapshot, type IndexState } from "../lib/indexStatus";

/**
 * The index of the folder at `path`, kept current from its events.
 *
 * Listens from mount, for every folder: the first sync starts inside
 * `workspace_open`, before the window learns the path, and on a small folder
 * it is over by then. The snapshot covers a window that was not listening at
 * all — a reload — and never overrides what an event has already said.
 */
export function useIndexStatus(path: string | null): IndexState | null {
  const [byRoot, setByRoot] = useState<Record<string, IndexState>>({});

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let live = true;
    onIndexEvent((event) =>
      setByRoot((known) => ({ ...known, [event.root]: applyIndexEvent(known[event.root], event) })),
    ).then((stop) => (live ? (unlisten = stop) : stop()));
    return () => {
      live = false;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!path) return;
    indexStatus()
      .then((snapshot) => {
        if (!snapshot) return;
        setByRoot((known) =>
          known[snapshot.root] ? known : { ...known, [snapshot.root]: fromSnapshot(snapshot) },
        );
      })
      .catch(() => {});
  }, [path]);

  return path ? (byRoot[path] ?? null) : null;
}

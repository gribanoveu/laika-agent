import { useCallback, useEffect, useState } from "react";
import { gitChanges, gitCommit, gitStage, gitUnstage, type WorkingChanges } from "../lib/chat";

/**
 * How often the open tab reads the status again: the agent and the user's
 * editor change files without telling it.
 */
// ponytail: a status per tick; a large repository pays it every 2s while the
// tab is open — the file watcher's events if that shows.
const POLL_MS = 2000;

const none: WorkingChanges = { staged: [], unstaged: [] };

/** The open folder's staged and unstaged files, read while the Changes tab is on screen. */
export function useStaging(visible: boolean) {
  const [changes, setChanges] = useState<WorkingChanges>(none);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(
    () =>
      gitChanges().then(
        (next) => {
          setChanges(next);
          setError(null);
        },
        (e) => {
          setChanges(none);
          setError(String(e));
        },
      ),
    [],
  );

  useEffect(() => {
    if (!visible) return;
    void load();
    const timer = setInterval(load, POLL_MS);
    return () => clearInterval(timer);
  }, [visible, load]);

  // Every action reads the status back rather than guessing it: staging a
  // file with edits on both sides, say, leaves it in one list, not two.
  const act = useCallback(
    async <T,>(op: () => Promise<T>): Promise<T> => {
      try {
        return await op();
      } finally {
        await load();
      }
    },
    [load],
  );

  return {
    ...changes,
    error,
    stage: (paths: string[]) => act(() => gitStage(paths)),
    unstage: (paths: string[]) => act(() => gitUnstage(paths)),
    commit: (message: string) => act(() => gitCommit(message)),
  };
}

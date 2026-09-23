import { useCallback, useEffect, useState } from "react";
import {
  gitChanges,
  gitCommit,
  gitStage,
  gitUnstage,
  onGitChanged,
  onIndexEvent,
  type WorkingChanges,
} from "../lib/chat";

const none: WorkingChanges = { staged: [], unstaged: [] };

/**
 * The open folder's staged and unstaged files, read while the Changes tab is
 * on screen: when it opens, after each of its own actions, and when the
 * backend says the folder changed — an edit to the tree starts an index sync,
 * and a change to `.git` has its own event.
 */
export function useStaging(visible: boolean, workspace: string | null) {
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
    const stops: (() => void)[] = [];
    let live = true;
    const keep = (stop: () => void) => (live ? stops.push(stop) : stop());
    onIndexEvent((event) => event.root === workspace && event.kind === "syncStarted" && void load()).then(keep);
    onGitChanged((root) => root === workspace && void load()).then(keep);
    return () => {
      live = false;
      stops.forEach((stop) => stop());
    };
  }, [visible, workspace, load]);

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

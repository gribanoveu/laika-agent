import { useCallback, useState } from "react";
import { gitWorktreeCheck, gitWorktreeRemove, type WorktreeCheck, type WorktreeRemoved } from "../lib/chat";

type Deps = {
  /** The open folder — the worktree to remove. */
  workspace: string | null;
  /** Runs `go` once leaving the open folder is agreed to — `useFolderSwitch`'s guard. */
  guard: (go: () => void, onCancel?: () => void) => void;
  open: (path: string) => Promise<boolean>;
  notify: (message: string) => void;
  /** The folder list, read again once the worktree is gone from it. */
  refreshRecent: () => void;
};

/** What the toast says once it is done. */
export function describeRemoval({ branchKept, chatsRemoved }: WorktreeRemoved) {
  const chats = chatsRemoved === 0 ? "" : chatsRemoved === 1 ? " with its chat" : ` with its ${chatsRemoved} chats`;
  const branch = branchKept ? ` — branch ${branchKept} kept` : "";
  return `Worktree removed${chats}${branch}`;
}

/**
 * Removing the open worktree: its state is read and shown first, then the
 * window goes back to the main folder — which stops the worktree's processes
 * and terminals — and only then is the folder deleted, with its chats.
 *
 * Nothing uncommitted is ever deleted: the backend refuses, and the dialog
 * says so before anyone clicks.
 */
export function useWorktreeRemoval({ workspace, guard, open, notify, refreshRecent }: Deps) {
  const [asked, setAsked] = useState<{ path: string; check: WorktreeCheck } | null>(null);

  const ask = useCallback(async () => {
    if (!workspace) return;
    try {
      setAsked({ path: workspace, check: await gitWorktreeCheck() });
    } catch (e) {
      notify(String(e));
    }
  }, [workspace, notify]);

  const close = useCallback(() => setAsked(null), []);

  const confirm = useCallback(() => {
    if (!asked) return;
    const { path, check } = asked;
    setAsked(null);
    guard(() => {
      void (async () => {
        if (!(await open(check.main))) return;
        try {
          notify(describeRemoval(await gitWorktreeRemove(path)));
        } catch (e) {
          notify(String(e));
        } finally {
          refreshRecent();
        }
      })();
    });
  }, [asked, guard, open, notify, refreshRecent]);

  return { asked, ask, close, confirm };
}

import { useCallback, useEffect, useState } from "react";
import { currentWorkspace, openWorkspace } from "../lib/chat";
import { pickFolder } from "../lib/dialog";

/** The folder the agent acts on. Resolved by the backend, displayed here. */
export function useWorkspace() {
  const [path, setPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // The backend may already have one — a reloaded window should not forget it.
  useEffect(() => {
    currentWorkspace().then(setPath).catch(() => {});
  }, []);

  const open = useCallback(async (next: string) => {
    try {
      setPath(await openWorkspace(next.trim()));
      setError(null);
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    }
  }, []);

  /** Asks for a folder and opens it. `false` also means "the user cancelled". */
  const pick = useCallback(async () => {
    const chosen = await pickFolder();
    if (!chosen) return false;
    return open(chosen);
  }, [open]);

  return { path, error, open, pick };
}

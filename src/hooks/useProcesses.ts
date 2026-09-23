import { useCallback, useEffect, useState } from "react";
import { onProcessChanged, processesList, stopProcess, type ProcessView } from "../lib/chat";

/**
 * The background processes, read while the Terminal tab is open: when it
 * opens, and each time the backend says one of them changed — at most once a
 * tick per process however much it writes.
 */
export function useProcesses(visible: boolean) {
  const [processes, setProcesses] = useState<ProcessView[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!visible) return;
    let live = true;
    const load = () =>
      processesList().then(
        (list) => {
          if (!live) return;
          setProcesses(list);
          setError(null);
        },
        (e) => live && setError(String(e)),
      );
    load();
    let unlisten: (() => void) | undefined;
    onProcessChanged(() => void load()).then((off) => (live ? (unlisten = off) : off()));
    return () => {
      live = false;
      unlisten?.();
    };
  }, [visible]);

  const stop = useCallback(async (id: number) => {
    try {
      setProcesses(await stopProcess(id));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  return { processes, error, stop };
}

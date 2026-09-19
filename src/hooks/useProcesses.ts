import { useCallback, useEffect, useState } from "react";
import { processesList, stopProcess, type ProcessView } from "../lib/chat";

/** How often the open tab asks again: a server's output should look live, not be. */
const POLL_MS = 1000;

/** The background processes, asked for while the Terminal tab is open and not otherwise. */
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
    const timer = setInterval(load, POLL_MS);
    return () => {
      live = false;
      clearInterval(timer);
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

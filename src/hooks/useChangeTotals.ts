import { useEffect, useState } from "react";
import { gitTotals, onGitChanged, onIndexEvent, type ChangeTotals } from "../lib/chat";

/**
 * Lines added and removed in the open folder since the last commit, for the
 * chat header. Read when the folder opens and whenever the backend says it
 * changed — the same two signals the Changes tab follows. `null` outside a
 * repository, or when it cannot be read: the header then shows nothing.
 */
export function useChangeTotals(workspace: string | null): ChangeTotals | null {
  const [totals, setTotals] = useState<ChangeTotals | null>(null);

  useEffect(() => {
    setTotals(null);
    if (!workspace) return;
    let live = true;
    const load = () =>
      gitTotals().then(
        (next) => live && setTotals(next),
        () => live && setTotals(null),
      );
    void load();
    const stops: (() => void)[] = [];
    const keep = (stop: () => void) => (live ? stops.push(stop) : stop());
    onIndexEvent((event) => event.root === workspace && event.kind === "syncStarted" && void load()).then(keep);
    onGitChanged((root) => root === workspace && void load()).then(keep);
    return () => {
      live = false;
      stops.forEach((stop) => stop());
    };
  }, [workspace]);

  return totals;
}

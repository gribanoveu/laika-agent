import { useCallback, useState } from "react";
import { processesList, type ProcessView } from "../lib/chat";
import { terminalList, type TerminalInfo } from "../lib/terminal";

/** What stands between the user and another folder. */
export type SwitchBlock =
  // The turn saves its chat into the folder open when it ends: switching
  // under it would file the chat in the other one. So this one is refused.
  | { kind: "agent" }
  // `workspace_open` stops every background process and closes every
  // terminal the user has open; asked, not refused.
  | { kind: "processes"; processes: ProcessView[]; terminals: TerminalInfo[]; go: () => void };

/**
 * Switching folders ends what runs in the open one. Before it does, the user
 * is told — and the processes and terminals are read then, once, from the backend.
 */
export function useFolderSwitch(agentRunning: boolean) {
  const [blocked, setBlocked] = useState<SwitchBlock | null>(null);

  const guard = useCallback(
    async (go: () => void) => {
      if (agentRunning) return setBlocked({ kind: "agent" });
      const [processes, terminals] = await Promise.all([
        processesList().catch(() => []),
        terminalList().catch(() => []),
      ]);
      const running = processes.filter((p) => p.state.state === "running");
      const shells = terminals.filter((t) => t.state.state === "running");
      if (running.length === 0 && shells.length === 0) return go();
      setBlocked({ kind: "processes", processes: running, terminals: shells, go });
    },
    [agentRunning],
  );

  const close = useCallback(() => setBlocked(null), []);
  return { blocked, guard, close };
}

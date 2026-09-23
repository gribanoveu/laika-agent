import { useEffect, useRef, useState } from "react";
import { Plus, X } from "lucide-react";
import { ProcessList } from "./ProcessList";
import { useProcesses } from "../hooks/useProcesses";
import { useTerminals } from "../hooks/useTerminals";
import { useTerminalScreen } from "../hooks/useTerminalScreen";
import { terminalTitle } from "../lib/terminal";
import "./TerminalPanel.css";

type Props = {
  active: boolean;
  workspace: string | null;
  /** The background process a chat row asked to see; a new object each ask. */
  processFocus: { id: number } | null;
};

/** What the pane shows: one of the user's shells, or the agent's background processes. */
type Shown = number | "processes";

function TerminalView({ id }: { id: number }) {
  const ref = useRef<HTMLDivElement>(null);
  useTerminalScreen(id, ref);
  return <div ref={ref} className="terminal-view" />;
}

/**
 * The Terminal tab: the user's own shells, a tab each, and the agent's
 * background processes as the last tab. Opening the pane gives a shell, as a
 * terminal window does — once per folder, not again after the last is closed.
 */
export function TerminalPanel({ active, workspace, processFocus }: Props) {
  const { processes, error: processError, stop } = useProcesses(active);
  const shells = useTerminals(active);
  const [picked, setPicked] = useState<Shown | null>(null);
  const openedFor = useRef<string | null>(null);

  useEffect(() => {
    if (!active || !workspace || !shells.loaded || openedFor.current === workspace) return;
    openedFor.current = workspace;
    if (shells.terminals.length === 0) void shells.open().then((t) => t && setPicked(t.id));
  }, [active, workspace, shells.loaded, shells.terminals.length, shells.open]);

  useEffect(() => {
    if (processFocus) setPicked("processes");
  }, [processFocus]);

  // A picked shell that was closed falls back to the newest one left.
  const newest = shells.terminals[shells.terminals.length - 1];
  const shown: Shown =
    picked === "processes" ? "processes" : (shells.terminals.find((t) => t.id === picked) ?? newest)?.id ?? "processes";
  const running = processes.filter((p) => p.state.state === "running").length;
  const newShell = () => void shells.open().then((t) => t && setPicked(t.id));

  return (
    <div className="terminal-panel">
      <div className="terminal-tabs" role="tablist" aria-label="Terminals">
        {shells.terminals.map((t) => (
          <div key={t.id} className={`terminal-tab${shown === t.id ? " selected" : ""}`}>
            <button
              type="button"
              role="tab"
              aria-selected={shown === t.id}
              className={`terminal-tab-name${t.state.state === "running" ? "" : " ended"}`}
              onClick={() => setPicked(t.id)}
            >
              {terminalTitle(t)}
            </button>
            <button
              type="button"
              className="terminal-tab-close"
              aria-label={`Close ${t.shell}`}
              title="Close — ends what runs in it"
              onClick={() => void shells.close(t.id)}
            >
              <X size={12} />
            </button>
          </div>
        ))}
        <button
          type="button"
          className="iconbtn terminal-new"
          title={workspace ? "New terminal" : "Open a folder first"}
          aria-label="New terminal"
          disabled={!workspace}
          onClick={newShell}
        >
          <Plus size={14} />
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={shown === "processes"}
          className={`terminal-tab-name terminal-processes-tab${shown === "processes" ? " selected" : ""}`}
          onClick={() => setPicked("processes")}
        >
          Processes{running > 0 && <span className="terminal-running">{running}</span>}
        </button>
      </div>
      {shells.error && <div className="terminal-error">{shells.error}</div>}
      {shown === "processes" ? (
        <div className="terminal-processes">
          <ProcessList processes={processes} error={processError} onStop={stop} focus={processFocus} />
        </div>
      ) : (
        <TerminalView key={shown} id={shown} />
      )}
    </div>
  );
}

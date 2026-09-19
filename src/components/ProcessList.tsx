import { useEffect, useRef, useState } from "react";
import { processStatus, type ProcessView } from "../lib/chat";
import "./ProcessList.css";

type Props = {
  processes: ProcessView[];
  error: string | null;
  onStop: (id: number) => void;
};

/** Kept scrolled to the end, where a running process's news is. */
function Tail({ text }: { text: string }) {
  const ref = useRef<HTMLPreElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.scrollTop = ref.current.scrollHeight;
  }, [text]);
  return (
    <pre ref={ref} className="process-tail">
      {text || "No output yet."}
    </pre>
  );
}

/** The agent's background processes: what runs, where, how it ended, and the end of what it wrote. */
export function ProcessList({ processes, error, onStop }: Props) {
  // The newest is the one being watched; others open on a click.
  const [open, setOpen] = useState<number | null>(null);
  const shown = open ?? processes[0]?.id ?? null;

  if (!processes.length) {
    return (
      <div className="process-list-empty">
        {error ??
          "No background processes. The agent starts one with runCommand in the background — a dev server, a watcher — and it runs until stopped."}
      </div>
    );
  }
  return (
    <div className="process-list">
      {error && <div className="process-list-error">{error}</div>}
      {processes.map((p) => {
        const running = p.state.state === "running";
        const failed = p.state.state === "exited" && p.state.code !== 0;
        return (
          <div key={p.id} className={`process${shown === p.id ? " open" : ""}`}>
            <div className="process-head" onClick={() => setOpen(shown === p.id ? -1 : p.id)}>
              <span className="process-id">#{p.id}</span>
              <div className="process-body">
                <code className="process-command">{p.command}</code>
                <span className="process-cwd">{p.cwd}</span>
              </div>
              <span className={`process-state ${running ? "running" : failed ? "failed" : "ended"}`}>
                {processStatus(p.state)}
              </span>
              {running && (
                <button
                  type="button"
                  className="btn btn-ghost process-stop"
                  onClick={(e) => {
                    e.stopPropagation();
                    onStop(p.id);
                  }}
                >
                  Stop
                </button>
              )}
            </div>
            {shown === p.id && <Tail text={p.tail} />}
          </div>
        );
      })}
    </div>
  );
}

import { useEffect, useState } from "react";
import type { Task } from "../lib/chat";
import "./PlanPanel.css";

type Props = {
  plan: string | null;
  checklist: Task[];
  /** Called when an edit is finished — on leaving the text, not per keystroke. */
  onEdit: (plan: string) => void;
  /** Present in Plan mode once there is something to hand over. */
  onImplement?: () => void;
  /** A running turn may be rewriting the plan; an edit then would be lost. */
  locked: boolean;
};

const MARK: Record<Task["status"], string> = { completed: "✓", inProgress: "→", pending: "·", cancelled: "✗" };

/** The conversation's plan: a document the user reads and corrects, and the checklist the agent works through. */
export function PlanPanel({ plan, checklist, onEdit, onImplement, locked }: Props) {
  const [draft, setDraft] = useState(plan ?? "");
  // A new plan from the agent, or another chat opened, replaces the draft.
  useEffect(() => setDraft(plan ?? ""), [plan]);

  return (
    <div className="panel-section plan-panel">
      <div className="section-label">
        <span>Plan</span>
      </div>
      <textarea
        className="plan-text"
        aria-label="Plan"
        placeholder="No plan yet. In Plan mode the agent writes one here — or write your own."
        value={draft}
        readOnly={locked}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => draft !== (plan ?? "") && onEdit(draft)}
      />
      {onImplement && plan && (
        <button type="button" className="btn btn-primary" disabled={locked} onClick={onImplement}>
          Implement in Agent mode
        </button>
      )}
      <div className="section-label">
        <span>Checklist</span>
        <span className="count">
          {checklist.filter((t) => t.status === "completed").length}/{checklist.length}
        </span>
      </div>
      {checklist.length === 0 ? (
        <div className="empty">No checklist yet.</div>
      ) : (
        <ul className="plan-checklist">
          {checklist.map((task) => (
            <li key={task.id} className={task.status}>
              <span className="plan-mark">{MARK[task.status]}</span>
              <span>
                {task.title}
                {task.note && <span className="plan-note"> — {task.note}</span>}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

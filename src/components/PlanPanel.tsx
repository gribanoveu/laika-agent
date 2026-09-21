import { useEffect, useState } from "react";
import { Circle, CircleCheck, CircleDot, CircleX, Pencil } from "lucide-react";
import type { Task } from "../lib/chat";
import { Markdown } from "./Markdown";
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

const MARK: Record<Task["status"], { icon: typeof Circle; label: string }> = {
  completed: { icon: CircleCheck, label: "Done" },
  inProgress: { icon: CircleDot, label: "In progress" },
  pending: { icon: Circle, label: "To do" },
  cancelled: { icon: CircleX, label: "Dropped" },
};

function Checklist({ tasks }: { tasks: Task[] }) {
  const done = tasks.filter((t) => t.status === "completed").length;
  return (
    <div className="panel-section plan-section">
      <div className="section-label">
        <span>Checklist</span>
        <span className="count">
          {done}/{tasks.length}
        </span>
      </div>
      <div className="plan-progress" aria-hidden>
        <div style={{ width: `${(done / tasks.length) * 100}%` }} />
      </div>
      <ul className="plan-checklist">
        {tasks.map((task) => {
          const { icon: Icon, label } = MARK[task.status];
          return (
            <li key={task.id} className={`plan-task ${task.status}`}>
              <Icon size={14} className="plan-mark" aria-label={label} />
              <div className="plan-task-body">
                <div className="plan-task-title">{task.title}</div>
                {task.note && <div className="plan-task-note">{task.note}</div>}
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/**
 * The conversation's plan: a document the user reads — drawn as Markdown over
 * the whole panel — and corrects on demand, and the checklist the agent works
 * through. The checklist comes first: it is short, and it is what changes
 * while the agent works. Until the agent makes one it is not shown at all,
 * and the plan has the whole panel.
 */
export function PlanPanel({ plan, checklist, onEdit, onImplement, locked }: Props) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(plan ?? "");
  // A new plan from the agent, or another chat opened, replaces the draft.
  useEffect(() => setDraft(plan ?? ""), [plan]);

  const save = () => draft !== (plan ?? "") && onEdit(draft);
  const finish = () => {
    save();
    setEditing(false);
  };

  return (
    <div className="plan-panel">
      {checklist.length > 0 && <Checklist tasks={checklist} />}

      <div className="panel-section plan-section plan-doc-section">
        <div className="section-label">
          <span>Plan</span>
          {editing ? (
            <button type="button" className="link-btn" onClick={finish}>
              Done
            </button>
          ) : (
            <button
              type="button"
              className="iconbtn plan-edit"
              title={locked ? "The agent is working on the plan" : "Edit plan"}
              aria-label="Edit plan"
              disabled={locked}
              onClick={() => setEditing(true)}
            >
              <Pencil size={12} />
            </button>
          )}
        </div>
        {editing ? (
          <textarea
            className="plan-text"
            aria-label="Plan"
            autoFocus
            placeholder="Write the plan in Markdown."
            value={draft}
            readOnly={locked}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={save}
            onKeyDown={(e) => e.key === "Escape" && finish()}
          />
        ) : plan ? (
          <div className="plan-doc">
            <Markdown text={plan} streaming={false} />
          </div>
        ) : (
          <div className="empty">
            No plan yet. In Plan mode the agent writes one here — or{" "}
            <button type="button" className="link-btn" disabled={locked} onClick={() => setEditing(true)}>
              write your own
            </button>
            .
          </div>
        )}
      </div>

      {onImplement && plan && (
        <button type="button" className="btn btn-primary plan-implement" disabled={locked} onClick={onImplement}>
          Implement in Agent mode
        </button>
      )}
    </div>
  );
}

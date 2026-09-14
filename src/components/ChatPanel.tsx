import { useState } from "react";
import { ChevronRight, FileText, GitBranch, Pencil, Search, TerminalSquare } from "lucide-react";
import { SESSION, TURNS } from "../mock/data";
import { isApproval, type Approval, type ToolCall } from "../types";
import "./ChatPanel.css";

const TOOL_ICON = {
  Read: FileText,
  Grep: Search,
  Edit: Pencil,
  Bash: TerminalSquare,
} as const;

function ToolRow({ call }: { call: ToolCall }) {
  const [open, setOpen] = useState(false);
  const Icon = TOOL_ICON[call.name];
  return (
    <div className={`tool-item${open ? " open" : ""}`}>
      <button className="tool" type="button" onClick={() => setOpen((v) => !v)}>
        <span className="ico">
          <Icon size={13} />
        </span>
        <span className="name">{call.name}</span>
        <span className="arg">{call.arg}</span>
        {call.meta && <span className="meta">{call.meta}</span>}
        {call.stat && (
          <span className="meta mono">
            <span className="add">+{call.stat.add}</span>{" "}
            <span className={call.stat.del ? "del" : "zero"}>-{call.stat.del}</span>
          </span>
        )}
        <ChevronRight className="chev" size={12} />
      </button>
      {open && <pre className="tool-detail">{call.detail}</pre>}
    </div>
  );
}

function ApprovalCard({ approval }: { approval: Approval }) {
  return (
    <div className={`approval-card${approval.approved ? " approved" : ""}`}>
      <div className="approval-label">
        {approval.approved ? "Approved" : "Approval required"} · {approval.tool}
      </div>
      <div className="approval-cmd">{approval.command}</div>
      {!approval.approved && (
        <div className="approval-actions">
          <button className="btn btn-primary" type="button">
            Allow
          </button>
          <button className="btn btn-ghost" type="button">
            Deny
          </button>
        </div>
      )}
    </div>
  );
}

export function ChatPanel({ onPickBranch }: { onPickBranch: () => void }) {
  const { used, limit } = SESSION.context;
  const percent = Math.round((used / limit) * 100);
  const fmt = (n: number) => `${Math.round(n / 1000)}k`;

  return (
    <section className="chat-panel">
      <header className="chat-head">
        <div>
          <h1>{SESSION.title}</h1>
          <button className="branch" type="button" title="Switch branch" onClick={onPickBranch}>
            <GitBranch size={11} />
            {SESSION.branch}
          </button>
        </div>
        <div className="head-right">
          <time className="head-time">{SESSION.time}</time>
          <div className="context-meter" title={`Context: ${fmt(used)} / ${fmt(limit)}`}>
            <span
              className="context-ring"
              role="img"
              aria-label={`${percent}% context used`}
              style={{ "--ring-deg": `${percent * 3.6}deg` } as React.CSSProperties}
            />
            <span className="context-meter-val">
              <span className="used">{fmt(used)}</span>
              <span className="sep">/</span>
              {fmt(limit)}
            </span>
          </div>
        </div>
      </header>

      <div className="thread">
        {TURNS.map((turn) => (
          <div className="turn" key={turn.id}>
            <div className={`role${turn.role === "agent" ? " agent" : ""}`}>
              {turn.role === "agent" ? "Agent" : "You"}
            </div>
            {turn.role === "user" ? (
              <div className="bubble">{turn.text}</div>
            ) : (
              <p className="msg">{turn.text}</p>
            )}
            {turn.tools && (
              <div className="tools">
                {turn.tools.map((item) =>
                  isApproval(item) ? (
                    <ApprovalCard key={item.id} approval={item} />
                  ) : (
                    <ToolRow key={item.id} call={item} />
                  ),
                )}
              </div>
            )}
          </div>
        ))}
      </div>
    </section>
  );
}

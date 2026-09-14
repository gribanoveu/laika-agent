import { useState } from "react";
import { ChevronRight, FileText, GitBranch, Pencil, Search, TerminalSquare } from "lucide-react";
import { ChatEmptyState } from "./ChatEmptyState";
import { isApproval, type Approval, type Session, type ToolCall, type Turn } from "../types";
import "./ChatPanel.css";

const TOOL_ICON = {
  Read: FileText,
  Grep: Search,
  Edit: Pencil,
  Bash: TerminalSquare,
} as const;

const compact = (n: number) => `${Math.round(n / 1000)}k`;

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

type Props = {
  session: Session | null;
  turns: Turn[];
  onPickBranch: () => void;
  onOpenRepo: () => void;
  onNewChat: () => void;
};

export function ChatPanel({ session, turns, onPickBranch, onOpenRepo, onNewChat }: Props) {
  const context = session?.context;
  const percent = context ? Math.round((context.used / context.limit) * 100) : 0;

  return (
    <section className="chat-panel">
      <header className="chat-head">
        <div>
          <h1>{session?.title ?? "New session"}</h1>
          {session && (
            <button className="branch" type="button" title="Switch branch" onClick={onPickBranch}>
              <GitBranch size={11} />
              {session.branch}
            </button>
          )}
        </div>
        <div className="head-right">
          {session && <time className="head-time">{session.updatedAt}</time>}
          {context && (
            <div
              className="context-meter"
              title={`Context: ${compact(context.used)} / ${compact(context.limit)}`}
            >
              <span
                className="context-ring"
                role="img"
                aria-label={`${percent}% context used`}
                style={{ "--ring-deg": `${percent * 3.6}deg` } as React.CSSProperties}
              />
              <span className="context-meter-val">
                <span className="used">{compact(context.used)}</span>
                <span className="sep">/</span>
                {compact(context.limit)}
              </span>
            </div>
          )}
        </div>
      </header>

      <div className={`thread${turns.length === 0 ? " thread-empty" : ""}`}>
        {turns.length === 0 ? (
          <ChatEmptyState session={session} onOpenRepo={onOpenRepo} onNewChat={onNewChat} />
        ) : (
          turns.map((turn) => (
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
          ))
        )}
      </div>
    </section>
  );
}

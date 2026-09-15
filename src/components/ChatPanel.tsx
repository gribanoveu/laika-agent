import { useEffect, useState } from "react";
import {
  ChevronRight,
  FileText,
  FolderTree,
  GitBranch,
  ListTodo,
  Pencil,
  Search,
  Terminal,
  TerminalSquare,
  Trash2,
} from "lucide-react";
import { ChatEmptyState } from "./ChatEmptyState";
import { describeTool } from "../lib/describeTool";
import type { Block, TurnState } from "../lib/chatTurnReducer";
import { previewCalls, type ChatUsage, type ToolCallDecision, type ToolPreview } from "../lib/chat";
import "./ChatPanel.css";

const TOOL_ICON: Record<string, typeof FileText> = {
  Read: FileText,
  Grep: Search,
  List: FolderTree,
  Write: Pencil,
  Edit: Pencil,
  Delete: Trash2,
  Mkdir: FolderTree,
  Move: FolderTree,
  Todo: ListTodo,
  Bash: TerminalSquare,
  Status: GitBranch,
  Diff: GitBranch,
  Blame: GitBranch,
};

const compact = (n: number) => (n >= 1000 ? `${Math.round(n / 1000)}k` : `${n}`);

function ToolRow({ block }: { block: Extract<Block, { kind: "tool" }> }) {
  const [open, setOpen] = useState(false);
  const shown = describeTool(block);
  const Icon = TOOL_ICON[shown.name] ?? Terminal;

  return (
    <div className={`tool-item${open ? " open" : ""} ${block.status}`}>
      <button
        className="tool"
        type="button"
        onClick={() => setOpen((v) => !v)}
        disabled={!shown.detail}
      >
        <span className="ico">
          <Icon size={13} />
        </span>
        <span className="name">{shown.name}</span>
        <span className="arg">{shown.arg}</span>
        {shown.meta && <span className="meta">{shown.meta}</span>}
        {shown.detail && <ChevronRight className="chev" size={12} />}
      </button>
      {open && shown.detail && <pre className="tool-detail">{shown.detail}</pre>}
    </div>
  );
}

function ApprovalCard({
  block,
  onDecide,
}: {
  block: Extract<Block, { kind: "approval" }>;
  onDecide: (decisions: ToolCallDecision[], always: string[]) => void;
}) {
  const [reason, setReason] = useState("");
  // What each call would do. Approving a write means approving its contents,
  // and the arguments alone do not show them.
  const [previews, setPreviews] = useState<ToolPreview[]>([]);
  const asked = block.calls.filter((call) => call.requiresConfirmation);

  useEffect(() => {
    let live = true;
    previewCalls(block.calls)
      // `?? []`: the card is worth drawing even if the previews are not.
      .then((next) => live && setPreviews(next ?? []))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [block.calls]);
  const answer = (approved: boolean, always: string[] = []) =>
    onDecide(
      asked.map((call) => ({
        id: call.id,
        approved,
        reason: approved || !reason.trim() ? null : reason.trim(),
      })),
      always,
    );

  return (
    <div className="approval-card">
      <div className="approval-label">
        Approval required · {asked.map((call) => describeTool({ ...emptyTool, ...call }).name).join(", ")}
      </div>
      {block.calls.map((call, index) => (
        <div key={call.id}>
          <div className={`approval-cmd${call.requiresConfirmation ? "" : " passive"}`}>
            {describeTool({ ...emptyTool, ...call }).arg}
          </div>
          <Preview preview={previews[index]} />
        </div>
      ))}
      <input
        className="approval-reason"
        type="text"
        placeholder="Why not? (optional — the agent is told)"
        value={reason}
        onChange={(e) => setReason(e.target.value)}
      />
      <div className="approval-actions">
        <button className="btn btn-primary" type="button" onClick={() => answer(true)}>
          Allow
        </button>
        <button
          className="btn btn-ghost"
          type="button"
          onClick={() => answer(true, asked.map((call) => call.name))}
        >
          Always
        </button>
        <button className="btn btn-ghost" type="button" onClick={() => answer(false)}>
          Deny
        </button>
      </div>
    </div>
  );
}

/** What the call would do, once the backend has worked it out. */
export function Preview({ preview }: { preview?: ToolPreview }) {
  if (!preview || preview.kind === "nothing") return null;

  if (preview.kind === "failed") {
    // Worth as much as a successful preview: an edit whose anchor no longer
    // matches is better refused before agreeing to it than after.
    return <p className="approval-preview failed">This would not succeed: {preview.reason}</p>;
  }

  if (preview.kind === "removes") {
    return (
      <p className="approval-preview">
        Removes {preview.files} {preview.files === 1 ? "file" : "files"} under {preview.path}
      </p>
    );
  }

  if (preview.kind === "command") {
    return <p className="approval-preview">Runs in {preview.cwd}</p>;
  }

  return (
    <div className="diff-preview-wrap">
      <div className="diff-preview-head">
        <span className="diff-preview-name">{preview.path}</span>
        <span className="meta mono">
          <span className="add">+{preview.diff.linesAdded}</span>{" "}
          <span className={preview.diff.linesRemoved ? "del" : "zero"}>
            -{preview.diff.linesRemoved}
          </span>
        </span>
      </div>
      <pre className="diff-preview">
        {preview.diff.unifiedDiff.split("\n").map((line, i) => (
          <span key={i} className={lineClass(line)}>
            {line}
            {"\n"}
          </span>
        ))}
      </pre>
      {preview.diff.truncated && <p className="approval-preview">…the rest is not shown</p>}
    </div>
  );
}

const lineClass = (line: string) =>
  line.startsWith("+") ? "ln-add" : line.startsWith("-") ? "ln-del" : undefined;

/** A pending call has no result yet, and `describeTool` reads the same shape either way. */
const emptyTool = {
  kind: "tool" as const,
  round: 0,
  status: "running" as const,
  output: "",
};

/**
 * The flat block stream, grouped the way the prototype draws it: a user bubble
 * opens a turn, and everything until the next one belongs to the agent.
 */
function group(blocks: Block[]): { role: "user" | "agent"; blocks: Block[] }[] {
  const groups: { role: "user" | "agent"; blocks: Block[] }[] = [];
  for (const block of blocks) {
    const role = block.kind === "user" || block.kind === "steer" ? "user" : "agent";
    const last = groups[groups.length - 1];
    if (!last || last.role !== role || block.kind === "user") {
      groups.push({ role, blocks: [block] });
    } else {
      last.blocks.push(block);
    }
  }
  return groups;
}

type Props = {
  workspace: string | null;
  turn: TurnState;
  usage: ChatUsage | null;
  onDecide: (decisions: ToolCallDecision[], always: string[]) => void;
  onOpenRepo: () => void;
  onNewChat: () => void;
};

export function ChatPanel({ workspace, turn, usage, onDecide, onOpenRepo, onNewChat }: Props) {
  const groups = group(turn.blocks);
  const name = workspace?.split("/").filter(Boolean).pop() ?? null;

  return (
    <section className="chat-panel">
      <header className="chat-head">
        <div>
          <h1>{name ?? "New session"}</h1>
          {workspace && (
            <span className="branch" title={workspace}>
              <GitBranch size={11} />
              {workspace}
            </span>
          )}
        </div>
        <div className="head-right">
          {turn.retrying && (
            <span className="head-time" title="The provider refused; waiting before trying again">
              retrying in {turn.retrying.delaySeconds}s ({turn.retrying.attempt}/
              {turn.retrying.maxAttempts})
            </span>
          )}
          {usage && (
            <div
              className="context-meter"
              title={`Context: ${compact(usage.totalTokens)} tokens in the last request`}
            >
              <span className="context-meter-val">
                <span className="used">{compact(usage.totalTokens)}</span>
              </span>
            </div>
          )}
        </div>
      </header>

      <div className={`thread${groups.length === 0 ? " thread-empty" : ""}`}>
        {groups.length === 0 ? (
          <ChatEmptyState workspace={workspace} onOpenRepo={onOpenRepo} onNewChat={onNewChat} />
        ) : (
          groups.map((turnGroup, index) => (
            <div className="turn" key={index}>
              <div className={`role${turnGroup.role === "agent" ? " agent" : ""}`}>
                {turnGroup.role === "agent" ? "Agent" : "You"}
              </div>
              {turnGroup.blocks.map((block) => renderBlock(block, onDecide))}
            </div>
          ))
        )}
      </div>
    </section>
  );
}

function renderBlock(
  block: Block,
  onDecide: (decisions: ToolCallDecision[], always: string[]) => void,
) {
  switch (block.kind) {
    case "user":
      return (
        <div className="bubble" key={block.id}>
          {block.text}
        </div>
      );
    case "steer":
      return (
        <div className="bubble steer" key={block.id} title="Sent while the agent was working">
          {block.text}
        </div>
      );
    case "message":
      return (
        <p className="msg" key={block.id}>
          {block.text}
        </p>
      );
    case "reasoning":
      return (
        <p className="msg reasoning" key={block.id}>
          {block.text}
        </p>
      );
    case "tool":
      return (
        <div className="tools" key={block.id}>
          <ToolRow block={block} />
        </div>
      );
    case "approval":
      return (
        <div className="tools" key={block.id}>
          <ApprovalCard block={block} onDecide={onDecide} />
        </div>
      );
  }
}

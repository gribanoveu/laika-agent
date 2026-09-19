import { useEffect, useState } from "react";
import {
  Brain,
  Loader2,
  ChevronRight,
  FileText,
  Folder,
  PanelRight,
  FolderTree,
  GitBranch,
  ListTodo,
  Pencil,
  Search,
  ShieldAlert,
  Terminal,
  TerminalSquare,
  Trash2,
} from "lucide-react";
import { ChatEmptyState } from "./ChatEmptyState";
import { IndexBadge } from "./IndexBadge";
import { ContextMeter } from "./ContextMeter";
import { Markdown } from "./Markdown";
import type { IndexState } from "../lib/indexStatus";
import { describeActive, describeRun, describeTool } from "../lib/describeTool";
import { useSteadyValue } from "../hooks/useSteadyValue";
import type { Block, TurnState } from "../lib/chatTurnReducer";
import {
  previewCalls,
  type ChatUsage,
  type ContextUsage,
  type ToolCallDecision,
  type ToolPreview,
} from "../lib/chat";
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
          {call.requiresConfirmation && call.reason && (
            <div className="approval-why" title="Asked even when this tool is always allowed">
              <ShieldAlert size={13} aria-hidden />
              <span>Always asks: {call.reason}</span>
            </div>
          )}
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
type Group = { role: "user" | "agent" | "notice"; blocks: Block[] };

function group(blocks: Block[]): Group[] {
  const groups: Group[] = [];
  for (const block of blocks) {
    // A notice is nobody's turn — it is the app saying what it did — so it
    // stands alone rather than appearing under "Agent" as something said.
    const role =
      block.kind === "notice"
        ? "notice"
        : block.kind === "user" || block.kind === "steer"
          ? "user"
          : "agent";
    const last = groups[groups.length - 1];
    if (!last || last.role !== role || block.kind === "user" || role === "notice") {
      groups.push({ role, blocks: [block] });
    } else {
      last.blocks.push(block);
    }
  }
  return groups;
}

/** 12s, 1m 23s, 1h 5m — the way Claude Code says how long it worked. */
export function formatDuration(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

/** The time the agent has spent on the turn under way, ticking. */
function WorkingClock({ since, before }: { since: number; before: number }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(tick);
  }, []);
  return (
    <div className="turn-clock live" role="timer">
      <span className="turn-clock-dot" aria-hidden="true" />
      Working… {formatDuration(before + Math.max(0, now - since))}
    </div>
  );
}

/** The message that started the turn a group belongs to. */
function turnMessage(groups: Group[], index: number) {
  for (let i = index; i >= 0; i--) {
    const found = groups[i].blocks.find((b) => b.kind === "user");
    if (found?.kind === "user") return found;
  }
  return undefined;
}

/**
 * "Worked for 1m 23s" under the last group of a finished turn's answer. The
 * turn under way shows its ticking clock at the end of the thread instead.
 */
function workedFooter(groups: Group[], index: number, turn: TurnState) {
  const next = groups[index + 1];
  if (groups[index].role !== "agent" || (next && next.role !== "user")) return null;
  const live = !next && (turn.status === "running" || turn.status === "awaitingApproval");
  const worked = turnMessage(groups, index)?.workedMs;
  if (live || !worked) return null;
  return <div className="turn-clock">Worked for {formatDuration(worked)}</div>;
}

type Tool = Extract<Block, { kind: "tool" }>;
type Run = { kind: "run"; id: string; blocks: Block[] };

/**
 * An agent's calls and thinking between two things it said, folded into one
 * line — the answer is what the reader came for, the work behind it is a click
 * away. Approvals stay out: they wait for an answer and must be seen.
 */
function fold(blocks: Block[]): (Block | Run)[] {
  const out: (Block | Run)[] = [];
  for (const block of blocks) {
    const last = out[out.length - 1];
    if (block.kind !== "tool" && block.kind !== "reasoning") {
      out.push(block);
    } else if (last?.kind === "run") {
      last.blocks.push(block);
    } else {
      out.push({ kind: "run", id: `run:${block.id}`, blocks: [block] });
    }
  }
  // Thinking alone is not a run of work: it keeps its own fold.
  return out.flatMap((item) =>
    item.kind === "run" && !item.blocks.some((b) => b.kind === "tool") ? item.blocks : [item],
  );
}

/** How long one step stays on the folded line before the next may replace it. */
const STEP_MS = 700;

/** What the run is doing right now: the call under way, else the last thing in it. */
function activity(run: Run): string {
  const running = run.blocks.filter((b): b is Tool => b.kind === "tool" && b.status === "running").pop();
  const last = running ?? run.blocks[run.blocks.length - 1];
  return last.kind === "tool" ? describeActive(last) : "Thinking";
}

function ToolRun({ run, live }: { run: Run; live: boolean }) {
  const tools = run.blocks.filter((b): b is Tool => b.kind === "tool");
  const failed = tools.filter((t) => t.status === "failed").length;
  // Held for a moment each, so quick calls do not flicker past unread.
  const step = useSteadyValue(live ? activity(run) : null, STEP_MS);

  return (
    <details className={`tool-run${live ? " live" : ""}`}>
      <summary>
        {live && <Loader2 className="tool-run-spin" size={12} aria-hidden="true" />}
        {live && step ? (
          <span key={step} className="tool-run-text tool-run-step">
            {step}…
          </span>
        ) : (
          <span className="tool-run-text">{describeRun(tools)}</span>
        )}
        {live && (
          <span className="tool-run-count">
            {tools.length} {tools.length === 1 ? "call" : "calls"}
          </span>
        )}
        {failed > 0 && <span className="tool-run-failed">{failed} failed</span>}
        <ChevronRight className="chev" size={12} />
      </summary>
      <div className="tools">
        {run.blocks.map((block) =>
          block.kind === "tool" ? <ToolRow key={block.id} block={block} /> : renderBlock(block, () => {}, false),
        )}
      </div>
    </details>
  );
}

type Props = {
  /** Whether the side panel is showing; the header's button shows and hides it. */
  asideOpen?: boolean;
  onToggleAside?: () => void;
  /** The open chat's title; `null` for one not saved yet. */
  title?: string | null;
  /** The open folder's index; `null` until anything is known about it. */
  index?: IndexState | null;
  workspace: string | null;
  turn: TurnState;
  usage: ChatUsage | null;
  /** What the next request will cost, as the backend's own estimate — the one
      that decides when a conversation is folded. */
  context: ContextUsage | null;
  onDecide: (decisions: ToolCallDecision[], always: string[]) => void;
  onOpenRepo: () => void;
  onNewChat: () => void;
  onCompact: () => void;
  /** Present while the conversation is in Plan mode: hands the plan to Agent mode. */
  onImplement?: () => void;
  /** Opens the Plan tab, where the plan is read and edited before handing it over. */
  onOpenPlan?: () => void;
  /** Bubbles a branch can start at; `null` while a turn runs. */
  branchable?: ReadonlySet<string> | null;
  onBranch?: (bubbleId: string) => void;
};

export function ChatPanel({
  asideOpen = false,
  onToggleAside,
  title = null,
  workspace,
  turn,
  usage,
  context,
  index,
  onDecide,
  onOpenRepo,
  onNewChat,
  onCompact,
  onImplement,
  onOpenPlan,
  branchable = null,
  onBranch,
}: Props) {
  const groups = group(turn.blocks);
  // The one answer still arriving: the last block of a running turn.
  const streamingId = turn.status === "running" ? turn.blocks[turn.blocks.length - 1]?.id : undefined;
  // Under a finished answer only: mid-turn the plan is not written yet, and
  // after a stop or a failure it may be half of one.
  const planReady = onImplement && turn.status === "done" && groups[groups.length - 1]?.role === "agent";
  const name = workspace?.split("/").filter(Boolean).pop() ?? null;

  return (
    <section className="chat-panel">
      <header className="chat-head">
        <div className="head-left">
          <h1 title={title ?? undefined}>{title ?? "New chat"}</h1>
          {workspace && (
            <div className="head-sub">
              <span className="chat-path" title={workspace}>
                <Folder size={11} />
                <span>{name}</span>
              </span>
              {index && <IndexBadge state={index} />}
            </div>
          )}
        </div>
        <div className="head-right">
          {turn.retrying && (
            <span className="head-time" title="The provider refused; waiting before trying again">
              retrying in {turn.retrying.delaySeconds}s ({turn.retrying.attempt}/
              {turn.retrying.maxAttempts})
            </span>
          )}
          {/* Shown from the first render, before anything has been said: an
              empty conversation already costs the prompt and the schemas. */}
          {context && (
            <ContextMeter
              context={context}
              usage={usage}
              running={turn.status === "running"}
              onCompact={onCompact}
            />
          )}
          {onToggleAside && (
            <button
              type="button"
              className={`iconbtn aside-button${asideOpen ? " on" : ""}`}
              title={asideOpen ? "Hide panel" : "Show panel"}
              aria-pressed={asideOpen}
              onClick={onToggleAside}
            >
              <PanelRight size={15} />
            </button>
          )}
        </div>
      </header>

      <div className={`thread chat-text${groups.length === 0 ? " thread-empty" : ""}`}>
        {groups.length === 0 ? (
          <ChatEmptyState workspace={workspace} onOpenRepo={onOpenRepo} onNewChat={onNewChat} />
        ) : (
          groups.map((turnGroup, index) => (
            <div className="turn" key={index}>
              {turnGroup.role !== "notice" && (
                <div className={`role${turnGroup.role === "agent" ? " agent" : ""}`}>
                  {turnGroup.role === "agent" ? "Agent" : "You"}
                </div>
              )}
              {fold(turnGroup.blocks).map((block, at, items) =>
                block.kind === "run" ? (
                  <ToolRun
                    key={block.id}
                    run={block}
                    // Only the work at the very end is under way; a run the
                    // agent has already written past is finished.
                    live={turn.status === "running" && index === groups.length - 1 && at === items.length - 1}
                  />
                ) : block.kind === "user" ? (
                  <UserBubble key={block.id} block={block} branchable={branchable} onBranch={onBranch} />
                ) : (
                  renderBlock(block, onDecide, block.id === streamingId)
                ),
              )}
              {workedFooter(groups, index, turn)}
            </div>
          ))
        )}
        {turn.status === "running" && turn.runningSince !== null && (
          <WorkingClock since={turn.runningSince} before={turnMessage(groups, groups.length - 1)?.workedMs ?? 0} />
        )}
        {planReady && (
          <div className="plan-handoff">
            <button type="button" className="btn btn-primary" onClick={onImplement}>
              Implement in Agent mode
            </button>
            {onOpenPlan && (
              <button type="button" className="btn btn-ghost" onClick={onOpenPlan}>
                Review the plan
              </button>
            )}
            <span>Switches to Agent with the plan and the checklist.</span>
          </div>
        )}
      </div>
    </section>
  );
}

/**
 * What the user said, with a way to try it differently: a branch keeps the
 * conversation up to here in a new chat and gives this text back to edit.
 *
 * Offered at rest only, and not on a message folded into the compaction
 * summary — the model no longer has what came before it. That button stays,
 * disabled, so its absence is not a mystery.
 */
function UserBubble({
  block,
  branchable,
  onBranch,
}: {
  block: Extract<Block, { kind: "user" }>;
  branchable: ReadonlySet<string> | null;
  onBranch?: (bubbleId: string) => void;
}) {
  const offered = branchable !== null && onBranch !== undefined;
  const can = branchable?.has(block.id) ?? false;
  return (
    <div className="bubble-row">
      <div className="bubble">{block.text}</div>
      {offered && (
        <button
          type="button"
          className="iconbtn bubble-branch"
          disabled={!can}
          aria-label="Branch from here"
          title={
            can
              ? "Branch from here: a new chat with the conversation up to this message, which you can change and send again"
              : "Folded into the summary of earlier conversation — the model no longer sees what came before it, so a branch cannot start here"
          }
          onClick={() => onBranch(block.id)}
        >
          <GitBranch size={14} />
        </button>
      )}
    </div>
  );
}

function renderBlock(
  block: Block,
  onDecide: (decisions: ToolCallDecision[], always: string[]) => void,
  streaming: boolean,
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
    case "notice":
      return (
        <p className="notice" key={block.id}>
          {block.text}
        </p>
      );
    case "message":
      return (
        <div className="msg" key={block.id}>
          <Markdown text={block.text} streaming={streaming} />
        </div>
      );
    case "reasoning":
      return (
        <details className="reasoning" key={block.id}>
          <summary>
            <Brain size={13} />
            Thinking
            <ChevronRight className="chev" size={12} />
          </summary>
          <p className="reasoning-text">{block.text}</p>
        </details>
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

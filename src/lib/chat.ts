import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
// Type-only, and erased: the transcript's block shapes are defined beside the
// reducer that builds them, and a saved chat is where they cross the wire.
import type { Block } from "./chatTurnReducer";

// One typed wrapper per command. Components and hooks call these, never
// `invoke` directly — the command names and payload shapes live here and
// nowhere else.

// ---------------------------------------------------------------- the wire

export type Role = "system" | "user" | "assistant" | "tool";

export type LlmToolCall = { id: string; name: string; arguments: string };

export type LlmMessage = {
  role: Role;
  content: string | null;
  toolCallId?: string | null;
  toolCalls?: LlmToolCall[];
};

export type TodoStatus = "pending" | "inProgress" | "completed" | "cancelled";
export type Task = { id: string; title: string; status: TodoStatus; note?: string | null };

export type PendingToolCall = {
  id: string;
  name: string;
  arguments: string;
  requiresConfirmation: boolean;
};

/** The whole state of a paused turn. Sent back untouched — see `chat_resume`. */
export type Checkpoint = {
  history: LlmMessage[];
  round: number;
  budgetUsed: number;
  eventSeq: number;
  calls: PendingToolCall[];
  todos: Task[];
  reads: unknown;
};

export type ToolCallDecision = { id: string; approved: boolean; reason?: string | null };

export type ChatUsage = { promptTokens: number; completionTokens: number; totalTokens: number };

export type TurnResult = {
  text: string;
  reasoning?: string;
  usage?: ChatUsage | null;
  toolCalls?: LlmToolCall[];
  truncated: boolean;
  todos: Task[];
};

export type Outcome =
  | { status: "done"; value: TurnResult }
  | { status: "pendingApproval"; value: Checkpoint }
  | { status: "cancelled"; value: TurnResult };

export type OutputStream = "stdout" | "stderr";

/** One event from a running turn. Flat: `type` picks the branch. */
export type TurnEvent = { turnId: string; seq: number; round: number; targetId?: string | null } & (
  | { type: "delta"; payload: { delta: string } }
  | { type: "reasoning"; payload: { delta: string } }
  | { type: "retrying"; payload: { attempt: number; maxAttempts: number; delaySeconds: number } }
  | { type: "steeringApplied"; payload: { id: string; text: string } }
  | { type: "historyCompacted"; payload: { folded: number } }
  | { type: "roundStarted" }
  | { type: "roundCompleted"; payload: { text: string; reasoning?: string } }
  | { type: "toolCallDelta"; payload: LlmToolCall }
  | { type: "toolCall"; payload: LlmToolCall }
  | { type: "toolResult"; payload: { id: string; result?: unknown; error?: string | null } }
  | { type: "commandOutput"; payload: { id: string; stream: OutputStream; chunk: string } }
  | { type: "contextUsage"; payload: ChatUsage }
);

/**
 * The other half of a contract whose first half is `CHAT_TURN_EVENT` in
 * `src-tauri/src/commands/chat_events.rs`. Nothing checks that the two match:
 * a mismatch shows up as a window where nothing ever happens.
 */
export const CHAT_TURN_EVENT = "chat:turn-event";

// ------------------------------------------------------------- the commands

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** Outside the app there is no backend. Say so, rather than let `invoke` fail with its own wording. */
function requireBackend() {
  if (!inTauri()) throw new Error("This needs the desktop app — there is no backend in a plain browser.");
}

export async function openWorkspace(path: string): Promise<string> {
  requireBackend();
  return invoke<string>("workspace_open", { path });
}

export async function currentWorkspace(): Promise<string | null> {
  if (!inTauri()) return null;
  return invoke<string | null>("workspace_current");
}

// --------------------------------------------------------- the folder index

/**
 * The channel the open folder's index reports on. Pinned on the Rust side by
 * `the_channel_name_is_pinned` in `commands/workspace_events.rs`.
 */
export const INDEX_EVENT = "workspace-index:event";

/** One report of a sync. `root` is the string `openWorkspace` returned. */
export type IndexEvent = { root: string } & (
  | { kind: "syncStarted" }
  | { kind: "keywordsReady"; indexed: number; unchanged: number; removed: number; skipped: number }
  | { kind: "embeddingProgress"; done: number; total: number }
  | { kind: "syncFinished"; embedded: number; embeddingError: string | null }
  | { kind: "failed"; error: string }
);

/** The index as it stands, for a window that was not listening when the sync began. */
export type IndexSnapshot = {
  root: string;
  syncing: boolean;
  embedded: number;
  skipped: number;
  embeddingError: string | null;
};

export async function indexStatus(): Promise<IndexSnapshot | null> {
  if (!inTauri()) return null;
  return invoke<IndexSnapshot | null>("workspace_index_status");
}

/** Every folder's index events: a sync can outlive its folder being open. */
export async function onIndexEvent(handler: (event: IndexEvent) => void): Promise<UnlistenFn> {
  if (!inTauri()) return () => {};
  return listen<IndexEvent>(INDEX_EVENT, ({ payload }) => handler(payload));
}

/** Starts a turn. `messages` is the whole conversation: the transcript is the caller's. */
export async function startChat(
  turnId: string,
  messages: LlmMessage[],
  todos: Task[] = [],
  plan: string | null = null,
): Promise<Outcome> {
  requireBackend();
  return invoke<Outcome>("chat_start", { turnId, messages, todos, plan });
}

/**
 * Shortens the conversation when it is worth shortening, and returns the
 * shorter one. `null` means "leave yours alone" — the decision is the
 * backend's, including how much to fold.
 */
export async function compactHistory(
  messages: LlmMessage[],
  force = false,
): Promise<{ history: LlmMessage[]; folded: number } | null> {
  if (!inTauri()) return null;
  return invoke<{ history: LlmMessage[]; folded: number } | null>("chat_compact", {
    messages,
    force,
  });
}

/** Continues a paused turn. `checkpoint` goes back exactly as it arrived. */
export async function resumeChat(
  turnId: string,
  checkpoint: Checkpoint,
  decisions: ToolCallDecision[],
  plan: string | null = null,
): Promise<Outcome> {
  requireBackend();
  return invoke<Outcome>("chat_resume", { turnId, checkpoint, decisions, plan });
}

/** Returns at once; the turn stops at its next checkpoint. */
export async function cancelChat(): Promise<void> {
  requireBackend();
  return invoke<void>("chat_cancel");
}

/** Queues a mid-turn note and returns its id, which is what withdraws it. */
export async function steer(text: string): Promise<string> {
  requireBackend();
  return invoke<string>("chat_steer", { text });
}

/** `false` means a round already took it — what is said to the model cannot be unsaid. */
export async function cancelSteer(id: string): Promise<boolean> {
  requireBackend();
  return invoke<boolean>("chat_cancel_steer", { id });
}

export async function alwaysAllow(tool: string): Promise<void> {
  requireBackend();
  return invoke<void>("approval_always_allow", { tool });
}

export async function setUnattended(unattended: boolean): Promise<void> {
  requireBackend();
  return invoke<void>("approval_set_unattended", { unattended });
}

/**
 * What the next request will cost, split into the parts that behave
 * differently: compacting shortens `conversation` and nothing else.
 *
 * Mirrors `domain::compaction::ContextUsage`. An estimate, and the one the
 * backend actually decides on — which is the point of asking for it rather
 * than counting characters here.
 */
export type ContextUsage = {
  instructions: number;
  tools: number;
  conversation: number;
  total: number;
  limit: number | null;
  /** The total at which a pass starts happening on its own. */
  compactsAt: number | null;
};

export async function contextUsage(messages: LlmMessage[]): Promise<ContextUsage> {
  requireBackend();
  return invoke<ContextUsage>("chat_context_usage", { messages });
}

/** What the agent is allowed to be. Mirrors `domain::conversation_mode`. */
export type ConversationMode = "agent" | "plan" | "ask";

/**
 * Chooses the mode for the turns that follow.
 *
 * Told to the backend rather than sent with each turn, because a paused turn
 * is resumed from a checkpoint that predates the chip — and because the mode
 * is enforced there, not here: the window asking nicely for fewer tools would
 * be a rule the model never sees.
 */
export async function setConversationMode(mode: ConversationMode): Promise<void> {
  requireBackend();
  return invoke<void>("chat_set_mode", { mode });
}

/**
 * Subscribes to one turn's events.
 *
 * The channel is global, so events from another turn arrive here too and are
 * dropped by id. Without that filter two overlapping turns interleave into one
 * message — which is the reason the id is on the wire at all.
 */
export async function onTurnEvent(
  turnId: string,
  handler: (event: TurnEvent) => void,
): Promise<UnlistenFn> {
  if (!inTauri()) return () => {};
  return listen<TurnEvent>(CHAT_TURN_EVENT, ({ payload }) => {
    if (payload.turnId === turnId) handler(payload);
  });
}

// ---------------------------------------------------- provider configuration

export type ProviderConfig = {
  id: string;
  baseUrl: string;
  model?: string | null;
  trustedCertPem?: string | null;
  requestHeaders?: Record<string, string>;
  temperature?: number | null;
  maxTokens?: number | null;
  /** The model's context window in tokens. Unset means the app does not know. */
  contextLimit?: number | null;
  reasoningEffort?: string | null;
};

/** A provider as the window sees it: the configuration, plus whether a key is stored. */
export type ProviderView = ProviderConfig & { hasApiKey: boolean };

export type LlmSettings = {
  providers: ProviderView[];
  activeProviderId: string | null;
  debugLogging: boolean;
};

/** What a turn still needs before it can start. Asked before sending, not discovered by failing. */
export type Readiness = { workspace: string | null; provider: string | null; hasKey: boolean };

export async function llmSettings(): Promise<LlmSettings> {
  if (!inTauri()) return { providers: [], activeProviderId: null, debugLogging: false };
  return invoke<LlmSettings>("llm_settings_get");
}

export async function saveProvider(provider: ProviderConfig): Promise<void> {
  requireBackend();
  return invoke<void>("llm_provider_save", { provider });
}

export async function removeProvider(id: string): Promise<void> {
  requireBackend();
  return invoke<void>("llm_provider_remove", { id });
}

/**
 * Sends the key to be sealed. There is no command that reads one back — an
 * empty string deletes the stored one.
 */
export async function saveApiKey(id: string, key: string): Promise<void> {
  requireBackend();
  return invoke<void>("llm_api_key_save", { id, key });
}

export async function setActiveProvider(id: string | null): Promise<void> {
  requireBackend();
  return invoke<void>("llm_active_provider_set", { id });
}

export async function setDebugLogging(enabled: boolean): Promise<void> {
  requireBackend();
  return invoke<void>("llm_debug_logging_set", { enabled });
}

/** A live call, so it is also what proves the URL and the key are both right. */
export async function listModels(id?: string): Promise<string[]> {
  requireBackend();
  return invoke<string[]>("llm_models_list", { id: id ?? null });
}

export async function readiness(): Promise<Readiness> {
  if (!inTauri()) return { workspace: null, provider: null, hasKey: false };
  return invoke<Readiness>("agent_readiness");
}

// ------------------------------------------------------------- previewing

export type FileDiffStats = {
  linesAdded: number;
  linesRemoved: number;
  unifiedDiff: string;
  truncated: boolean;
};

/** What a paused call would do, worked out without doing it. */
export type ToolPreview =
  | { kind: "diff"; path: string; diff: FileDiffStats }
  | { kind: "removes"; path: string; files: number }
  | { kind: "command"; command: string; cwd: string }
  | { kind: "failed"; reason: string }
  | { kind: "nothing" };

/** One round trip for the whole card: it shows every call the round asked for. */
export async function previewCalls(calls: PendingToolCall[]): Promise<ToolPreview[]> {
  if (!inTauri()) return calls.map(() => ({ kind: "nothing" }) as ToolPreview);
  return invoke<ToolPreview[]>("chat_preview", { calls });
}

// ------------------------------------------------------------ saved chats

/** A row in the sidebar. Not the whole conversation — see `chat_load`. */
export type ChatSummary = { id: string; title: string; updatedAt: number };

/**
 * One saved conversation. Two lists, because they are not the same list: the
 * model reads `messages`, the reader reads `blocks`, and one tool call is two
 * messages and one block.
 *
 * The shape is fixed on the other side in
 * `src-tauri/src/domain/chat_record.rs`, where `schemaVersion` says which
 * spelling of it this is.
 */
export type ChatRecord = {
  schemaVersion: number;
  id: string;
  workspace: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: LlmMessage[];
  blocks: Block[];
  todos: Task[];
  /** Absent in chats saved before plans existed. */
  plan?: string | null;
};

/** Chats of the open folder, newest first. No folder, no backend: no rows. */
export async function listChats(): Promise<ChatSummary[]> {
  if (!inTauri()) return [];
  return invoke<ChatSummary[]>("chat_list");
}

export async function loadChat(id: string): Promise<ChatRecord> {
  requireBackend();
  return invoke<ChatRecord>("chat_load", { id });
}

export async function saveChat(
  id: string,
  messages: LlmMessage[],
  blocks: Block[],
  todos: Task[],
  plan: string | null = null,
): Promise<ChatSummary> {
  requireBackend();
  return invoke<ChatSummary>("chat_save", { id, messages, blocks, todos, plan });
}

export async function deleteChat(id: string): Promise<void> {
  requireBackend();
  return invoke("chat_delete", { id });
}

// ---------------------------------------------------------------- skills

/** One folder of the skills directory. `error` is set when its SKILL.md did not parse. */
export type SkillListItem = { name: string; description: string; enabled: boolean; error: string | null };

/** `dir` is where skills go, so an empty list can say so. */
export type SkillsView = { dir: string; skills: SkillListItem[] };

export async function skillsList(): Promise<SkillsView> {
  if (!inTauri()) return { dir: "", skills: [] };
  return invoke<SkillsView>("skills_list");
}

export async function setSkillEnabled(name: string, enabled: boolean): Promise<void> {
  requireBackend();
  return invoke<void>("skills_set_enabled", { name, enabled });
}

// ---------------------------------------------------------------- project rules

/** An instruction file at the open folder's root. `error` when it cannot be sent; then it has no switch. */
export type RuleListItem = {
  name: string;
  path: string;
  enabled: boolean;
  content: string;
  truncated: boolean;
  error: string | null;
};

export async function rulesList(): Promise<RuleListItem[]> {
  if (!inTauri()) return [];
  return invoke<RuleListItem[]>("rules_list");
}

export async function setRuleEnabled(path: string, enabled: boolean): Promise<void> {
  requireBackend();
  return invoke<void>("rules_set_enabled", { path, enabled });
}

// ---------------------------------------------------------------- tool-call log

export type CallStatus = "ok" | "error" | "denied";

/** One settled call, as stored: arguments and result have their file content replaced by `<redacted>`. */
export type ToolLogRow = {
  id: number;
  tsMs: number;
  repoRoot: string;
  round: number;
  providerId: string;
  model: string;
  tool: string;
  /** `null` when the model's arguments did not parse. */
  args: { tool?: string; args?: Record<string, unknown> } | null;
  status: CallStatus;
  error: string | null;
  result: unknown;
  durationMs: number;
};

export type ToolLogFilter = {
  tool?: string;
  status?: CallStatus;
  search?: string;
  limit?: number;
  offset?: number;
};

export type ToolLogPage = { rows: ToolLogRow[]; total: number };

export async function toolLogQuery(filter: ToolLogFilter): Promise<ToolLogPage> {
  if (!inTauri()) return { rows: [], total: 0 };
  return invoke<ToolLogPage>("tool_log_query", { filter });
}

export async function toolLogClear(): Promise<number> {
  requireBackend();
  return invoke<number>("tool_log_clear");
}

export async function toolLogEnabled(): Promise<boolean> {
  if (!inTauri()) return true;
  return invoke<boolean>("tool_log_enabled_get");
}

export async function setToolLogEnabled(enabled: boolean): Promise<void> {
  requireBackend();
  return invoke<void>("tool_log_enabled_set", { enabled });
}

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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

/** Starts a turn. `messages` is the whole conversation: the transcript is the caller's. */
export async function startChat(
  turnId: string,
  messages: LlmMessage[],
  todos: Task[] = [],
): Promise<Outcome> {
  requireBackend();
  return invoke<Outcome>("chat_start", { turnId, messages, todos });
}

/** Continues a paused turn. `checkpoint` goes back exactly as it arrived. */
export async function resumeChat(
  turnId: string,
  checkpoint: Checkpoint,
  decisions: ToolCallDecision[],
): Promise<Outcome> {
  requireBackend();
  return invoke<Outcome>("chat_resume", { turnId, checkpoint, decisions });
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

import type { ChatUsage, Checkpoint, Outcome, TurnEvent } from "./chat";

// The transcript, assembled from one ordered stream of events.
//
// Pure and free of React on purpose: what a turn looks like is decided here,
// where it can be tested by feeding it events, and the components only render
// what comes out. The block shapes follow the prototype
// (`docs/prototype/atlas-cli.html`): a user bubble, the agent's prose, a list
// of tool items with an expandable detail, and an approval card.

export type ToolStatus = "running" | "done" | "failed";

export type Block =
  | { kind: "user"; id: string; text: string }
  | { kind: "message"; id: string; round: number; text: string }
  | { kind: "reasoning"; id: string; round: number; text: string }
  | { kind: "steer"; id: string; text: string }
  | {
      kind: "tool";
      id: string;
      round: number;
      name: string;
      /** Raw JSON. Turning it into something readable is the renderer's job. */
      arguments: string;
      status: ToolStatus;
      result?: unknown;
      error?: string;
      /** Live output of a running command, appended as it arrives. */
      output: string;
    }
  | {
      kind: "approval";
      id: string;
      round: number;
      /** One card per call the round asked for, decided or not. */
      calls: { id: string; name: string; arguments: string; requiresConfirmation: boolean }[];
    };

export type TurnStatus = "idle" | "running" | "awaitingApproval" | "done" | "cancelled";

export type TurnState = {
  blocks: Block[];
  status: TurnStatus;
  /** The last sequence number applied. Everything at or below it is a replay. */
  lastSeq: number;
  /** Events that arrived early, kept until the gap in front of them is filled. */
  buffered: TurnEvent[];
  usage: ChatUsage | null;
  retrying: { attempt: number; maxAttempts: number; delaySeconds: number } | null;
  /** Set when the turn pauses; sent back verbatim to continue it. */
  checkpoint: Checkpoint | null;
};

export const emptyTurn = (): TurnState => ({
  blocks: [],
  status: "idle",
  lastSeq: 0,
  buffered: [],
  usage: null,
  retrying: null,
  checkpoint: null,
});

export function appendUserMessage(state: TurnState, text: string): TurnState {
  return {
    ...state,
    status: "running",
    blocks: [...state.blocks, { kind: "user", id: `user:${state.blocks.length}`, text }],
  };
}

/**
 * Applies one event, in order.
 *
 * Three things can go wrong with a stream that crosses a process boundary, and
 * all three are handled here rather than by whoever renders it: an event can
 * arrive twice (dropped), early (buffered until its predecessor lands), or —
 * for command output alone — with no sequence number at all.
 *
 * That last one is not an oversight: command output is written from the
 * runner's reader threads, where the turn's cursor does not exist. It belongs
 * to the call named in its payload and is ordered against nothing.
 */
export function acceptEvent(state: TurnState, event: TurnEvent): TurnState {
  if (event.type === "commandOutput") return applyEvent(state, event);

  if (event.seq <= state.lastSeq) return state;
  if (event.seq > state.lastSeq + 1) {
    return { ...state, buffered: [...state.buffered, event] };
  }

  let next = applyEvent({ ...state, lastSeq: event.seq }, event);

  // The gap is filled; anything that was waiting on it may now apply, in order.
  let progressed = true;
  while (progressed) {
    progressed = false;
    const ready = next.buffered.find((e) => e.seq === next.lastSeq + 1);
    if (ready) {
      next = applyEvent(
        { ...next, lastSeq: ready.seq, buffered: next.buffered.filter((e) => e !== ready) },
        ready,
      );
      progressed = true;
    }
  }
  return next;
}

/** What the turn's own outcome adds: the pause, or the end. */
export function acceptOutcome(state: TurnState, outcome: Outcome): TurnState {
  if (outcome.status === "pendingApproval") {
    const checkpoint = outcome.value;
    return {
      ...state,
      status: "awaitingApproval",
      checkpoint,
      blocks: [
        ...state.blocks,
        {
          kind: "approval",
          id: `approval:${checkpoint.round}`,
          round: checkpoint.round,
          calls: checkpoint.calls,
        },
      ],
    };
  }
  return {
    ...state,
    status: outcome.status === "cancelled" ? "cancelled" : "done",
    checkpoint: null,
    retrying: null,
  };
}

/** Removes the pause once it has been answered, so the card does not linger. */
export function clearApproval(state: TurnState): TurnState {
  return {
    ...state,
    status: "running",
    checkpoint: null,
    blocks: state.blocks.filter((block) => block.kind !== "approval"),
  };
}

function applyEvent(state: TurnState, event: TurnEvent): TurnState {
  switch (event.type) {
    case "roundStarted":
      // Nothing to add — but the next prose must not join the previous round's
      // paragraph, so a fresh block is opened by `appendText` keying on the
      // round. Stated as an event rather than inferred, because a round that
      // ended in prose and was followed by another had its two answers
      // concatenated mid-sentence, permanently.
      return { ...state, retrying: null };

    case "delta":
      return appendText(state, "message", event.round, event.payload.delta);

    case "reasoning":
      return appendText(state, "reasoning", event.round, event.payload.delta);

    case "roundCompleted":
      // The authoritative text, replacing whatever the deltas built: a delta
      // lost on the way here is permanent once the transcript is saved.
      return setText(state, "message", event.round, event.payload.text);

    case "steeringApplied":
      return {
        ...state,
        blocks: [
          ...state.blocks,
          { kind: "steer", id: `steer:${event.payload.id}`, text: event.payload.text },
        ],
      };

    case "toolCallDelta":
    case "toolCall":
      return upsertTool(state, event.round, event.payload.id, (block) => ({
        ...block,
        name: event.payload.name || block.name,
        arguments: event.payload.arguments,
      }));

    case "toolResult":
      return upsertTool(state, event.round, event.payload.id, (block) => ({
        ...block,
        status: event.payload.error ? "failed" : "done",
        result: event.payload.result ?? undefined,
        error: event.payload.error ?? undefined,
      }));

    case "commandOutput":
      return upsertTool(state, event.round, event.payload.id, (block) => ({
        ...block,
        output: block.output + event.payload.chunk,
      }));

    case "contextUsage":
      return { ...state, usage: event.payload };

    case "retrying":
      return { ...state, retrying: event.payload };

    default:
      return state;
  }
}

function appendText(
  state: TurnState,
  kind: "message" | "reasoning",
  round: number,
  delta: string,
): TurnState {
  const existing = lastOf(state, kind, round);
  if (!existing) {
    return {
      ...state,
      blocks: [...state.blocks, { kind, id: `round:${round}:${kind}`, round, text: delta }],
    };
  }
  return replace(state, existing, { ...existing, text: existing.text + delta });
}

function setText(
  state: TurnState,
  kind: "message" | "reasoning",
  round: number,
  text: string,
): TurnState {
  const existing = lastOf(state, kind, round);
  // A round that only called tools says nothing, and an empty block would draw
  // an empty paragraph in the transcript.
  if (!existing) {
    if (!text) return state;
    return {
      ...state,
      blocks: [...state.blocks, { kind, id: `round:${round}:${kind}`, round, text }],
    };
  }
  return replace(state, existing, { ...existing, text });
}

function lastOf(state: TurnState, kind: "message" | "reasoning", round: number) {
  return state.blocks.find(
    (block): block is Extract<Block, { kind: "message" | "reasoning" }> =>
      block.kind === kind && block.round === round,
  );
}

/**
 * Tool blocks are addressed by the model's own call id, so a result applies to
 * the call it answers however many other calls and rounds happen in between.
 */
function upsertTool(
  state: TurnState,
  round: number,
  id: string,
  update: (block: Extract<Block, { kind: "tool" }>) => Extract<Block, { kind: "tool" }>,
): TurnState {
  const existing = state.blocks.find(
    (block): block is Extract<Block, { kind: "tool" }> => block.kind === "tool" && block.id === id,
  );
  if (!existing) {
    const fresh: Extract<Block, { kind: "tool" }> = {
      kind: "tool",
      id,
      round,
      name: "",
      arguments: "",
      status: "running",
      output: "",
    };
    return { ...state, blocks: [...state.blocks, update(fresh)] };
  }
  return replace(state, existing, update(existing));
}

function replace(state: TurnState, before: Block, after: Block): TurnState {
  return {
    ...state,
    blocks: state.blocks.map((block) => (block === before ? after : block)),
  };
}

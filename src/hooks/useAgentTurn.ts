import { useCallback, useEffect, useRef, useState } from "react";
import {
  cancelChat,
  loadChat,
  onTurnEvent,
  resumeChat,
  saveChat,
  startChat,
  steer as steerCommand,
  alwaysAllow,
  type LlmMessage,
  type Task,
  type ToolCallDecision,
} from "../lib/chat";
import {
  acceptEvent,
  acceptOutcome,
  appendUserMessage,
  clearApproval,
  emptyTurn,
  restoredTurn,
  type TurnState,
} from "../lib/chatTurnReducer";

// One conversation, driven from the window.
//
// The transcript lives here, not in the backend: `chat_start` takes the whole
// history and `chat_resume` takes the whole checkpoint, so this hook is the
// side that remembers. What it keeps is deliberately two things — the blocks a
// reader sees, and the messages the model sees. They are not the same list: a
// tool call is one block and two messages, and a collapsed detail is neither.
//
// Both halves are written to disk when a turn ends, which is what makes the
// conversation outlive the window. Not before it ends: a transcript saved
// mid-turn has a tool call in it with no result.

export function useAgentTurn({ onSaved }: { onSaved?: () => void } = {}) {
  const [turn, setTurn] = useState<TurnState>(emptyTurn);
  const [chatId, setChatId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const history = useRef<LlmMessage[]>([]);
  const todos = useRef<Task[]>([]);
  // Set when something happened that is worth writing down. Without it,
  // *opening* a chat would save it straight back and push it to the top of
  // the sidebar for having been read.
  const unsaved = useRef(false);
  // A turn's own id, so its events can be told from another turn's on the one
  // global channel.
  const turnId = useRef(0);
  const subscribed = useRef<(() => void) | null>(null);

  useEffect(() => () => subscribed.current?.(), []);

  const listen = useCallback(async (id: string) => {
    subscribed.current?.();
    const off = await onTurnEvent(id, (event) => setTurn((state) => acceptEvent(state, event)));
    subscribed.current = off;
  }, []);

  const finish = useCallback((outcome: Awaited<ReturnType<typeof startChat>>) => {
    setTurn((state) => acceptOutcome(state, outcome));
    if (outcome.status !== "pendingApproval") {
      todos.current = outcome.value.todos;
      // The assistant's answer joins the history, so the next turn sees it.
      if (outcome.value.text) {
        history.current = [
          ...history.current,
          { role: "assistant", content: outcome.value.text },
        ];
      }
      subscribed.current?.();
      subscribed.current = null;
    }
  }, []);

  // Saved once the turn has come to rest, from the render that has the last
  // block in it — which is why this is an effect and not the tail of `finish`,
  // where the final events have been dispatched but not yet applied.
  useEffect(() => {
    if (!unsaved.current) return;
    if (turn.status !== "done" && turn.status !== "cancelled") return;
    unsaved.current = false;

    const id = chatId ?? crypto.randomUUID();
    setChatId(id);
    saveChat(id, history.current, turn.blocks, todos.current)
      .then(() => onSaved?.())
      .catch((e) => setError(String(e)));
  }, [turn.status, turn.blocks, chatId, onSaved]);

  /** Sends what the user typed. While a turn is running the same text steers it. */
  const send = useCallback(
    async (text: string) => {
      const trimmed = text.trim();
      if (!trimmed) return;

      if (turn.status === "running") {
        await steerCommand(trimmed);
        return;
      }

      setError(null);
      unsaved.current = true;
      setTurn((state) => appendUserMessage(state, trimmed));
      history.current = [...history.current, { role: "user", content: trimmed }];

      const id = `turn-${++turnId.current}`;
      try {
        await listen(id);
        finish(await startChat(id, history.current, todos.current));
      } catch (e) {
        setError(String(e));
        setTurn((state) => ({ ...state, status: "done" }));
      }
    },
    [turn.status, listen, finish],
  );

  /** Answers the approval card. `always` widens the policy before continuing. */
  const decide = useCallback(
    async (decisions: ToolCallDecision[], always: string[] = []) => {
      const checkpoint = turn.checkpoint;
      if (!checkpoint) return;

      setTurn(clearApproval);
      const id = `turn-${turnId.current}`;
      try {
        for (const tool of always) await alwaysAllow(tool);
        finish(await resumeChat(id, checkpoint, decisions));
      } catch (e) {
        setError(String(e));
        setTurn((state) => ({ ...state, status: "done" }));
      }
    },
    [turn.checkpoint, finish],
  );

  const cancel = useCallback(async () => {
    await cancelChat();
  }, []);

  /** Reopens a saved conversation, transcript and model history both. */
  const open = useCallback(async (id: string) => {
    try {
      const record = await loadChat(id);
      subscribed.current?.();
      subscribed.current = null;
      history.current = record.messages;
      todos.current = record.todos;
      unsaved.current = false;
      setChatId(record.id);
      setError(null);
      setTurn(restoredTurn(record.blocks));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  /** Starts over. What was said is already on disk; this only stops pointing at it. */
  const reset = useCallback(() => {
    subscribed.current?.();
    subscribed.current = null;
    history.current = [];
    todos.current = [];
    unsaved.current = false;
    setChatId(null);
    setError(null);
    setTurn(emptyTurn());
  }, []);

  return { turn, chatId, error, send, decide, cancel, open, reset };
}

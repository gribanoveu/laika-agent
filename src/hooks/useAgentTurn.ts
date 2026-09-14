import { useCallback, useEffect, useRef, useState } from "react";
import {
  cancelChat,
  onTurnEvent,
  resumeChat,
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
  type TurnState,
} from "../lib/chatTurnReducer";

// One conversation, driven from the window.
//
// The transcript lives here, not in the backend: `chat_start` takes the whole
// history and `chat_resume` takes the whole checkpoint, so this hook is the
// side that remembers. What it keeps is deliberately two things — the blocks a
// reader sees, and the messages the model sees. They are not the same list: a
// tool call is one block and two messages, and a collapsed detail is neither.

export function useAgentTurn() {
  const [turn, setTurn] = useState<TurnState>(emptyTurn);
  const [error, setError] = useState<string | null>(null);
  const history = useRef<LlmMessage[]>([]);
  const todos = useRef<Task[]>([]);
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

  /** Starts over. The backend keeps nothing, so forgetting here is enough. */
  const reset = useCallback(() => {
    subscribed.current?.();
    subscribed.current = null;
    history.current = [];
    todos.current = [];
    setError(null);
    setTurn(emptyTurn());
  }, []);

  return { turn, error, send, decide, cancel, reset };
}

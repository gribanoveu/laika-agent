import { afterEach, describe, expect, mock, test } from "bun:test";
import { fireEvent, render, screen } from "@testing-library/react";

// The chip is the only way to reach `chat_set_mode`, and what it says has to
// be what the backend was actually told — a label reading "Plan" over a turn
// that is still armed is the failure worth a test.

const calls: { command: string; args: unknown }[] = [];
let refuse = false;

mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string, args: unknown) => {
    calls.push({ command, args });
    return refuse ? Promise.reject(new Error("no")) : Promise.resolve(null);
  },
  transformCallback: (callback: unknown) => callback,
}));

(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};

const { Composer } = await import("../components/Composer");
const { setConversationMode } = await import("../lib/chat");

afterEach(() => {
  calls.length = 0;
  refuse = false;
});

function composer(conversation: "agent" | "plan" | "ask", onConversation = () => {}) {
  return render(
    <Composer
      onSend={() => {}}
      onStop={() => {}}
      running={false}
      onNotify={() => {}}
      conversation={conversation}
      onConversation={onConversation}
    />,
  );
}

describe("the conversation-mode chip", () => {
  test("offers each mode and reports the one that was picked", () => {
    const picked: string[] = [];
    composer("agent", (mode) => picked.push(mode));

    fireEvent.click(screen.getByTitle("What the agent may do"));
    for (const label of ["Agent", "Plan", "Ask"]) {
      expect(screen.getByRole("option", { name: new RegExp(label) })).toBeTruthy();
    }

    fireEvent.click(screen.getByRole("option", { name: /Plan/ }));
    expect(picked).toEqual(["plan"]);
  });

  /// The wire value and the word on screen are different things: "plan" is
  /// what `domain::conversation_mode` deserializes, not a word for a menu.
  test("shows the word, not the wire value", () => {
    composer("plan");
    expect(screen.getByTitle("What the agent may do").textContent).toContain("Plan");
    expect(screen.getByTitle("What the agent may do").textContent).not.toContain("plan");
  });

  /// Two chips, two questions. Permissions decide who agrees before a call;
  /// the mode decides whether the call exists at all.
  test("does not replace the permission chip", () => {
    composer("agent");
    expect(screen.getByTitle("Permission mode")).toBeTruthy();
    expect(screen.getByTitle("What the agent may do")).toBeTruthy();
  });
});

describe("telling the backend", () => {
  test("the mode goes to the command that enforces it", async () => {
    await setConversationMode("ask");
    expect(calls).toEqual([{ command: "chat_set_mode", args: { mode: "ask" } }]);
  });

  /// A refusal has to reach the caller: the chip only moves after the backend
  /// agreed, and it can only do that if the failure is not swallowed here.
  test("a refused change is not reported as a success", async () => {
    refuse = true;
    expect(setConversationMode("plan")).rejects.toThrow();
  });
});

const { useConversationMode } = await import("../hooks/useConversationMode");
const { act, renderHook } = await import("@testing-library/react");

describe("holding the chosen mode", () => {
  /// The chip must not move ahead of the backend. Showing "Plan" over a turn
  /// that is still armed is worse than showing the old mode for a moment.
  test("the mode only changes once the backend has agreed", async () => {
    refuse = true;
    const { result } = renderHook(() => useConversationMode());

    let failed: string | null = null;
    await act(async () => {
      failed = await result.current.pick("plan");
    });

    expect(failed).toBeTruthy();
    expect(result.current.mode).toBe("agent");
  });

  test("a change the backend accepted is kept", async () => {
    const { result } = renderHook(() => useConversationMode());

    await act(async () => {
      expect(await result.current.pick("ask")).toBeNull();
    });

    expect(result.current.mode).toBe("ask");
    expect(calls).toEqual([{ command: "chat_set_mode", args: { mode: "ask" } }]);
  });
});

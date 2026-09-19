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

function composer(
  conversation: "agent" | "plan" | "ask",
  onConversation: (mode: "agent" | "plan" | "ask") => void = () => {},
  unattended = false,
  onUnattended: (unattended: boolean) => void = () => {},
) {
  return render(
    <Composer
      onSend={() => {}}
      onStop={() => {}}
      running={false}
      conversation={conversation}
      onConversation={onConversation}
      unattended={unattended}
      onUnattended={onUnattended}
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

describe("the permission chip", () => {
  /// It used to be three labels over a backend with two states, wired to
  /// nothing. A control that moves and changes nothing is worse than no
  /// control: it reads as a guarantee.
  test("offers exactly the two states the backend has", () => {
    composer("agent");
    fireEvent.click(screen.getByTitle("Permission mode"));

    const options = screen.getAllByRole("option").map((o) => o.textContent);
    expect(options).toEqual(["Askconfirm changes", "Autonever ask"]);
  });

  test("picking auto reports that nothing will be asked", () => {
    const picked: boolean[] = [];
    composer("agent", () => {}, false, (v) => picked.push(v));

    fireEvent.click(screen.getByTitle("Permission mode"));
    fireEvent.click(screen.getByRole("option", { name: /Auto/ }));
    expect(picked).toEqual([true]);
  });

  /// The label is the backend's state, not a click this component remembered.
  test("shows the state it was given", () => {
    composer("agent", () => {}, true);
    expect(screen.getByTitle("Permission mode").textContent).toContain("Auto");
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

const { useBackendSetting } = await import("../hooks/useBackendSetting");
const { act, renderHook } = await import("@testing-library/react");

describe("holding the chosen mode", () => {
  /// The chip must not move ahead of the backend. Showing "Plan" over a turn
  /// that is still armed is worse than showing the old mode for a moment.
  test("the mode only changes once the backend has agreed", async () => {
    refuse = true;
    const { result } = renderHook(() => useBackendSetting(setConversationMode, "agent"));

    let failed: string | null = null;
    await act(async () => {
      failed = await result.current.pick("plan");
    });

    expect(failed).toBeTruthy();
    expect(result.current.value).toBe("agent");
  });

  test("a change the backend accepted is kept", async () => {
    const { result } = renderHook(() => useBackendSetting(setConversationMode, "agent"));

    await act(async () => {
      expect(await result.current.pick("ask")).toBeNull();
    });

    expect(result.current.value).toBe("ask");
    expect(calls).toEqual([{ command: "chat_set_mode", args: { mode: "ask" } }]);
  });

  /// The chip the prototype drew and nobody connected. The command existed on
  /// the backend for two stages with no way to reach it.
  test("the permission chip reaches the policy that enforces it", async () => {
    const { setUnattended } = await import("../lib/chat");
    await setUnattended(true);
    expect(calls).toEqual([
      { command: "approval_set_unattended", args: { unattended: true } },
    ]);
  });
});

describe("text handed to the box", () => {
  const withDraft = (draft: { text: string; seq: number }) => (
    <Composer
      onSend={() => {}}
      onStop={() => {}}
      running={false}
      conversation="agent"
      onConversation={() => {}}
      unattended={false}
      onUnattended={() => {}}
      draft={draft}
    />
  );

  /// A branch gives its message back; the same text twice is two branches.
  test("replaces what was typed, each time it is handed", () => {
    const { rerender } = render(withDraft({ text: "try this", seq: 1 }));
    const box = screen.getByRole("textbox") as HTMLTextAreaElement;
    expect(box.value).toBe("try this");

    fireEvent.change(box, { target: { value: "edited" } });
    rerender(withDraft({ text: "try this", seq: 2 }));
    expect(box.value).toBe("try this");
  });
});

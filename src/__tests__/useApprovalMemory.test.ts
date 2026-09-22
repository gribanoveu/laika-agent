import { afterEach, describe, expect, mock, test } from "bun:test";

// Ask/Auto is put back from the backend's memory whenever the chat or folder
// changes; Auto coming back on by itself is said out loud.

const calls: { command: string; args: unknown }[] = [];
let restored = false;
let refuse = false;
// Set to hold restores back until the test lets them answer.
let gate: Promise<void> | null = null;
let scope = "repository";

mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string, args: unknown) => {
    calls.push({ command, args });
    if (refuse) return Promise.reject(new Error("no"));
    if (command === "approval_restore") {
      const answer = restored;
      return (gate ?? Promise.resolve()).then(() => answer);
    }
    if (command === "approval_remember_get") return Promise.resolve(scope);
    return Promise.resolve(null);
  },
  transformCallback: (callback: unknown) => callback,
}));

(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};

const { useApprovalMemory } = await import("../hooks/useApprovalMemory");
const { act, renderHook, waitFor } = await import("@testing-library/react");

afterEach(() => {
  calls.length = 0;
  restored = false;
  refuse = false;
  gate = null;
  scope = "repository";
});

const restores = () => calls.filter((c) => c.command === "approval_restore").map((c) => c.args);

describe("remembered Ask/Auto", () => {
  test("each chat and folder gets back what was chosen for it, and a return of Auto is said", async () => {
    let told = 0;
    restored = true;
    const { result, rerender } = renderHook(({ chat, folder }) => useApprovalMemory(chat, folder, () => told++), {
      initialProps: { chat: "c1" as string | null, folder: "/repo" },
    });
    await waitFor(() => expect(result.current.unattended).toBe(true));
    expect(result.current.remember).toBe("repository");
    expect(told).toBe(1);

    rerender({ chat: "c2", folder: "/repo" });
    await waitFor(() => expect(restores()).toContainEqual({ chatId: "c2" }));
    expect(told).toBe(1, "already on: nothing new to say");

    restored = false;
    rerender({ chat: null, folder: "/repo" });
    await waitFor(() => expect(result.current.unattended).toBe(false));
    expect(restores().at(-1)).toEqual({ chatId: null });
  });

  test("a pick is remembered for the chat it was made in, and only moves once accepted", async () => {
    const { result } = renderHook(() => useApprovalMemory("c1", "/repo", () => {}));
    await waitFor(() => expect(result.current.remember).toBe("repository"));
    await act(async () => {
      expect(await result.current.pick(true)).toBeNull();
    });
    expect(calls).toContainEqual({ command: "approval_set_unattended", args: { unattended: true, chatId: "c1" } });
    expect(result.current.unattended).toBe(true);

    refuse = true;
    await act(async () => {
      expect(await result.current.pick(false)).toBeTruthy();
    });
    expect(result.current.unattended).toBe(true);
  });

  test("a restore that answers after the user picked does not undo the pick", async () => {
    let open = () => {};
    gate = new Promise((resolve) => (open = resolve));
    scope = "chat"; // the default: loading it starts no second restore
    const { result } = renderHook(() => useApprovalMemory("c1", "/repo", () => {}));
    await act(async () => {
      expect(await result.current.pick(true)).toBeNull();
    });
    await act(async () => {
      open();
      await gate;
    });
    expect(result.current.unattended).toBe(true);
  });

  test("where it is remembered is saved, and asked again under the new rule", async () => {
    const { result } = renderHook(() => useApprovalMemory("c1", "/repo", () => {}));
    await waitFor(() => expect(result.current.remember).toBe("repository"));
    const before = restores().length;
    await act(async () => {
      expect(await result.current.pickRemember("chat")).toBeNull();
    });
    expect(calls).toContainEqual({ command: "approval_remember_set", args: { remember: "chat" } });
    await waitFor(() => expect(restores().length).toBeGreaterThan(before));
  });
});

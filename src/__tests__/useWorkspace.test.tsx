import { afterAll, afterEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook } from "@testing-library/react";

// The folder picker has two answers, and one of them is "the user changed
// their mind". Telling them apart is the whole of this hook's new part.

const calls: { command: string; args: unknown }[] = [];
let opened: string | null = null;

mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string, args: unknown) => {
    calls.push({ command, args });
    return Promise.resolve(command === "workspace_open" ? (args as { path: string }).path : null);
  },
  // The dialog plugin imports this from the same module; a mock that omits it
  // fails at import time rather than at the call.
  transformCallback: (callback: unknown) => callback,
}));

mock.module("@tauri-apps/plugin-dialog", () => ({
  open: () => Promise.resolve(opened),
}));

(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
afterAll(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

const { useWorkspace } = await import("../hooks/useWorkspace");

afterEach(() => {
  calls.length = 0;
  opened = null;
});

describe("picking a folder", () => {
  test("a chosen folder is opened", async () => {
    opened = "/Users/me/project";
    const { result } = renderHook(() => useWorkspace());

    await act(async () => {
      expect(await result.current.pick()).toBe(true);
    });

    expect(calls).toContainEqual({
      command: "workspace_open",
      args: { path: "/Users/me/project" },
    });
    expect(result.current.path).toBe("/Users/me/project");
  });

  /// Cancelling is not a path. Passing it on opened the workspace at
  /// whatever `null` spells once it has been through a string.
  test("cancelling opens nothing", async () => {
    opened = null;
    const { result } = renderHook(() => useWorkspace());

    await act(async () => {
      expect(await result.current.pick()).toBe(false);
    });

    expect(calls.filter((call) => call.command === "workspace_open")).toEqual([]);
    expect(result.current.path).toBeNull();
    // And it is not a failure either: a toast saying something went wrong is
    // a strange answer to having pressed Cancel.
    expect(result.current.error).toBeNull();
  });
});

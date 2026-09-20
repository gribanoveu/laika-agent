import { describe, expect, mock, test } from "bun:test";
import { renderHook, waitFor } from "@testing-library/react";

// The header's branch: asked for when the folder opens and again when a turn
// ends — the agent may have switched it — but not in the middle of one.

let branch: string | null = "main";
let asked = 0;

mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string) => {
    if (command === "workspace_branch") {
      asked++;
      return Promise.resolve(branch);
    }
    return Promise.resolve(null);
  },
  transformCallback: (callback: unknown) => callback,
}));

(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};

const { useGitBranch } = await import("../hooks/useGitBranch");

describe("the checked-out branch", () => {
  test("is read for the open folder and again after a turn, not during one", async () => {
    asked = 0;
    branch = "main";
    const { result, rerender } = renderHook(({ status }) => useGitBranch("/work/a", status), {
      initialProps: { status: "idle" },
    });
    await waitFor(() => expect(result.current).toBe("main"));

    branch = "feature/x";
    rerender({ status: "running" });
    expect(asked).toBe(1);

    rerender({ status: "done" });
    await waitFor(() => expect(result.current).toBe("feature/x"));
    expect(asked).toBe(2);
  });

  test("is nothing with no folder open", () => {
    asked = 0;
    const { result } = renderHook(() => useGitBranch(null, "idle"));
    expect(result.current).toBeNull();
    expect(asked).toBe(0);
  });
});

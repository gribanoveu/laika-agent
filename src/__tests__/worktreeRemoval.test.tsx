import { afterAll, afterEach, describe, expect, mock, test } from "bun:test";
import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import type { WorktreeCheck } from "../lib/chat";

// Removing the open worktree: shown first, then the window goes back to the
// main folder, and only then is the worktree deleted with its chats. Work
// that is not committed is never deleted.

const clean: WorktreeCheck = {
  main: "/work/kibo",
  branch: "kibo/main-0925-2214",
  dirty: [],
  ownCommits: 0,
  keepsBranch: false,
  chats: 2,
};
let check: WorktreeCheck = clean;
const calls: { command: string; args: unknown }[] = [];

mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string, args: unknown) => {
    calls.push({ command, args });
    if (command === "git_worktree_check") return Promise.resolve(check);
    if (command === "git_worktree_remove") return Promise.resolve({ branchKept: null, chatsRemoved: 2 });
    return Promise.resolve(null);
  },
  transformCallback: (callback: unknown) => callback,
}));
(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
afterAll(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});
afterEach(() => {
  check = clean;
  calls.length = 0;
});

const { useWorktreeRemoval, describeRemoval } = await import("../hooks/useWorktreeRemoval");
const { WorktreeRemoveDialog } = await import("../components/WorktreeRemoveDialog");
const { FolderTab } = await import("../components/FolderTab");

const WORKTREE = "/home/.kibo/worktrees/kibo/main-0925-2214";
const settle = () => act(() => new Promise((resolve) => setTimeout(resolve, 0)));

const removal = ({ agree = true, opens = true } = {}) => {
  const log: string[] = [];
  const hook = renderHook(() =>
    useWorktreeRemoval({
      workspace: WORKTREE,
      guard: (go, onCancel) => (agree ? go() : onCancel?.()),
      open: async (path) => {
        log.push(`open ${path}`);
        return opens;
      },
      notify: (message) => log.push(`notify ${message}`),
      refreshRecent: () => log.push("refresh"),
    }),
  );
  return { hook, log };
};

describe("removing the open worktree", () => {
  test("goes back to the main folder first, then removes the worktree — not the folder now open", async () => {
    const { hook, log } = removal();
    await act(() => hook.result.current.ask());
    expect(hook.result.current.asked).toEqual({ path: WORKTREE, check: clean });

    act(() => hook.result.current.confirm());
    await settle();
    expect(hook.result.current.asked).toBeNull();
    expect(calls.map((call) => call.command)).toEqual(["git_worktree_check", "git_worktree_remove"]);
    expect(calls[1].args).toEqual({ path: WORKTREE });
    expect(log).toEqual(["open /work/kibo", "notify Worktree removed with its 2 chats", "refresh"]);
  });

  test("removes nothing when leaving the worktree is not agreed to, or its main folder does not open", async () => {
    for (const setup of [{ agree: false }, { opens: false }]) {
      calls.length = 0;
      const { hook } = removal(setup);
      await act(() => hook.result.current.ask());
      act(() => hook.result.current.confirm());
      await settle();
      expect(calls.map((call) => call.command)).toEqual(["git_worktree_check"]);
    }
  });

  test("the toast says what went with it and what stayed", () => {
    expect(describeRemoval({ branchKept: null, chatsRemoved: 0 })).toBe("Worktree removed");
    expect(describeRemoval({ branchKept: null, chatsRemoved: 1 })).toBe("Worktree removed with its chat");
    expect(describeRemoval({ branchKept: "kibo/x", chatsRemoved: 3 })).toBe(
      "Worktree removed with its 3 chats — branch kibo/x kept",
    );
  });
});

describe("the dialog", () => {
  const dialog = (over: Partial<WorktreeCheck>) => {
    let confirmed = 0;
    render(
      <WorktreeRemoveDialog
        asked={{ path: WORKTREE, check: { ...clean, ...over } }}
        onConfirm={() => confirmed++}
        onClose={() => {}}
      />,
    );
    return () => confirmed;
  };

  test("says what goes and asks", () => {
    const confirmed = dialog({});
    const text = screen.getByRole("dialog").textContent ?? "";
    expect(text).toContain("The folder main-0925-2214 is deleted, and Kibo goes back to kibo.");
    expect(text).toContain("Its 2 chats are deleted with it.");
    expect(text).toContain("Branch kibo/main-0925-2214 is deleted");
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(confirmed()).toBe(1);
  });

  test("names the branch it keeps and why", () => {
    dialog({ ownCommits: 2, keepsBranch: true, chats: 1 });
    const text = screen.getByRole("dialog").textContent ?? "";
    expect(text).toContain("Its chat is deleted with it.");
    expect(text).toContain("Branch kibo/main-0925-2214 is kept — 2 commits are only on it.");
  });

  test("uncommitted work: lists it and offers no way to remove", () => {
    dialog({ dirty: ["src/a.ts", "notes.md"] });
    expect(screen.getAllByRole("listitem").map((li) => li.textContent)).toEqual(["src/a.ts", "notes.md"]);
    expect(screen.queryByRole("button", { name: "Remove" })).toBeNull();
    expect(screen.getByRole("dialog").textContent).toContain("Nothing was removed");
  });

  test("commits on no branch: refused as well", () => {
    dialog({ branch: null, ownCommits: 3 });
    expect(screen.getByRole("dialog").textContent).toContain("3 commits are on no branch");
    expect(screen.queryByRole("button", { name: "Remove" })).toBeNull();
  });
});

describe("the folder menu", () => {
  const menu = (worktreeOf: string | null) => {
    let asked = 0;
    render(
      <FolderTab
        path={WORKTREE}
        worktreeOf={worktreeOf}
        recent={[]}
        onOpenFolder={() => {}}
        onPickFolder={() => {}}
        onRemoveWorktree={() => asked++}
        onOpenChanges={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { expanded: false }));
    return () => asked;
  };

  test("offers removal only in a worktree", () => {
    const asked = menu("/work/kibo");
    fireEvent.click(screen.getByRole("option", { name: "Remove this worktree…" }));
    expect(asked()).toBe(1);
  });

  test("an ordinary folder has no such item", () => {
    menu(null);
    expect(screen.queryByRole("option", { name: "Remove this worktree…" })).toBeNull();
  });
});

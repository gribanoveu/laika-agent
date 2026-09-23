import { afterAll, beforeEach, describe, expect, mock, test } from "bun:test";
import { act, fireEvent, render, screen } from "@testing-library/react";
import type { WorkingChanges } from "../lib/chat";

// The Changes tab: the repository's unstaged and staged files with their line
// counts, moved between the two by the row's button, and committed.

let repo: WorkingChanges;
let calls: { command: string; args?: Record<string, unknown> }[] = [];
let commitFails: string | null = null;

mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    const paths = (args?.paths ?? []) as string[];
    const move = (from: "staged" | "unstaged", to: "staged" | "unstaged") => {
      const moved = repo[from].filter((f) => paths.includes(f.path));
      repo = { ...repo, [from]: repo[from].filter((f) => !paths.includes(f.path)), [to]: [...repo[to], ...moved] } as WorkingChanges;
    };
    if (command === "git_changes") return Promise.resolve(structuredClone(repo));
    if (command === "git_stage") return Promise.resolve(move("unstaged", "staged"));
    if (command === "git_unstage") return Promise.resolve(move("staged", "unstaged"));
    if (command === "git_commit") {
      if (commitFails) return Promise.reject(commitFails);
      repo = { ...repo, staged: [] };
      return Promise.resolve("abc1234");
    }
    return Promise.resolve(null);
  },
  transformCallback: (callback: unknown) => callback,
}));

/** The backend's events, by channel: what each listener would hear. */
const listeners = new Map<string, Set<(message: { payload: unknown }) => void>>();
const emit = (channel: string, payload: unknown) => listeners.get(channel)?.forEach((handler) => handler({ payload }));
mock.module("@tauri-apps/api/event", () => ({
  listen: (channel: string, handler: (message: { payload: unknown }) => void) => {
    if (!listeners.has(channel)) listeners.set(channel, new Set());
    listeners.get(channel)!.add(handler);
    return Promise.resolve(() => listeners.get(channel)!.delete(handler));
  },
}));

(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
afterAll(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

const { ChangesPanel } = await import("../components/ChangesPanel");
const settle = () => act(() => new Promise((resolve) => setTimeout(resolve, 0)));

beforeEach(() => {
  repo = {
    staged: [],
    unstaged: [
      { path: "src/main/tax/TaxProfileMapper.java", add: 4, del: 1 },
      { path: "README.md", add: 3, del: 0 },
    ],
  };
  calls = [];
  commitFails = null;
});

function panel(message = "", notes: string[] = [], messages: string[] = []) {
  return render(
    <ChangesPanel
      active
      workspace="/repo"
      onNotify={(note) => notes.push(note)}
      message={message}
      onMessage={(next) => messages.push(next)}
    />,
  );
}

describe("ChangesPanel", () => {
  test("lists the changed files by name, and folder, with their counts", async () => {
    panel();
    await settle();
    const name = screen.getByText("TaxProfileMapper.java");
    expect(name.parentElement?.title).toBe("src/main/tax/TaxProfileMapper.java — show diff");
    expect(name.parentElement?.textContent).toBe("TaxProfileMapper.javasrc/main/tax");
    // A file at the root has no folder line.
    expect(screen.getByText("README.md").parentElement?.textContent).toBe("README.md");
    expect(screen.getByText("+4")).toBeTruthy();
    expect(screen.getByText("-1")).toBeTruthy();
    expect(screen.getByText("Stage files to commit")).toBeTruthy();
  });

  test("a row's button stages it by path, and the lists are read back", async () => {
    panel();
    await settle();
    fireEvent.click(screen.getAllByTitle("Stage")[0]);
    await settle();
    expect(calls.find((c) => c.command === "git_stage")?.args).toEqual({ paths: ["src/main/tax/TaxProfileMapper.java"] });
    expect(screen.getAllByTitle("Unstage")).toHaveLength(1);

    fireEvent.click(screen.getByTitle("Unstage"));
    await settle();
    expect(calls.find((c) => c.command === "git_unstage")?.args).toEqual({ paths: ["src/main/tax/TaxProfileMapper.java"] });
    expect(screen.queryByTitle("Unstage")).toBeNull();
  });

  test("Stage all stages every unstaged file at once", async () => {
    panel();
    await settle();
    fireEvent.click(screen.getByText("Stage all"));
    await settle();
    expect(calls.find((c) => c.command === "git_stage")?.args).toEqual({
      paths: ["src/main/tax/TaxProfileMapper.java", "README.md"],
    });
    expect(screen.getByText("No unstaged changes")).toBeTruthy();
  });

  test("a commit sends the message, clears it and says the id", async () => {
    repo = { staged: repo.unstaged, unstaged: [] };
    const notes: string[] = [];
    const messages: string[] = [];
    panel("Fix the tax rate", notes, messages);
    await settle();
    fireEvent.click(screen.getByText("Commit"));
    await settle();
    expect(calls.find((c) => c.command === "git_commit")?.args).toEqual({ message: "Fix the tax rate" });
    expect(messages).toEqual([""]);
    expect(notes).toEqual(["Committed abc1234"]);
  });

  test("a failed commit keeps the message and says why", async () => {
    repo = { staged: repo.unstaged, unstaged: [] };
    commitFails = "git user.name and user.email are not set";
    const notes: string[] = [];
    const messages: string[] = [];
    panel("Fix", notes, messages);
    await settle();
    fireEvent.click(screen.getByText("Commit"));
    await settle();
    expect(messages).toEqual([]);
    expect(notes).toEqual(["git user.name and user.email are not set"]);
  });

  test("Commit waits for something staged and a message", async () => {
    panel("Fix");
    await settle();
    expect((screen.getByText("Commit") as HTMLButtonElement).disabled).toBe(true);
  });

  test("reads again when the folder or its .git changes, and only for the open folder", async () => {
    const view = panel();
    await settle();
    const reads = () => calls.filter((c) => c.command === "git_changes").length;
    const before = reads();

    act(() => emit("workspace-git:changed", { root: "/repo" }));
    await settle();
    expect(reads()).toBe(before + 1);

    act(() => emit("workspace-index:event", { root: "/repo", kind: "syncStarted" }));
    await settle();
    expect(reads()).toBe(before + 2);

    // Another folder's events, and the rest of a sync, are not a change here.
    act(() => {
      emit("workspace-git:changed", { root: "/elsewhere" });
      emit("workspace-index:event", { root: "/elsewhere", kind: "syncStarted" });
      emit("workspace-index:event", { root: "/repo", kind: "keywordsReady" });
    });
    await settle();
    expect(reads()).toBe(before + 2);

    // Hidden, the tab stops listening.
    view.rerender(<ChangesPanel active={false} workspace="/repo" onNotify={() => {}} message="" onMessage={() => {}} />);
    await settle();
    act(() => emit("workspace-git:changed", { root: "/repo" }));
    await settle();
    expect(reads()).toBe(before + 2);
  });
});

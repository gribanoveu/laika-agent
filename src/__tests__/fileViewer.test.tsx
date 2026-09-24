import { afterAll, beforeEach, describe, expect, mock, test } from "bun:test";
import { act, fireEvent, render, screen } from "@testing-library/react";
import type { FileTarget, FileView } from "../lib/chat";

// The viewer beside the chat: one file, its changes or the whole of it, read
// again when the folder changes.

let view: FileView;
let asked: unknown[] = [];

mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => {
    if (command === "file_view") {
      asked.push(args);
      return Promise.resolve(structuredClone(view));
    }
    return Promise.resolve(null);
  },
  transformCallback: (callback: unknown) => callback,
}));

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

const { FileViewer } = await import("../components/FileViewer");
const settle = () => act(() => new Promise((resolve) => setTimeout(resolve, 0)));

const ten = Array.from({ length: 10 }, (_, i) => `${i + 1}`).join("\n") + "\n";
beforeEach(() => {
  view = { old: ten, new: ten.replace("6\n", "six\n"), unviewable: null };
  asked = [];
});

async function open(target: FileTarget = { path: "src/main.rs", side: "unstaged" }, onClose = () => {}) {
  const shown = render(<FileViewer target={target} workspace="/repo" onClose={onClose} />);
  await settle();
  return shown;
}
const rows = () => [...document.querySelectorAll(".diff-row")].map((row) => row.textContent);

describe("FileViewer", () => {
  test("asks for the file on its side and shows what changed, named and counted", async () => {
    await open();
    expect(asked).toEqual([{ path: "src/main.rs", side: "unstaged" }]);
    expect(document.querySelector(".file-viewer-title")?.textContent).toBe("main.rssrc");
    expect(screen.getByText("Unstaged")).toBeTruthy();
    expect(screen.getByText("+1")).toBeTruthy();
    expect(screen.getByText("-1")).toBeTruthy();
    expect(rows()).toHaveLength(9); // the hunk header, three lines each side, the line removed and added
  });

  test("File shows every line with the change in place", async () => {
    await open();
    fireEvent.click(screen.getByRole("tab", { name: "File" }));
    expect(rows()).toHaveLength(11);
    expect(screen.getByRole("tab", { name: "File" }).getAttribute("aria-selected")).toBe("true");
  });

  test("a file with nothing changed is shown whole, and Diff is off", async () => {
    view = { old: "a\n", new: "a\n", unviewable: null };
    await open({ path: "a.txt", side: "worktree" });
    expect(rows()).toEqual(["11 a"]);
    expect((screen.getByRole("tab", { name: "Diff" }) as HTMLButtonElement).disabled).toBe(true);
    // The open folder's files carry no side label.
    expect(document.querySelector(".file-viewer-side")).toBeNull();
  });

  test("says why a binary or huge file is not drawn", async () => {
    view = { old: null, new: null, unviewable: "binary" };
    await open();
    expect(screen.getByText("A binary file — not shown.")).toBeTruthy();
  });

  test("Escape closes it", async () => {
    let closed = 0;
    await open(undefined, () => closed++);
    fireEvent.keyDown(document.querySelector(".file-viewer")!, { key: "Escape" });
    expect(closed).toBe(1);
  });

  test("reads again when the open folder changes, not another one", async () => {
    await open();
    view = { old: ten, new: ten.replace("2\n", "two\n"), unviewable: null };
    act(() => emit("workspace-index:event", { root: "/elsewhere", kind: "syncStarted" }));
    await settle();
    expect(asked).toHaveLength(1);
    act(() => emit("workspace-index:event", { root: "/repo", kind: "syncStarted" }));
    await settle();
    act(() => emit("workspace-git:changed", { root: "/repo" }));
    await settle();
    expect(asked).toHaveLength(3);
    expect(rows().some((row) => row?.includes("two"))).toBe(true);
  });
});

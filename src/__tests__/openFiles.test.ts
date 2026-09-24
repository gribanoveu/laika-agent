import { describe, expect, test } from "bun:test";
import { openFilesReducer, type OpenFiles } from "../hooks/useOpenFiles";
import type { FileTarget } from "../lib/chat";

// The viewer's tabs: one per file and side, and where the view goes when the
// tab showing is closed.

const a: FileTarget = { path: "a.rs", side: "unstaged" };
const aStaged: FileTarget = { path: "a.rs", side: "staged" };
const b: FileTarget = { path: "b.rs", side: "worktree" };
const c: FileTarget = { path: "c.rs", side: "worktree" };

const run = (actions: Parameters<typeof openFilesReducer>[1][], from: OpenFiles = { files: [], active: null }) =>
  actions.reduce(openFilesReducer, from);

describe("open files", () => {
  test("opening adds a tab and shows it; opening one already open only shows it", () => {
    const state = run([{ kind: "open", target: a }, { kind: "open", target: b }, { kind: "open", target: { ...a } }]);
    expect(state).toEqual({ files: [a, b], active: a });
  });

  test("the same path from another side is another tab", () => {
    expect(run([{ kind: "open", target: a }, { kind: "open", target: aStaged }]).files).toEqual([a, aStaged]);
  });

  test("closing the tab showing moves to the next, or the one before when it was last", () => {
    const three = run([{ kind: "open", target: a }, { kind: "open", target: b }, { kind: "open", target: c }]);
    expect(run([{ kind: "close", target: b }], { ...three, active: b })).toEqual({ files: [a, c], active: c });
    expect(run([{ kind: "close", target: c }], three)).toEqual({ files: [a, b], active: b });
    expect(run([{ kind: "close", target: a }, { kind: "close", target: b }, { kind: "close", target: c }], three)).toEqual({
      files: [],
      active: null,
    });
  });

  test("closing another tab keeps the one showing; closing one not open changes nothing", () => {
    const three = run([{ kind: "open", target: a }, { kind: "open", target: b }, { kind: "open", target: c }]);
    expect(run([{ kind: "close", target: a }], three)).toEqual({ files: [b, c], active: c });
    expect(run([{ kind: "close", target: aStaged }], three)).toBe(three);
    expect(run([{ kind: "closeAll" }], three)).toEqual({ files: [], active: null });
  });
});

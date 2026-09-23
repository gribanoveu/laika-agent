import { describe, expect, test } from "bun:test";
import { render, screen } from "@testing-library/react";
import type { Block } from "../lib/chatTurnReducer";
import { touchedFiles } from "../lib/touchedFiles";
import { FilesPanel } from "../components/FilesPanel";

// The Files tab's "In this chat": the files the agent's calls touched, read
// from the transcript — each once, the latest first, with what was done.

let n = 0;
const call = (name: string, args: unknown, result: unknown = {}, status: "done" | "failed" = "done"): Block => ({
  kind: "tool",
  id: `t${n++}`,
  round: 1,
  name,
  arguments: JSON.stringify(args),
  status,
  result,
  output: "",
});
const diff = (linesAdded: number, linesRemoved: number) => ({ diff: { linesAdded, linesRemoved, unifiedDiff: "", truncated: false } });

describe("touchedFiles", () => {
  test("each file once, the latest touched first, with what was done in order and the lines summed", () => {
    const files = touchedFiles(
      [
        call("readFile", { path: "src/a.ts" }, { content: "", startLine: 1, endLine: 3, totalLines: 3 }),
        call("readFile", { path: "README.md" }),
        call("editFile", { path: "src/a.ts" }, { path: "src/a.ts", ...diff(3, 1) }),
        call("editFile", { path: "src/a.ts" }, { path: "src/a.ts", ...diff(2, 2) }),
        call("readFile", { path: "src/a.ts" }),
      ],
      "/repo",
    );
    expect(files).toEqual([
      { path: "src/a.ts", touches: ["read", "edited"], add: 5, del: 3 },
      { path: "README.md", touches: ["read"], add: 0, del: 0 },
    ]);
  });

  test("a failed call did nothing, and a call that touches no file is not a file", () => {
    const files = touchedFiles(
      [
        call("editFile", { path: "src/a.ts" }, {}, "failed"),
        call("grep", { pattern: "x", path: "src" }),
        call("listFiles", { path: "src" }),
        call("runCommand", { command: "ls" }),
      ],
      "/repo",
    );
    expect(files).toEqual([]);
  });

  test("one file however its path was written: ./, the folder's absolute path", () => {
    const files = touchedFiles(
      [
        call("readFile", { path: "./src/a.ts" }),
        call("readFile", { path: "/repo/src/a.ts" }),
        call("writeFile", { path: "src/a.ts" }, { path: "src/a.ts", ...diff(1, 0) }),
      ],
      "/repo/",
    );
    expect(files).toEqual([{ path: "src/a.ts", touches: ["read", "written"], add: 1, del: 0 }]);
  });

  test("a move carries the history to the new path; a folder's move is not a file", () => {
    const files = touchedFiles(
      [
        call("editFile", { path: "old.ts" }, { path: "old.ts", ...diff(1, 1) }),
        call("move", { path: "old.ts", newPath: "lib/new.ts" }, { from: "old.ts", to: "lib/new.ts" }),
        call("move", { path: "src", newPath: "lib" }, { from: "src", to: "lib", files: 4 }),
      ],
      null,
    );
    expect(files).toEqual([{ path: "lib/new.ts", touches: ["edited", "moved"], add: 1, del: 1 }]);
  });

  test("a deletion counts the lines it removed", () => {
    const files = touchedFiles([call("deleteFile", { path: "gone.ts" }, { path: "gone.ts", ...diff(0, 12) })], null);
    expect(files).toEqual([{ path: "gone.ts", touches: ["deleted"], add: 0, del: 12 }]);
  });
});

describe("FilesPanel", () => {
  test("a row per file: name, folder, what was done, lines", () => {
    render(
      <FilesPanel
        active={false}
        workspace="/repo"
        blocks={[
          call("readFile", { path: "src/main/Tax.java" }),
          call("editFile", { path: "src/main/Tax.java" }, { path: "src/main/Tax.java", ...diff(4, 1) }),
          call("deleteFile", { path: "old.txt" }, { path: "old.txt", ...diff(0, 2) }),
        ]}
      />,
    );
    const row = screen.getByTitle("src/main/Tax.java");
    expect(row.textContent).toBe("Tax.javasrc/main" + "readedited" + "+4−1");
    expect(screen.getByText("2")).toBeTruthy();
    // Deleted last: the name says so too.
    expect(screen.getByTitle("old.txt").className).toContain("gone");
  });

  test("before the agent touches anything it says what will be here", () => {
    render(<FilesPanel active={false} workspace="/repo" blocks={[]} />);
    expect(screen.getByText(/Files the agent reads or changes/)).toBeTruthy();
  });
});

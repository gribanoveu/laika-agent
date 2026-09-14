import { describe, expect, test } from "bun:test";
import { describeTool } from "../lib/describeTool";
import type { Block } from "../lib/chatTurnReducer";

// What a tool call looks like in the transcript. Rendering, so the tests are
// about what reads well — and about not falling over on arguments that are
// still half-written.

const tool = (over: Partial<Extract<Block, { kind: "tool" }>>): Extract<Block, { kind: "tool" }> => ({
  kind: "tool",
  id: "c1",
  round: 1,
  name: "readFile",
  arguments: "{}",
  status: "done",
  output: "",
  ...over,
});

describe("what each call shows", () => {
  test("a read names the file and the lines it actually returned", () => {
    const shown = describeTool(
      tool({
        name: "readFile",
        arguments: '{"path":"TaxService.java","startLine":119}',
        result: { content: "line\n", startLine: 119, endLine: 124, totalLines: 400 },
      }),
    );

    expect(shown).toMatchObject({ name: "Read", arg: "TaxService.java", meta: "lines 119-124" });
    expect(shown.detail).toBe("line\n");
  });

  test("a search counts hits and the files they are in", () => {
    const shown = describeTool(
      tool({
        name: "grep",
        arguments: '{"pattern":"getIncome("}',
        result: {
          matches: [
            { path: "a.java", line: 1, text: "getIncome()" },
            { path: "a.java", line: 9, text: "getIncome()" },
            { path: "b.java", line: 4, text: "getIncome()" },
          ],
          truncated: false,
        },
      }),
    );

    expect(shown.meta).toBe("3 matches · 2 files");
    expect(shown.detail.split("\n")).toHaveLength(3);
  });

  /// A capped search that reads as exhaustive is the one thing grep must not
  /// do — the backend says so, and the row has to pass it on.
  test("a capped search says there are more", () => {
    const shown = describeTool(
      tool({
        name: "grep",
        result: { matches: [{ path: "a", line: 1, text: "x" }], truncated: true },
      }),
    );
    expect(shown.meta).toContain("+");
  });

  test("a write shows the size of the change and the diff itself", () => {
    const shown = describeTool(
      tool({
        name: "editFile",
        arguments: '{"path":"Mapper.java"}',
        result: { path: "Mapper.java", diff: { linesAdded: 4, linesRemoved: 1, unifiedDiff: "-a\n+b\n" } },
      }),
    );

    expect(shown).toMatchObject({ name: "Edit", arg: "Mapper.java", meta: "+4 -1" });
    expect(shown.detail).toContain("+b");
  });

  test("a command shows its exit code", () => {
    const shown = describeTool(
      tool({
        name: "runCommand",
        arguments: '{"command":"./gradlew test"}',
        result: { stdout: "BUILD SUCCESSFUL\n", stderr: "", exitCode: 0, timedOut: false },
      }),
    );

    expect(shown).toMatchObject({ name: "Bash", arg: "./gradlew test", meta: "exit 0" });
    expect(shown.detail).toContain("BUILD SUCCESSFUL");
  });

  /// A killed command has no exit code at all, and "exit 0" would read as
  /// success.
  test("a command killed by its timeout says that instead", () => {
    const shown = describeTool(
      tool({
        name: "runCommand",
        arguments: '{"command":"sleep 300"}',
        result: { stdout: "", stderr: "", exitCode: null, timedOut: true },
      }),
    );

    expect(shown.meta).toBe("timed out");
  });

  test("a running command shows what has streamed so far", () => {
    const shown = describeTool(
      tool({
        name: "runCommand",
        arguments: '{"command":"cargo test"}',
        status: "running",
        result: undefined,
        output: "Compiling…\n",
      }),
    );

    expect(shown.detail).toBe("Compiling…\n");
    expect(shown.meta).toBeUndefined();
  });

  test("a failed call shows the same message the model was given", () => {
    const shown = describeTool(
      tool({
        name: "writeFile",
        arguments: '{"path":"a.rs"}',
        status: "failed",
        error: "Error: read a.rs before writing to it",
      }),
    );

    expect(shown.meta).toBe("failed");
    expect(shown.detail).toContain("before writing");
  });

  test("a move names both ends", () => {
    const shown = describeTool(
      tool({ name: "move", arguments: '{"path":"old.rs","newPath":"new.rs"}' }),
    );
    expect(shown.arg).toBe("old.rs → new.rs");
  });
});

describe("arguments that are not finished yet", () => {
  /// The normal case while the model is still writing the call: the row has to
  /// draw something rather than throw.
  test("half-written JSON draws an empty argument, not an exception", () => {
    const shown = describeTool(tool({ name: "writeFile", arguments: '{"path":"a.r', status: "running" }));
    expect(shown.name).toBe("Write");
    expect(shown.arg).toBe("");
  });

  test("a tool this build has never heard of is shown by its own name", () => {
    const shown = describeTool(tool({ name: "summonDragon", arguments: "{}" }));
    expect(shown.name).toBe("summonDragon");
  });
});

import { describe, expect, test } from "bun:test";
import { diffRows, fileRows, type DiffRow } from "../lib/diffRows";

// How a unified diff becomes the rows the approval card draws: which line is
// which on both sides, and which words of an edited line actually changed.

const lines = (rows: DiffRow[]) =>
  rows.map((row) =>
    !("parts" in row)
      ? `${row.kind === "file" ? "## " : ""}${row.text}`
      : `${row.oldNo ?? "."} ${row.newNo ?? "."} ${row.kind} ${row.parts.map((p) => (p.changed ? `[${p.text}]` : p.text)).join("")}`,
  );

describe("rows of a diff", () => {
  test("numbers each side from the hunk header, and marks only the words that changed", () => {
    const rows = diffRows("@@ -9,4 +9,4 @@\n keep\n-see skills/a here\n-old words\n+see .agents/skills/a here\n+new words\n tail\n");
    expect(lines(rows)).toEqual([
      "@@ -9,4 +9,4 @@",
      "9 9 context keep",
      "10 . del see skills/a here",
      "11 . del [old] words",
      ". 10 add see [.agents/]skills/a here",
      ". 11 add [new] words",
      "12 12 context tail",
    ]);
  });

  test("a line with no partner is a whole addition, with no word marks", () => {
    const rows = diffRows("@@ -1,1 +1,2 @@\n-one\n+uno\n+two\n");
    expect(lines(rows)).toEqual(["@@ -1,1 +1,2 @@", "1 . del [one]", ". 1 add [uno]", ". 2 add two"]);
  });

  /// The backend cuts a long diff on a line boundary, which leaves the last
  /// hunk shorter than its header says.
  test("a diff cut short keeps the hunks that are whole", () => {
    const rows = diffRows("@@ -1 +1 @@\n-a\n+b\n@@ -20,3 +20,3 @@\n x\n-y\n");
    expect(lines(rows)).toEqual(["@@ -1,1 +1,1 @@", "1 . del [a]", ". 1 add [b]"]);
  });

  /// A directory's diff: each file under its own name, numbered from its own
  /// hunks.
  test("several files each start with their name", () => {
    const rows = diffRows("--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-one\n+two\n--- a/b.rs\n+++ b/b.rs\n@@ -3,0 +4 @@\n+new\n");
    expect(lines(rows)).toEqual(["## a.rs", "@@ -1,1 +1,1 @@", "1 . del [one]", ". 1 add [two]", "## b.rs", "@@ -4,0 +4,1 @@", ". 4 add new"]);
  });

  test("text that is not a diff gives no rows", () => {
    expect(diffRows("")).toEqual([]);
    expect(diffRows("@@ -1,5 +1,5 @@\n x\n")).toEqual([]);
  });
});

describe("rows of a file", () => {
  const old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
  const next = "1\n2\n3\n4\n5\nsix\n7\n8\n9\n10\n";

  test("the diff is the changed run with three lines each side, under its header", () => {
    expect(lines(fileRows(old, next, false))).toEqual([
      "@@ -3,7 +3,7 @@",
      "3 3 context 3",
      "4 4 context 4",
      "5 5 context 5",
      "6 . del [6]",
      ". 6 add [six]",
      "7 7 context 7",
      "8 8 context 8",
      "9 9 context 9",
    ]);
  });

  test("the whole file is every line, the change in place, and no header", () => {
    const rows = lines(fileRows(old, next, true));
    expect(rows).toHaveLength(11);
    expect(rows[0]).toBe("1 1 context 1");
    expect(rows.slice(5, 7)).toEqual(["6 . del [6]", ". 6 add [six]"]);
    expect(rows[10]).toBe("10 10 context 10");
  });

  test("an unchanged file is its lines, numbered, and a new one is all added", () => {
    expect(lines(fileRows("a\nb\n", "a\nb\n", true))).toEqual(["1 1 context a", "2 2 context b"]);
    expect(lines(fileRows("a\nb", "a\nb", false))).toEqual(["1 1 context a", "2 2 context b"]);
    expect(lines(fileRows("", "x\ny\n", true))).toEqual([". 1 add x", ". 2 add y"]);
    expect(fileRows("", "", false)).toEqual([]);
  });
});

import { describe, expect, test } from "bun:test";
import { act, fireEvent, render } from "@testing-library/react";
import { DiffView, rowStarts } from "../components/DiffView";
import { fileRows } from "../lib/diffRows";

// The viewer's DiffView draws only the rows in view: a file thousands of lines
// long is a few dozen elements, placed where they would be.

const text = Array.from({ length: 2000 }, (_, i) => `line ${i + 1}`).join("\n") + "\n";
const drawn = () => [...document.querySelectorAll<HTMLElement>(".diff-row")];

describe("DiffView, virtual", () => {
  test("draws a margin of rows, not the whole file, in a box as tall as all of them", () => {
    render(<DiffView rows={fileRows(text, text, true)} virtual />);
    const rows = drawn();
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(200);
    const height = parseFloat(rows[0].style.height);
    expect(parseFloat(document.querySelector<HTMLElement>(".diff-rows")!.style.height)).toBe(2000 * height);
    expect(rows[1].style.top).toBe(`${height}px`);
  });

  test("scrolled, it draws the rows there, each at its own place", () => {
    render(<DiffView rows={fileRows(text, text, true)} virtual />);
    const height = parseFloat(drawn()[0].style.height);
    const box = document.querySelector<HTMLElement>(".diff-scroll")!;
    act(() => {
      box.scrollTop = 1000 * height;
      fireEvent.scroll(box);
    });
    const line1001 = drawn().find((row) => row.textContent?.endsWith("line 1001"));
    expect(line1001?.style.top).toBe(`${1000 * height}px`);
    // The start of the file is no longer drawn.
    expect(Math.min(...drawn().map((row) => parseFloat(row.style.top)))).toBeGreaterThan(800 * height);
    expect(drawn().length).toBeLessThan(200);
  });

  test("without `virtual` every row is drawn, as the chat's diffs are", () => {
    render(<DiffView rows={fileRows("a\nb\n", "a\nc\n", true)} />);
    expect(drawn()).toHaveLength(3);
    expect(drawn()[0].style.top).toBe("");
  });
});

describe("where wrapped rows start", () => {
  const rows = fileRows("", "abcdefghij\n\tx\n\nabcd\n", true);

  test("unwrapped, every row is one line", () => {
    expect(rowStarts(rows, null)).toEqual([0, 1, 2, 3, 4]);
  });

  test("wrapped, a row takes as many lines as its columns fill, a tab four of them, an empty one still one", () => {
    // 10 characters in 4 columns: 3 lines; "\tx" is 5 wide: 2; "": 1; "abcd": exactly 1.
    expect(rowStarts(rows, { cols: 4, headerCols: 4 })).toEqual([0, 3, 5, 6, 7]);
  });

  test("a hunk header wraps by its own width", () => {
    const hunk = fileRows("1\n2\n3\n4\n5\n6\n7\n8\n9\n", "1\n2\n3\n4\nfive\n6\n7\n8\n9\n", false);
    expect(hunk[0].kind).toBe("hunk");
    const header = "@@ -2,7 +2,7 @@".length;
    expect(rowStarts(hunk, { cols: 100, headerCols: 5 })[1]).toBe(Math.ceil(header / 5));
  });
});

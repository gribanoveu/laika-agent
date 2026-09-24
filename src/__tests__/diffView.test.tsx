import { describe, expect, test } from "bun:test";
import { act, fireEvent, render } from "@testing-library/react";
import { DiffView } from "../components/DiffView";
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

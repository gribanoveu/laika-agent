import { diffWords, parsePatch } from "diff";

/** A run of text on a changed line; `changed` is the part that differs from its pair. */
export type DiffPart = { text: string; changed: boolean };

export type DiffRow =
  | { kind: "hunk"; text: string }
  | { kind: "context" | "add" | "del"; oldNo: number | null; newNo: number | null; parts: DiffPart[] };

/**
 * Rows to draw from a unified diff, with line numbers on both sides and, for a
 * line that was edited rather than replaced, the words that changed.
 *
 * A removed line is paired with the added line in the same position of the
 * run that follows it — the shape a one-word edit takes in a unified diff.
 * Unpaired lines are whole additions or removals and get no word marks.
 */
export function diffRows(unified: string): DiffRow[] {
  const rows: DiffRow[] = [];
  for (const hunk of hunks(unified)) {
    rows.push({ kind: "hunk", text: `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@` });
    let oldNo = hunk.oldStart;
    let newNo = hunk.newStart;
    const lines = hunk.lines.filter((line) => !line.startsWith("\\"));
    for (let i = 0; i < lines.length; ) {
      if (lines[i][0] === " ") {
        rows.push({ kind: "context", oldNo: oldNo++, newNo: newNo++, parts: whole(lines[i++]) });
        continue;
      }
      const dels: string[] = [];
      const adds: string[] = [];
      while (i < lines.length && lines[i][0] === "-") dels.push(lines[i++].slice(1));
      while (i < lines.length && lines[i][0] === "+") adds.push(lines[i++].slice(1));
      const pairs = dels.map((del, k) => (k < adds.length ? diffWords(del, adds[k]) : null));
      dels.forEach((del, k) => {
        const words = pairs[k];
        rows.push({
          kind: "del",
          oldNo: oldNo++,
          newNo: null,
          parts: words ? words.filter((w) => !w.added).map((w) => ({ text: w.value, changed: !!w.removed })) : [{ text: del, changed: false }],
        });
      });
      adds.forEach((add, k) => {
        const words = pairs[k];
        rows.push({
          kind: "add",
          oldNo: null,
          newNo: newNo++,
          parts: words ? words.filter((w) => !w.removed).map((w) => ({ text: w.value, changed: !!w.added })) : [{ text: add, changed: false }],
        });
      });
    }
  }
  return rows;
}

const whole = (line: string): DiffPart[] => [{ text: line.slice(1), changed: false }];

/**
 * The diff's hunks. A diff cut short for size ends in a hunk shorter than its
 * header says, which the parser refuses — that hunk is dropped rather than the
 * whole diff, and the caller already says the rest is not shown.
 */
function hunks(unified: string) {
  for (let text = unified; text; text = text.slice(0, Math.max(0, text.lastIndexOf("\n@@")))) {
    try {
      return parsePatch(text)[0]?.hunks ?? [];
    } catch {
      // Try again without the last hunk.
    }
  }
  return [];
}

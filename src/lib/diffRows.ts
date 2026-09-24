import { diffWords, parsePatch, structuredPatch, type StructuredPatchHunk } from "diff";

/**
 * A run of text on a changed line; `changed` is the part that differs from its
 * pair. `style` is a syntax colour, as Shiki gives it, once the file is coloured.
 */
export type DiffPart = { text: string; changed: boolean; style?: Record<string, string> };

/** A coloured file: per line, its tokens in order. */
export type Painted = { text: string; style?: Record<string, string> }[][];

export type DiffRow =
  | { kind: "file" | "hunk"; text: string }
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
  for (const patch of patches(unified)) {
    // A diff of several files names each; a single file's has no header.
    if (patch.newFileName) rows.push({ kind: "file", text: patch.newFileName.replace(/^b\//, "") });
    rows.push(...hunkRows(patch.hunks));
  }
  return rows;
}

/**
 * Rows for the file viewer: `old` against `next`, the changed runs with a few
 * lines around them — or, `full`, every line with the changes in place. A
 * file that did not change is its lines, numbered.
 */
export function fileRows(old: string, next: string, full: boolean): DiffRow[] {
  if (old === next) {
    return splitLines(next).map((text, i) => ({ kind: "context", oldNo: i + 1, newNo: i + 1, parts: [{ text, changed: false }] }));
  }
  const { hunks } = structuredPatch("", "", old, next, "", "", { context: full ? Number.MAX_SAFE_INTEGER : 3 });
  // One hunk holds the whole file: its header says nothing.
  return hunkRows(hunks).filter((row) => !full || row.kind !== "hunk");
}

/**
 * The rows with syntax colours laid over them: a removed line takes the old
 * file's colours, every other line the new one's. The word marks stay, cut at
 * the token boundaries. A line whose text does not match its tokens — the
 * colouring was of other text — stays as it was.
 */
export function paintRows(rows: DiffRow[], old: Painted | null, next: Painted | null): DiffRow[] {
  return rows.map((row) => {
    if (!("parts" in row)) return row;
    const tokens = row.kind === "del" ? old?.[row.oldNo! - 1] : next?.[row.newNo! - 1];
    return tokens ? { ...row, parts: paint(row.parts, tokens) } : row;
  });
}

function paint(parts: DiffPart[], line: Painted[number]): DiffPart[] {
  const tokens = line.filter((token) => token.text);
  if (parts.map((p) => p.text).join("") !== tokens.map((t) => t.text).join("")) return parts;
  const out: DiffPart[] = [];
  let at = 0; // into tokens[t]
  let t = 0;
  for (const part of parts) {
    for (let rest = part.text; rest; ) {
      const token = tokens[t];
      const take = Math.min(token.text.length - at, rest.length);
      out.push({ text: rest.slice(0, take), changed: part.changed, style: token.style });
      rest = rest.slice(take);
      at += take;
      if (at === token.text.length) [t, at] = [t + 1, 0];
    }
  }
  return out;
}

/** A file's lines, without the empty one after its last newline. */
function splitLines(text: string) {
  const all = text.split("\n");
  return all[all.length - 1] === "" ? all.slice(0, -1) : all;
}

function hunkRows(hunks: StructuredPatchHunk[]): DiffRow[] {
  const rows: DiffRow[] = [];
  for (const hunk of hunks) {
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
 * The diff's files, each with its hunks. A diff cut short for size ends in a
 * hunk shorter than its header says, which the parser refuses — that hunk is
 * dropped rather than the whole diff, and the caller already says the rest is
 * not shown.
 */
function patches(unified: string) {
  for (let text = unified; text; text = text.slice(0, Math.max(0, text.lastIndexOf("\n@@")))) {
    try {
      return parsePatch(text);
    } catch {
      // Try again without the last hunk.
    }
  }
  return [];
}

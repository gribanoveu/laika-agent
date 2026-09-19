/**
 * The assistant sometimes answers with a directory-tree diagram
 * (`├──`/`└──`/`│`) as plain prose, not inside a fenced code block. A
 * CommonMark paragraph's line breaks are "soft" — the raw text keeps its
 * `\n` characters, but everything downstream (the `<p>` element, ordinary
 * `white-space: normal` CSS, most non-`pre` renderers) is free to collapse
 * them, so the tree renders as one run-on line instead of a diagram. Fenced
 * code blocks don't have this problem: they're rendered as `<pre>`, which
 * always preserves whitespace. Rather than depend on paragraph CSS staying
 * exactly right forever, this wraps any unfenced tree-looking run of lines
 * in a fence before the content reaches the markdown renderer, so it always
 * gets the `<pre>` treatment regardless of how the model formatted it.
 */

const TREE_CHARS = /[├└│─┌┐┘┬┴┼]/;
const FENCE_RE = /^ {0,3}(`{3,}|~{3,})/;
/** A bare root-label line directly above the first tree line, e.g. `specs/`
 * or `repository` — absorbed into the fence so the label stays attached to
 * its diagram instead of being left outside as its own paragraph. */
const ROOT_LABEL_RE = /^\S+\/?$/;

export function wrapAsciiTrees(content: string): string {
  const lines = content.split("\n");
  const out: string[] = [];
  let inFence = false;
  let fenceMarker = "";

  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const fenceMatch = FENCE_RE.exec(line);
    if (fenceMatch) {
      if (!inFence) {
        inFence = true;
        [, fenceMarker] = fenceMatch;
      } else if (line.trimStart().startsWith(fenceMarker)) {
        inFence = false;
      }
      out.push(line);
      i += 1;
      continue;
    }

    if (inFence || !TREE_CHARS.test(line)) {
      out.push(line);
      i += 1;
      continue;
    }

    let start = i;
    const prev = out[out.length - 1];
    if (start > 0 && prev !== undefined && prev.trim() !== "" && !TREE_CHARS.test(prev) && ROOT_LABEL_RE.test(prev.trim())) {
      start -= 1;
      out.pop();
    }

    let end = i;
    while (end + 1 < lines.length && (TREE_CHARS.test(lines[end + 1]) || lines[end + 1].trim() === "")) {
      end += 1;
    }
    while (end > start && lines[end].trim() === "") {
      end -= 1;
    }

    out.push("```text", ...lines.slice(start, end + 1), "```");
    i = end + 1;
  }

  return out.join("\n");
}

import type { Block } from "./chatTurnReducer";
import type { LlmMessage } from "./chat";

/**
 * Where a branch can start: for each user message the model still sees, the
 * point in the model's history just before it.
 *
 * Matched from the end, bubble to message, by text. Both lists end with the
 * same turns, but the history may begin with a compaction summary where the
 * transcript still shows every bubble — and a bubble folded into that summary
 * cannot be branched from: the model no longer has what came before it, only
 * a digest that also covers what came after. A bubble with no message left to
 * match is where the summary starts, and everything before it is out of reach.
 *
 * A user message with no bubble is skipped rather than ending the walk: a
 * turn adds its own — a note typed while it ran, what a Stop hook said — and
 * the history now keeps them.
 */
export function branchPoints(blocks: Block[], history: LlmMessage[]): Map<string, number> {
  const bubbles = blocks.filter((block) => block.kind === "user");
  const asked = history.flatMap((message, at) => (message.role === "user" ? [at] : []));
  const points = new Map<string, number>();
  for (let b = bubbles.length - 1, m = asked.length - 1; b >= 0; b--, m--) {
    const bubble = bubbles[b];
    if (bubble.kind !== "user") break;
    while (m >= 0 && history[asked[m]].content !== bubble.text) m--;
    if (m < 0) break;
    points.set(bubble.id, asked[m]);
  }
  return points;
}

/**
 * The conversation as it stood just before `bubbleId` was sent, with that
 * message handed back for editing. `null` when the bubble cannot be branched
 * from (see `branchPoints`).
 */
export function branchAt(
  blocks: Block[],
  history: LlmMessage[],
  bubbleId: string,
): { blocks: Block[]; history: LlmMessage[]; text: string } | null {
  const cut = branchPoints(blocks, history).get(bubbleId);
  const at = blocks.findIndex((block) => block.id === bubbleId);
  const bubble = blocks[at];
  if (cut === undefined || bubble?.kind !== "user") return null;
  return { blocks: blocks.slice(0, at), history: history.slice(0, cut), text: bubble.text };
}

import type { Block } from "./chatTurnReducer";

/**
 * The plan the model wrote in `blocks`, or `null` if it wrote none there.
 *
 * Given only the blocks of the turn that just ended — never the whole
 * transcript: a plan the user edited after an earlier `writePlan` must not be
 * overwritten by that call when a later turn finishes without writing one.
 * The text comes from the call's arguments; its result is only a receipt.
 */
export function writtenPlan(blocks: Block[]): string | null {
  for (let i = blocks.length - 1; i >= 0; i--) {
    const block = blocks[i];
    if (block.kind !== "tool" || block.name !== "writePlan" || block.status !== "done") continue;
    try {
      const content = (JSON.parse(block.arguments) as { content?: unknown }).content;
      if (typeof content === "string" && content.trim()) return content.trim();
    } catch {
      // A call whose arguments did not parse never ran, so it is not "done".
    }
  }
  return null;
}

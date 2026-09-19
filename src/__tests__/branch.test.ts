import { describe, expect, test } from "bun:test";
import { branchAt, branchPoints } from "../lib/branch";
import type { Block } from "../lib/chatTurnReducer";
import type { LlmMessage } from "../lib/chat";

// Where a branch may start, and what it keeps. The transcript and the model's
// history are two lists; a branch has to cut both at the same place.

const user = (id: string, text: string): Block => ({ kind: "user", id, text });
const said = (role: "user" | "assistant", content: string): LlmMessage => ({ role, content });

const blocks: Block[] = [
  user("u0", "first"),
  { kind: "message", id: "m0", text: "one" },
  user("u1", "second"),
  { kind: "message", id: "m1", text: "two" },
];

describe("branching", () => {
  test("cuts both lists just before the message and hands it back", () => {
    const history = [said("user", "first"), said("assistant", "one"), said("user", "second"), said("assistant", "two")];

    expect(branchAt(blocks, history, "u1")).toEqual({
      blocks: blocks.slice(0, 2),
      history: history.slice(0, 2),
      text: "second",
    });
    expect(branchAt(blocks, history, "u0")).toEqual({ blocks: [], history: [], text: "first" });
  });

  /// The model has a summary where the first exchange was: nothing honest
  /// can be rebuilt from before the second message.
  test("a message folded into the summary cannot be branched from", () => {
    const history = [
      said("user", "[Compacted summary of earlier conversation]\n\nfirst, one"),
      said("user", "second"),
      said("assistant", "two"),
    ];

    expect([...branchPoints(blocks, history).keys()]).toEqual(["u1"]);
    expect(branchAt(blocks, history, "u0")).toBeNull();
    expect(branchAt(blocks, history, "u1")?.history).toEqual([history[0]]);
  });

  /// Walking back, the first message the two lists disagree on is where
  /// the summary begins. An older bubble whose text happens to match an
  /// older message is still behind it.
  test("nothing before the first mismatch is offered, even where the text matches", () => {
    const three: Block[] = [user("u0", "first"), user("u1", "second"), user("u2", "third")];
    const history = [said("user", "first"), said("user", "a summary"), said("user", "third")];

    expect([...branchPoints(three, history).keys()]).toEqual(["u2"]);
  });

  /// Only a bubble can start one.
  test("an id that is not a user message is refused", () => {
    const history = [said("user", "first"), said("assistant", "one"), said("user", "second")];
    expect(branchAt(blocks, history, "m0")).toBeNull();
    expect(branchAt(blocks, history, "nope")).toBeNull();
  });
});

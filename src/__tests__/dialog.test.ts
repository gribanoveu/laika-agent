import { describe, expect, test } from "bun:test";
import { safeName } from "../lib/dialog";

// A chat is named after the first thing the user typed, which is a sentence,
// not a file name.
describe("a file name from a chat title", () => {
  test("keeps the words and loses what a path cannot hold", () => {
    expect(safeName("why does src/lib.rs drop the token?", "md")).toBe(
      "why does src lib.rs drop the token.md",
    );
  });

  test("a title of nothing still names a file", () => {
    expect(safeName("   ", "md")).toBe("chat.md");
  });

  test("a long title is cut, so the name stays a name", () => {
    expect(safeName("x".repeat(200), "md")).toBe(`${"x".repeat(60)}.md`);
  });
});

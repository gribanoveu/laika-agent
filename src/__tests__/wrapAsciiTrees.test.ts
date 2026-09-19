import { describe, expect, test } from "bun:test";
import { wrapAsciiTrees } from "../lib/wrapAsciiTrees";

describe("wrapAsciiTrees", () => {
  test("wraps an unfenced tree diagram, absorbing its root-label line", () => {
    const input = [
      "Вот структура проекта:",
      "",
      "specs/",
      "├── api.yaml",
      "└── operations/",
      "    ├── Create.yaml",
      "    └── Get.yaml",
      "",
      "Итого: 3 файла.",
    ].join("\n");

    expect(wrapAsciiTrees(input)).toBe(
      [
        "Вот структура проекта:",
        "",
        "```text",
        "specs/",
        "├── api.yaml",
        "└── operations/",
        "    ├── Create.yaml",
        "    └── Get.yaml",
        "```",
        "",
        "Итого: 3 файла.",
      ].join("\n"),
    );
  });

  test("leaves prose without tree characters untouched", () => {
    const input = "Just a normal paragraph.\nWith a second line.";
    expect(wrapAsciiTrees(input)).toBe(input);
  });

  test("does not touch a tree that is already fenced", () => {
    const input = ["```", "root/", "└── file.txt", "```"].join("\n");
    expect(wrapAsciiTrees(input)).toBe(input);
  });

  test("ignores tree-looking characters inside an unrelated fenced block", () => {
    const input = ["```", "a │ b", "```"].join("\n");
    expect(wrapAsciiTrees(input)).toBe(input);
  });

  test("handles a tree diagram with no preceding root-label line", () => {
    const input = ["├── a.txt", "└── b.txt"].join("\n");
    expect(wrapAsciiTrees(input)).toBe(["```text", "├── a.txt", "└── b.txt", "```"].join("\n"));
  });
});

import { describe, expect, mock, test } from "bun:test";
import { render, waitFor } from "@testing-library/react";

mock.module("@tauri-apps/plugin-opener", () => ({ openUrl: async () => {} }));

const { Markdown } = await import("../components/Markdown");
const { highlight, resolveLanguage } = await import("../lib/highlight");

/** The repair of half-written markup drops text to bet on the next delta.
 * On a finished answer no delta is coming, and what the model wrote must stay. */
const finished: Array<[name: string, text: string, kept: string[]]> = [
  ["a bracket", "Take arr[0 and look at the result", ["arr[0", "result"]],
  ["an unclosed link", "See [the import docs", ["the import docs"]],
  ["an unclosed bold", "Bottom line: **important", ["important"]],
  ["an unclosed fence", "Code:\n```ts\nconst a = 1;", ["const a = 1;"]],
  ["underscores in names", "chat_store.rs and llm_chat.rs", ["chat_store", "llm_chat"]],
];

describe("Markdown", () => {
  for (const [name, text, kept] of finished) {
    test(`a finished answer keeps its text: ${name}`, () => {
      const { container } = render(<Markdown text={text} streaming={false} />);
      for (const part of kept) expect(container.textContent).toContain(part);
    });
  }

  test("draws the markup, not the characters", () => {
    const { container } = render(
      <Markdown text={"## Plan\n\n- **one**\n- two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nuse `rg`"} streaming={false} />,
    );
    expect(container.querySelector("h2")?.textContent).toBe("Plan");
    expect(container.querySelectorAll("li")).toHaveLength(2);
    expect(container.querySelector("strong")?.textContent).toBe("one");
    expect(container.querySelectorAll("td")).toHaveLength(2);
    expect(container.querySelector(".md-code-inline")?.textContent).toBe("rg");
    expect(container.textContent).not.toContain("**");
  });

  test("a fenced block has numbered lines and is coloured once closed", async () => {
    const { container } = render(<Markdown text={"```rust\nfn main() {\n}\n```"} streaming={false} />);
    const gutters = [...container.querySelectorAll(".md-code-gutter")].map((g) => g.textContent);
    expect(gutters).toEqual(["1", "2"]);
    await waitFor(() => expect(container.querySelector(".md-code-text span")).not.toBeNull());
    expect(container.querySelector(".md-code-text")?.textContent).toBe("fn main() {");
  });

  test("a task list draws its own boxes, without bullets; other items keep theirs", () => {
    const { container } = render(<Markdown text={"- [ ] todo\n- [x] done\n- plain"} streaming={false} />);
    expect(container.querySelector("input")).toBeNull();
    const boxes = [...container.querySelectorAll(".md-check")].map((b) => b.getAttribute("aria-checked"));
    expect(boxes).toEqual(["false", "true"]);
    expect([...container.querySelectorAll("li")].map((li) => li.className)).toEqual(["md-li md-task", "md-li md-task", "md-li"]);
  });

  test("a fence without a language is a block, its lines kept", () => {
    const { container } = render(<Markdown text={"```\nsrc/\n├── a/\n└── b/\n```"} streaming={false} />);
    expect(container.querySelector(".md-code-inline")).toBeNull();
    expect(container.querySelectorAll(".md-code-line")).toHaveLength(3);
  });

  test("a shell block hands its command to the terminal; other blocks and unfinished ones do not", () => {
    const pasted: string[] = [];
    const text = "```bash\nbun test\n```\n\n```ts\nconst a = 1;\n```";
    const { container, getAllByLabelText, rerender } = render(<Markdown text={text} streaming={false} onPaste={(c) => pasted.push(c)} />);
    const run = getAllByLabelText("Paste into terminal");
    expect(run).toHaveLength(1);
    run[0].click();
    expect(pasted).toEqual(["bun test"]);
    rerender(<Markdown text={"```bash\nrm -rf bu"} streaming onPaste={(c) => pasted.push(c)} />);
    expect(container.querySelector('[aria-label="Paste into terminal"]')).toBeNull();
    rerender(<Markdown text={text} streaming={false} />);
    expect(container.querySelector('[aria-label="Paste into terminal"]')).toBeNull();
  });

  test("a fence still streaming stays plain, not re-coloured on every delta", async () => {
    const { container } = render(<Markdown text={"Code:\n```rust\nfn main() {"} streaming={true} />);
    // The grammar is loaded and the highlighter answers — and still no colour.
    await highlight("fn main() {", "rust");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(container.querySelector(".md-code-text")?.textContent).toBe("fn main() {");
    expect(container.querySelector(".md-code-text span")).toBeNull();
  });

  test("a tree the model did not fence keeps its lines", () => {
    const { container } = render(<Markdown text={"src/\n├── a.rs\n└── b.rs"} streaming={false} />);
    expect(container.querySelector(".md-code")).not.toBeNull();
    expect(container.querySelectorAll(".md-code-line")).toHaveLength(3);
  });
  test("a link to a file opens it in the viewer, and a web link still goes to the browser", () => {
    const opened: string[] = [];
    const { getByText } = render(
      <Markdown
        text={"See [mutation](docs/11-mutation-testing.md), [readme](README.md:42) and [site](https://example.com)."}
        streaming={false}
        onOpenFile={(link) => opened.push(link)}
      />,
    );
    getByText("mutation").click();
    getByText("readme").click();
    getByText("site").click();
    expect(opened).toEqual(["docs/11-mutation-testing.md", "README.md:42"]);
    expect(getByText("site").getAttribute("href")).toBe("https://example.com/");
  });

  test("without a handler a file link is plain text, not a blocked one", () => {
    const { container } = render(<Markdown text={"See [notes](docs/notes.md)."} streaming={false} />);
    expect(container.querySelector("a")).toBeNull();
    expect(container.textContent).toBe("See notes.");
  });
});

describe("highlight", () => {
  test("colours for both themes, from the JavaScript engine", async () => {
    const tokens = await highlight("let x = 1;", "ts");
    const styles = tokens?.flat().map((t) => t.htmlStyle ?? {}) ?? [];
    expect(styles.some((s) => s["--shiki-light"] && s["--shiki-dark"])).toBe(true);
  });

  test("the same text asked again is the same answer, not a second tokenizing", async () => {
    const first = highlight("const same = 1;", "ts");
    expect(highlight("const same = 1;", "typescript")).toBe(first);
    expect(highlight("const other = 1;", "ts")).not.toBe(first);
    // The same text in another language is another answer.
    expect(highlight("const same = 1;", "rust")).not.toBe(first);
    expect((await first)?.[0]?.map((t) => t.content).join("")).toBe("const same = 1;");
  });

  test("an unknown language stays plain", async () => {
    expect(await highlight("x", "no-such-lang")).toBeNull();
    expect(await highlight("x", null)).toBeNull();
  });

  test("the tags models write resolve to grammars", () => {
    expect(resolveLanguage("bash")).toBe("shell");
    expect(resolveLanguage(" RS ")).toBe("rust");
    expect(resolveLanguage("tsx")).toBe("tsx");
  });
});

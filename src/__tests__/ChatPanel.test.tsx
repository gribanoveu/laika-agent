import { describe, expect, test } from "bun:test";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { ChatPanel, Preview, formatDuration } from "../components/ChatPanel";
import { emptyTurn, type Block, type TurnState } from "../lib/chatTurnReducer";
import { fromSnapshot, type IndexState } from "../lib/indexStatus";

// The transcript's own rules: who a block belongs to, and what the approval
// card sends back. Both are decided here rather than by the backend, so both
// are checked here.

const state = (blocks: Block[], over: Partial<TurnState> = {}): TurnState => ({
  ...emptyTurn(),
  blocks,
  ...over,
});

const panel = (
  turn: TurnState,
  onDecide = () => {},
  over: { index?: IndexState | null; onImplement?: () => void } = {},
) =>
  render(
    <ChatPanel
      workspace="/tmp/project"
      index={over.index}
      turn={turn}
      onDecide={onDecide}
      onOpenRepo={() => {}}
      onNewChat={() => {}}
      onImplement={over.onImplement}
    />,
  );

describe("handing a plan to Agent mode", () => {
  const answered = [
    { kind: "user", id: "u0", text: "plan the fix" },
    { kind: "message", id: "m1", round: 1, text: "1. read 2. fix" },
  ] as Block[];

  test("offered under a finished answer, and pressing it hands the plan over", () => {
    let handed = 0;
    panel(state(answered, { status: "done" }), undefined, { onImplement: () => handed++ });
    fireEvent.click(screen.getByText("Implement in Agent mode"));
    expect(handed).toBe(1);
  });

  test("not while the plan is being written, waiting, stopped, or outside Plan mode", () => {
    for (const status of ["running", "awaitingApproval", "cancelled"] as const) {
      const { unmount } = panel(state(answered, { status }), undefined, { onImplement: () => {} });
      expect(screen.queryByText("Implement in Agent mode")).toBeNull();
      unmount();
    }
    panel(state(answered, { status: "done" }));
    expect(screen.queryByText("Implement in Agent mode")).toBeNull();
  });

  test("not when the last thing shown is not the plan — a compaction note, say", () => {
    const noted = [...answered, { kind: "notice", id: "n1", text: "Earlier messages were summarized." }] as Block[];
    panel(state(noted, { status: "done" }), undefined, { onImplement: () => {} });
    expect(screen.queryByText("Implement in Agent mode")).toBeNull();
  });
});

describe("grouping", () => {
  test("everything after a question belongs to the answer", () => {
    panel(
      state([
        { kind: "user", id: "u0", text: "fix it" },
        { kind: "message", id: "m1", round: 1, text: "looking" },
        { kind: "tool", id: "c1", round: 1, name: "readFile", arguments: '{"path":"a.rs"}', status: "done", output: "" },
        { kind: "message", id: "m2", round: 2, text: "fixed" },
      ]),
    );

    // One "You" and one "Agent" label: the tool call did not open a third turn.
    expect(screen.getAllByText("You")).toHaveLength(1);
    expect(screen.getAllByText("Agent")).toHaveLength(1);
  });

  test("a second question opens a second turn", () => {
    panel(
      state([
        { kind: "user", id: "u0", text: "first" },
        { kind: "message", id: "m1", round: 1, text: "done" },
        { kind: "user", id: "u1", text: "second" },
      ]),
    );

    expect(screen.getAllByText("You")).toHaveLength(2);
  });

  test("with nothing said yet the empty state fills the thread", () => {
    panel(state([]));
    expect(screen.getByText("Start the conversation")).toBeDefined();
  });
});

describe("a tool row", () => {
  const read: Block = {
    kind: "tool",
    id: "c1",
    round: 1,
    name: "readFile",
    arguments: '{"path":"TaxService.java"}',
    status: "done",
    result: { content: "119  income = null;", startLine: 119, endLine: 124 },
    output: "",
  };

  test("shows what it did without being expanded", () => {
    panel(state([read]));

    expect(screen.getByText("Read")).toBeDefined();
    expect(screen.getByText("TaxService.java")).toBeDefined();
    expect(screen.getByText("lines 119-124")).toBeDefined();
    expect(screen.queryByText(/income = null/)).toBeNull();
  });

  test("and the body only when asked", () => {
    panel(state([read]));

    fireEvent.click(screen.getByText("Read"));
    expect(screen.getByText(/income = null/)).toBeDefined();
  });
});

describe("a run of calls", () => {
  const call = (id: string, name: string, status: "done" | "running" = "done"): Block => ({
    kind: "tool",
    id,
    round: 1,
    name,
    arguments: '{"path":"a.rs"}',
    status,
    output: "",
  });

  test("folds into one line, and a message between two runs keeps them apart", () => {
    const { container } = panel(
      state([
        call("c1", "readFile"),
        { kind: "reasoning", id: "r1", round: 1, text: "now grep" },
        call("c2", "grep"),
        { kind: "message", id: "m1", round: 1, text: "found it" },
        call("c3", "runCommand"),
      ] as Block[]),
    );
    const runs = [...container.querySelectorAll(".tool-run > summary")].map((s) => s.textContent);
    expect(runs).toEqual(["Read a file, searched a pattern", "Ran a command"]);
  });

  test("names the call under way while the turn runs", () => {
    const { container } = panel(state([call("c1", "readFile"), call("c2", "readFile", "running")], { status: "running" }));
    expect(container.querySelector(".tool-run-step")?.textContent).toBe("Reading a.rs…");
    expect(container.querySelector(".tool-run-count")?.textContent).toBe("2 calls");
  });

  test("a run the agent has written past is not under way", () => {
    const { container } = panel(
      state(
        [call("c1", "readFile"), { kind: "message", id: "m1", round: 1, text: "now" } as Block, call("c2", "grep", "running")],
        { status: "running" },
      ),
    );
    const live = [...container.querySelectorAll(".tool-run")].map((r) => r.classList.contains("live"));
    expect(live).toEqual([false, true]);
  });
});

/** The card asks the backend what its calls would do; let that answer land. */
const settle = () => act(async () => {});

describe("the approval card", () => {
  const approval: Block = {
    kind: "approval",
    id: "approval:1",
    round: 1,
    calls: [
      { id: "w1", name: "writeFile", arguments: '{"path":"a.rs"}', requiresConfirmation: true },
      { id: "l1", name: "listFiles", arguments: "{}", requiresConfirmation: false },
    ],
  };

  test("asks only about the calls that need an answer", async () => {
    panel(state([approval]));
    await settle();

    expect(screen.getByText(/Approval required/).textContent).toContain("Write");
    expect(screen.getByText(/Approval required/).textContent).not.toContain("List");
  });

  test("allowing answers every call that was asked about", async () => {
    const decided: unknown[] = [];
    panel(state([approval]), (decisions, always) => decided.push({ decisions, always }));
    await settle();

    fireEvent.click(screen.getByText("Allow"));

    expect(decided).toEqual([
      { decisions: [{ id: "w1", approved: true, reason: null }], always: [] },
    ]);
  });

  /// A model told only "denied" tries the same call again, then a near variant
  /// of it. The reason is what ends that in one round — so it has to reach the
  /// decision, not just the textbox.
  test("a refusal carries the reason that was typed", async () => {
    const decided: { decisions: { reason?: string | null }[] }[] = [];
    panel(state([approval]), (decisions) => decided.push({ decisions }));
    await settle();

    fireEvent.change(screen.getByPlaceholderText(/Why not/), {
      target: { value: "use the existing helper" },
    });
    fireEvent.click(screen.getByText("Deny"));

    expect(decided[0]?.decisions[0]?.reason).toBe("use the existing helper");
  });

  test("calls that need no answer fold into one line, counted", async () => {
    panel(
      state([
        {
          ...approval,
          calls: [
            { id: "w1", name: "writeFile", arguments: '{"path":"a.rs"}', requiresConfirmation: true },
            { id: "t1", name: "todo", arguments: '{"op":"update"}', requiresConfirmation: false },
            { id: "t2", name: "todo", arguments: '{"op":"update"}', requiresConfirmation: false },
            { id: "l1", name: "listFiles", arguments: "{}", requiresConfirmation: false },
          ],
        } as Block,
      ]),
    );
    await settle();

    const passive = document.querySelectorAll(".approval-cmd.passive");
    expect(passive).toHaveLength(1);
    expect(passive[0].textContent).toMatch(/×2, List/);
    expect(screen.queryByText("update")).toBeNull();
  });

  test("always allow names the tool it should stop asking about", async () => {
    const decided: { always: string[] }[] = [];
    panel(state([approval]), (_decisions, always) => decided.push({ always }));
    await settle();

    fireEvent.click(screen.getByText("Always"));

    expect(decided[0]?.always).toEqual(["writeFile"]);
  });
});

describe("what a call would do", () => {
  test("a write shows its diff, coloured line by line", () => {
    render(
      <Preview
        preview={{
          kind: "diff",
          path: "Mapper.java",
          diff: {
            linesAdded: 1,
            linesRemoved: 1,
            unifiedDiff: "@@ -1 +1 @@\n-return null;\n+return IncomeAmount.zero();\n",
            truncated: false,
          },
        }}
      />,
    );

    expect(screen.getByText("Mapper.java")).toBeDefined();
    expect(screen.getByText("+1")).toBeDefined();
    expect(document.querySelectorAll(".diff-add")).toHaveLength(1);
    expect(document.querySelectorAll(".diff-del")).toHaveLength(1);
    // Of the edited line, only what changed is marked.
    expect([...document.querySelectorAll(".diff-view mark")].map((m) => m.textContent)).toEqual(["null", "IncomeAmount.zero()"]);
  });

  /// Learning that an edit cannot apply after approving it costs a round and
  /// the user's trust in the card.
  test("an edit that cannot apply says so instead", () => {
    render(<Preview preview={{ kind: "failed", reason: "edit text not found: nowhere" }} />);

    expect(screen.getByText(/would not succeed/).textContent).toContain("not found");
  });

  /// The scariest thing to approve is a path that turned out broader than it
  /// looked.
  test("a recursive delete says how much it would take", () => {
    render(<Preview preview={{ kind: "removes", path: "src", files: 428 }} />);
    expect(screen.getByText(/428 files/)).toBeDefined();
  });

  test("a command says where it would run", () => {
    render(<Preview preview={{ kind: "command", command: "cargo test", cwd: "crate" }} />);
    expect(screen.getByText(/Runs in crate/)).toBeDefined();
  });

  test("a call with nothing to show draws nothing at all", () => {
    const { container } = render(<Preview preview={{ kind: "nothing" }} />);
    expect(container.textContent).toBe("");
  });

  test("and so does a call whose preview has not arrived yet", () => {
    const { container } = render(<Preview preview={undefined} />);
    expect(container.textContent).toBe("");
  });
});

describe("how long the agent worked", () => {
  const turn: Block[] = [
    { kind: "user", id: "u0", text: "fix it", workedMs: 83_000 },
    { kind: "message", id: "m1", round: 1, text: "fixed" },
  ] as Block[];

  test("reads like Claude Code's", () => {
    expect([formatDuration(12_400), formatDuration(83_000), formatDuration(3_900_000)]).toEqual([
      "12s",
      "1m 23s",
      "1h 5m",
    ]);
  });

  test("is said under a finished answer", () => {
    panel(state(turn, { status: "done" }));
    expect(screen.getByText("Worked for 1m 23s")).toBeTruthy();
  });

  test("ticks at the end of the thread while the turn runs", () => {
    panel(state([{ kind: "user", id: "u0", text: "fix it" }], { status: "running", runningSince: Date.now() - 5_000 }));
    expect(screen.getByRole("timer").textContent).toBe("Working… 5s");
    expect(screen.queryByText(/Worked for/)).toBeNull();
  });
});

describe("the header", () => {
  test("names the chat, and the folder under it", () => {
    render(
      <ChatPanel
        title="Fix the tax rounding"
        workspace="/tmp/project"
        turn={state([])}
        onDecide={() => {}}
        onOpenRepo={() => {}}
        onNewChat={() => {}}
      />,
    );
    expect(screen.getByRole("heading", { name: "Fix the tax rounding" })).toBeTruthy();
    expect(screen.getByTitle("/tmp/project").textContent).toBe("project");
  });

  test("shows and hides the side panel, and says which it would do", () => {
    let toggled = 0;
    const { rerender } = render(
      <ChatPanel
        workspace="/tmp/project"
        turn={state([])}
        onDecide={() => {}}
        onOpenRepo={() => {}}
        onNewChat={() => {}}
        onToggleAside={() => toggled++}
      />,
    );
    fireEvent.click(screen.getByTitle("Show changes"));
    expect(toggled).toBe(1);

    rerender(
      <ChatPanel
        workspace="/tmp/project"
        turn={state([])}
        onDecide={() => {}}
        onOpenRepo={() => {}}
        onNewChat={() => {}}
        asideOpen
        onToggleAside={() => toggled++}
      />,
    );
    expect(screen.getByTitle("Hide changes").getAttribute("aria-pressed")).toBe("true");
  });

  test("the … menu opens a side panel by name", () => {
    const opened: string[] = [];
    render(
      <ChatPanel
        workspace="/tmp/project"
        turn={state([])}
        onDecide={() => {}}
        onOpenRepo={() => {}}
        onNewChat={() => {}}
        onOpenPanel={(tab) => opened.push(tab)}
      />,
    );
    fireEvent.click(screen.getByTitle("More"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Terminal" }));

    expect(opened).toEqual(["terminal"]);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  test("the … menu offers the export, and closes once it is picked", () => {
    let exported = 0;
    render(
      <ChatPanel
        workspace="/tmp/project"
        turn={state([])}
        onDecide={() => {}}
        onOpenRepo={() => {}}
        onNewChat={() => {}}
        onExport={() => exported++}
      />,
    );
    expect(screen.queryByRole("menu")).toBeNull();

    fireEvent.click(screen.getByTitle("More"));
    fireEvent.click(screen.getByRole("menuitem", { name: /Export chat/ }));

    expect(exported).toBe(1);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  test("the … menu closes on Escape, and is not there with nothing in it", () => {
    const { unmount } = render(
      <ChatPanel
        workspace="/tmp/project"
        turn={state([])}
        onDecide={() => {}}
        onOpenRepo={() => {}}
        onNewChat={() => {}}
        onExport={() => {}}
      />,
    );
    fireEvent.click(screen.getByTitle("More"));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    unmount();

    render(
      <ChatPanel
        workspace="/tmp/project"
        turn={state([])}
        onDecide={() => {}}
        onOpenRepo={() => {}}
        onNewChat={() => {}}
      />,
    );
    expect(screen.queryByTitle("More")).toBeNull();
  });

  test("shows the checked-out branch under the title, the folder in its tooltip", () => {
    render(
      <ChatPanel
        branch="feature/parser"
        workspace="/tmp/project"
        turn={state([])}
        onDecide={() => {}}
        onOpenRepo={() => {}}
        onNewChat={() => {}}
      />,
    );
    expect(screen.getByTitle("/tmp/project").textContent).toBe("feature/parser");
  });

  test("a chat not saved yet is a new one", () => {
    panel(state([]));
    expect(screen.getByRole("heading", { name: "New chat" })).toBeTruthy();
  });
});

describe("a notice", () => {
  /// It is the app talking, not the agent. Under an "Agent" label it reads as
  /// something the model said about itself.
  test("belongs to neither side of the conversation", () => {
    panel(
      state([
        { kind: "user", id: "u0", text: "hi" },
        { kind: "notice", id: "n0", text: "Older history compacted" },
      ]),
    );

    expect(screen.getByText("Older history compacted")).toBeTruthy();
    expect(screen.queryByText("Agent")).toBeNull();
  });
});

describe("the folder's index", () => {
  test("its state is shown beside the folder, with the detail on hover", () => {
    const index = fromSnapshot({ root: "/tmp/project", syncing: false, embedded: 3, skipped: 2, embeddingError: null });
    panel(state([]), () => {}, { index });

    const badge = screen.getByRole("status");
    expect(badge.textContent).toBe("Indexed");
    expect(badge.getAttribute("title")).toContain("2 files not indexed");
  });

  test("nothing is shown before anything is known", () => {
    panel(state([]));
    expect(screen.queryByRole("status")).toBeNull();
  });
});

describe("branching from a message", () => {
  const blocks: Block[] = [
    { kind: "user", id: "u0", text: "folded away" },
    { kind: "user", id: "u1", text: "still seen" },
  ];
  const branchPanel = (branchable: ReadonlySet<string> | null, onBranch: (id: string) => void = () => {}) =>
    render(
      <ChatPanel
        workspace="/tmp/project"
        turn={state(blocks, { status: "done" })}
        onDecide={() => {}}
        onOpenRepo={() => {}}
        onNewChat={() => {}}
        branchable={branchable}
        onBranch={onBranch}
      />,
    );

  test("is offered where the model still sees the message, and says why not elsewhere", () => {
    const picked: string[] = [];
    branchPanel(new Set(["u1"]), (id) => picked.push(id));

    const [folded, seen] = screen.getAllByRole("button", { name: "Branch from here" }) as HTMLButtonElement[];
    expect(folded.disabled).toBe(true);
    expect(folded.title).toContain("Folded into the summary");
    expect(seen.disabled).toBe(false);

    fireEvent.click(seen);
    expect(picked).toEqual(["u1"]);
  });

  test("is not offered while a turn is under way", () => {
    branchPanel(null);
    expect(screen.queryAllByRole("button", { name: "Branch from here" })).toHaveLength(0);
  });
});

describe("a card that asks although its tool is always allowed", () => {
  test("says why, and an ordinary card says nothing extra", () => {
    const block: Block = {
      kind: "approval",
      id: "approval:2",
      round: 2,
      calls: [
        {
          id: "r1",
          name: "runCommand",
          arguments: '{"command":"git pf"}',
          requiresConfirmation: true,
          reason: "rewrites a remote (git push --force)",
        },
        { id: "r2", name: "runCommand", arguments: '{"command":"cargo test"}', requiresConfirmation: true },
      ],
    };
    panel(state([block], { status: "awaitingApproval" }));

    const why = screen.getAllByText(/Always asks:/);
    expect(why).toHaveLength(1);
    expect(why[0].textContent).toBe("Always asks: rewrites a remote (git push --force)");
  });
});

describe("the answer as Markdown", () => {
  // The repair of half-written markup drops everything after an unmatched
  // `![`: right for the answer still arriving, content loss for any other.
  const blocks = [
    { kind: "user", id: "u0", text: "go" },
    { kind: "message", id: "m1", round: 1, text: "Earlier ![one and the tail" },
    { kind: "message", id: "m2", round: 2, text: "Now ![two and more" },
  ] as Block[];

  test("only the last block of a running turn is repaired as it streams", () => {
    const { container } = panel(state(blocks, { status: "running" }));
    expect(container.textContent).toContain("and the tail");
    expect(container.textContent).not.toContain("and more");
  });

  test("once the turn ends, nothing is", () => {
    const { container } = panel(state(blocks, { status: "done" }));
    expect(container.textContent).toContain("and more");
  });
});

describe("a finished call's diff", () => {
  test("opens as a diff, not as text", () => {
    const { container } = panel(
      state([
        {
          kind: "tool",
          id: "d1",
          round: 1,
          name: "gitDiff",
          arguments: '{"path":"a.md"}',
          status: "done",
          output: "",
          result: { path: "a.md", label: "index → working tree", isBinary: false, diff: { linesAdded: 1, linesRemoved: 1, unifiedDiff: "@@ -1 +1 @@\n-one\n+uno\n", truncated: false } },
        } as Block,
      ]),
    );
    fireEvent.click(container.querySelector(".tool-item button.tool")!);

    expect(container.querySelector(".tool-detail")).toBeNull();
    expect(container.querySelectorAll(".diff-view .diff-add")).toHaveLength(1);
  });
});

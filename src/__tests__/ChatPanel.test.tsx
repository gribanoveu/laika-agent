import { describe, expect, test } from "bun:test";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { ChatPanel, Preview, formatDuration } from "../components/ChatPanel";
import { emptyTurn, type Block, type TurnState } from "../lib/chatTurnReducer";
import type { ContextUsage } from "../lib/chat";
import { fromSnapshot, type IndexState } from "../lib/indexStatus";

// The transcript's own rules: who a block belongs to, and what the approval
// card sends back. Both are decided here rather than by the backend, so both
// are checked here.

const state = (blocks: Block[], over: Partial<TurnState> = {}): TurnState => ({
  ...emptyTurn(),
  blocks,
  ...over,
});

const usage = (over: Partial<ContextUsage> = {}): ContextUsage => ({
  instructions: 1_000,
  tools: 3_000,
  conversation: 4_000,
  total: 8_000,
  limit: null,
  compactsAt: null,
  ...over,
});

const panel = (
  turn: TurnState,
  onDecide = () => {},
  over: { context?: ContextUsage | null; onCompact?: () => void; index?: IndexState | null; onImplement?: () => void } = {},
) =>
  render(
    <ChatPanel
      workspace="/tmp/project"
      index={over.index}
      turn={turn}
      usage={turn.usage}
      context={over.context === undefined ? usage() : over.context}
      onDecide={onDecide}
      onOpenRepo={() => {}}
      onNewChat={() => {}}
      onCompact={over.onCompact ?? (() => {})}
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
    expect(document.querySelectorAll(".ln-add")).toHaveLength(1);
    expect(document.querySelectorAll(".ln-del")).toHaveLength(1);
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
        usage={null}
        context={null}
        onDecide={() => {}}
        onOpenRepo={() => {}}
        onNewChat={() => {}}
        onCompact={() => {}}
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
        usage={null}
        context={null}
        onDecide={() => {}}
        onOpenRepo={() => {}}
        onNewChat={() => {}}
        onCompact={() => {}}
        onToggleAside={() => toggled++}
      />,
    );
    fireEvent.click(screen.getByTitle("Show panel"));
    expect(toggled).toBe(1);

    rerender(
      <ChatPanel
        workspace="/tmp/project"
        turn={state([])}
        usage={null}
        context={null}
        onDecide={() => {}}
        onOpenRepo={() => {}}
        onNewChat={() => {}}
        onCompact={() => {}}
        asideOpen
        onToggleAside={() => toggled++}
      />,
    );
    expect(screen.getByTitle("Hide panel").getAttribute("aria-pressed")).toBe("true");
  });

  test("a chat not saved yet is a new one", () => {
    panel(state([]));
    expect(screen.getByRole("heading", { name: "New chat" })).toBeTruthy();
  });
});

describe("the context meter", () => {
  const hi: Block[] = [{ kind: "user", id: "u0", text: "hi" }];
  const openMeter = () => fireEvent.click(screen.getByRole("button", { name: /Context usage/ }));

  /// Without the window, a number of tokens says nothing: 8k is nothing on a
  /// 200k model and the end of the road on a 8k one.
  test("shows how much of the window is gone, once the window is known", () => {
    panel(state(hi), () => {}, { context: usage({ limit: 200_000, compactsAt: 160_000 }) });

    expect(screen.getByRole("button", { name: "Context usage: 4%" })).toBeTruthy();
    openMeter();
    expect(screen.getByText("8k of 200k tokens")).toBeTruthy();
  });

  test("and asks for a compaction from its panel", () => {
    let asked = 0;
    panel(state(hi), () => {}, { onCompact: () => (asked += 1) });

    openMeter();
    fireEvent.click(screen.getByText("Compact now"));
    expect(asked).toBe(1);
    expect(screen.queryByRole("dialog", { name: "Context" })).toBeNull();
  });

  /// Mid-turn the history is the turn's, not the window's: shortening it from
  /// under a running request is not something to offer.
  test("but not while a turn is running", () => {
    panel(state(hi, { status: "running" }), () => {}, {});

    openMeter();
    expect((screen.getByText("Compact now") as HTMLButtonElement).disabled).toBe(true);
  });

  /// The failure this closes: the meter used to read the last turn's reported
  /// usage, so an untouched chat showed nothing at all — over a window with
  /// the prompt and the tool schemas already in it.
  test("is there before anything has been said", () => {
    panel(state([]), () => {}, { context: usage({ conversation: 0, total: 4_000, limit: 200_000 }) });

    expect(screen.getByRole("button", { name: "Context usage: 2%" })).toBeTruthy();
  });

  /// Folding the conversation moves one of the numbers. Showing only the
  /// total hides which one a click would help with.
  test("says what the tokens are spent on", () => {
    panel(state(hi), () => {}, { context: usage({ limit: 200_000, compactsAt: 160_000 }) });

    openMeter();
    const text = screen.getByRole("dialog", { name: "Context" }).textContent ?? "";
    expect(text).toContain("Instructions and tools4k");
    expect(text).toContain("Conversation4k");
    expect(text).toContain("at 160k");
  });

  /// This is an estimate. When the provider has said what the last request
  /// really cost, that number belongs next to it rather than behind it.
  test("puts the provider's own count beside the estimate, with the cached share", () => {
    panel(
      state(hi, {
        usage: { promptTokens: 9_500, completionTokens: 300, totalTokens: 9_800, cachedTokens: 9_000 },
      }),
    );

    openMeter();
    expect(screen.getByText("The last request actually cost 10k, 9k of it from the cache.")).toBeTruthy();
  });

  test("closes on Escape", () => {
    panel(state(hi));
    openMeter();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Context" })).toBeNull();
  });

  /// No window is a number with no scale — not a ring filled against a guess.
  test("fills no ring without a known window, and says why", () => {
    const { container } = panel(state(hi));

    expect(container.querySelector(".ctx-arc")).toBeNull();
    openMeter();
    expect(screen.getByText(/window is not known/)).toBeTruthy();
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
        usage={null}
        context={usage()}
        onDecide={() => {}}
        onOpenRepo={() => {}}
        onNewChat={() => {}}
        onCompact={() => {}}
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

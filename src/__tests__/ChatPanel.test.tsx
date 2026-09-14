import { describe, expect, test } from "bun:test";
import { render, screen, fireEvent } from "@testing-library/react";
import { ChatPanel } from "../components/ChatPanel";
import { emptyTurn, type Block, type TurnState } from "../lib/chatTurnReducer";

// The transcript's own rules: who a block belongs to, and what the approval
// card sends back. Both are decided here rather than by the backend, so both
// are checked here.

const state = (blocks: Block[], over: Partial<TurnState> = {}): TurnState => ({
  ...emptyTurn(),
  blocks,
  ...over,
});

const panel = (turn: TurnState, onDecide = () => {}) =>
  render(
    <ChatPanel
      workspace="/tmp/project"
      turn={turn}
      usage={turn.usage}
      onDecide={onDecide}
      onOpenRepo={() => {}}
      onNewChat={() => {}}
    />,
  );

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

  test("asks only about the calls that need an answer", () => {
    panel(state([approval]));

    expect(screen.getByText(/Approval required/).textContent).toContain("Write");
    expect(screen.getByText(/Approval required/).textContent).not.toContain("List");
  });

  test("allowing answers every call that was asked about", () => {
    const decided: unknown[] = [];
    panel(state([approval]), (decisions, always) => decided.push({ decisions, always }));

    fireEvent.click(screen.getByText("Allow"));

    expect(decided).toEqual([
      { decisions: [{ id: "w1", approved: true, reason: null }], always: [] },
    ]);
  });

  /// A model told only "denied" tries the same call again, then a near variant
  /// of it. The reason is what ends that in one round — so it has to reach the
  /// decision, not just the textbox.
  test("a refusal carries the reason that was typed", () => {
    const decided: { decisions: { reason?: string | null }[] }[] = [];
    panel(state([approval]), (decisions) => decided.push({ decisions }));

    fireEvent.change(screen.getByPlaceholderText(/Why not/), {
      target: { value: "use the existing helper" },
    });
    fireEvent.click(screen.getByText("Deny"));

    expect(decided[0]?.decisions[0]?.reason).toBe("use the existing helper");
  });

  test("always allow names the tool it should stop asking about", () => {
    const decided: { always: string[] }[] = [];
    panel(state([approval]), (_decisions, always) => decided.push({ always }));

    fireEvent.click(screen.getByText("Always"));

    expect(decided[0]?.always).toEqual(["writeFile"]);
  });
});

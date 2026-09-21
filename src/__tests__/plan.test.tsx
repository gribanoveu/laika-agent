import { afterAll, afterEach, describe, expect, mock, test } from "bun:test";
import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import type { Block } from "../lib/chatTurnReducer";
import type { TurnEvent } from "../lib/chat";

// The plan is a document the user reads and edits between turns. What must
// hold: the agent's newest plan reaches the tab, the user's edit reaches the
// next turn and the saved chat, and an old plan in the transcript never
// overwrites an edit made after it.

type Call = { command: string; args: Record<string, unknown> };
const calls: Call[] = [];
let emit: (event: TurnEvent) => void = () => {};
/** What the next `chat_start` "does" before it returns: the events it emits. */
let during: TurnEvent[] = [];
let record: unknown = null;

mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string, args: Record<string, unknown>) => {
    calls.push({ command, args });
    if (command === "chat_start") {
      for (const event of during) emit({ ...event, turnId: args.turnId as string });
      return Promise.resolve({ status: "done", value: { text: "", truncated: false, todos: [], history: args.messages } });
    }
    if (command === "chat_load") return Promise.resolve(record);
    return Promise.resolve(null);
  },
}));
mock.module("@tauri-apps/api/event", () => ({
  listen: (_: string, handler: (message: { payload: TurnEvent }) => void) => {
    emit = (event) => handler({ payload: event });
    return Promise.resolve(() => {});
  },
}));

(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
afterAll(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

const { useAgentTurn } = await import("../hooks/useAgentTurn");
const { writtenPlan } = await import("../lib/plan");
const { PlanPanel } = await import("../components/PlanPanel");
const { describeTool } = await import("../lib/describeTool");

afterEach(() => {
  calls.length = 0;
  during = [];
  record = null;
});

const tool = (id: string, name: string, args: unknown, status: "done" | "failed" = "done"): Block => ({
  kind: "tool",
  id,
  round: 1,
  name,
  arguments: JSON.stringify(args),
  status,
  output: "",
});

/** The events of a turn in which the model wrote `content` as the plan. */
const writes = (content: string): TurnEvent[] => [
  { turnId: "", seq: 1, round: 1, type: "toolCall", payload: { id: "p1", name: "writePlan", arguments: JSON.stringify({ content }) } },
  { turnId: "", seq: 2, round: 1, type: "toolResult", payload: { id: "p1", result: { result: "planWritten", lines: 1 } } },
];

const lastSave = () => calls.filter((c) => c.command === "chat_save").at(-1)?.args;
const lastStart = () => calls.filter((c) => c.command === "chat_start").at(-1)?.args;

describe("writtenPlan", () => {
  test("the newest plan that was written, not one that failed", () => {
    expect(
      writtenPlan([
        tool("a", "writePlan", { content: "# One" }),
        tool("b", "todo", { op: "write" }),
        tool("c", "writePlan", { content: "  # Two\n" }),
        tool("d", "writePlan", { content: "# Refused" }, "failed"),
      ]),
    ).toBe("# Two");
  });

  test("none written is none", () => {
    expect(writtenPlan([tool("a", "readFile", { path: "a.rs" })])).toBeNull();
    expect(writtenPlan([{ ...tool("a", "writePlan", {}), arguments: "{broken" } as Block])).toBeNull();
  });
});

describe("the plan through a conversation", () => {
  test("a plan the agent wrote reaches the tab, the saved chat and the next turn", async () => {
    const { result } = renderHook(() => useAgentTurn());
    during = writes("# Fix the parser");
    await act(async () => result.current.send("plan it"));

    expect(result.current.plan).toBe("# Fix the parser");
    expect(lastSave()?.plan).toBe("# Fix the parser");

    await act(async () => result.current.send("go on"));
    expect(lastStart()?.plan).toBe("# Fix the parser");
  });

  test("the user's edit is what the next turn gets — and a turn that writes none keeps it", async () => {
    const { result } = renderHook(() => useAgentTurn());
    during = writes("# Draft");
    await act(async () => result.current.send("plan it"));

    act(() => result.current.editPlan("# Draft, corrected"));
    expect(lastSave()?.plan).toBe("# Draft, corrected");

    during = [];
    await act(async () => result.current.send("looks right?"));
    expect(lastStart()?.plan).toBe("# Draft, corrected");
    // The old writePlan is still in the transcript; it must not win.
    expect(result.current.plan).toBe("# Draft, corrected");
    expect(lastSave()?.plan).toBe("# Draft, corrected");
  });

  test("a reopened chat brings its plan and its checklist; a new chat has neither", async () => {
    record = {
      id: "c1",
      messages: [],
      blocks: [tool("old", "writePlan", { content: "# Old" })],
      todos: [{ id: "t1", title: "read", status: "pending" }],
      plan: "# Edited later",
    };
    const { result } = renderHook(() => useAgentTurn());
    await act(async () => result.current.open("c1"));
    expect(result.current.plan).toBe("# Edited later");
    expect(result.current.checklist).toHaveLength(1);

    await act(async () => result.current.send("continue"));
    expect(lastSave()?.plan).toBe("# Edited later");

    act(() => result.current.reset());
    expect(result.current.plan).toBeNull();
    expect(result.current.checklist).toEqual([]);
  });

  test("a chat saved before plans existed opens without one", async () => {
    record = { id: "c1", messages: [], blocks: [], todos: [] };
    const { result } = renderHook(() => useAgentTurn());
    await act(async () => result.current.open("c1"));
    expect(result.current.plan).toBeNull();
  });
});

describe("the Plan tab", () => {
  const tab = (props: Partial<Parameters<typeof PlanPanel>[0]> = {}) =>
    render(<PlanPanel plan="# Fix" checklist={[]} onEdit={() => {}} locked={false} {...props} />);

  test("the plan is read as Markdown until the user asks to edit it", () => {
    tab({ plan: "# Fix the parser\n\n- read the grammar" });
    expect(screen.getByRole("heading", { name: "Fix the parser" })).toBeTruthy();
    expect(screen.queryByLabelText("Plan")).toBeNull();
    fireEvent.click(screen.getByLabelText("Edit plan"));
    expect((screen.getByLabelText("Plan") as HTMLTextAreaElement).value).toBe("# Fix the parser\n\n- read the grammar");
  });

  test("an edit is handed over when the user leaves the text, not per keystroke", () => {
    const edits: string[] = [];
    const { rerender } = tab({ onEdit: (p) => edits.push(p) });
    fireEvent.click(screen.getByLabelText("Edit plan"));
    const text = screen.getByLabelText("Plan");

    fireEvent.change(text, { target: { value: "# Fix\n\n1. first" } });
    expect(edits).toEqual([]);
    fireEvent.blur(text);
    expect(edits).toEqual(["# Fix\n\n1. first"]);

    // The parent keeps the edit; leaving the text again changes nothing.
    rerender(<PlanPanel plan="# Fix\n\n1. first" checklist={[]} onEdit={(p) => edits.push(p)} locked={false} />);
    fireEvent.blur(text);
    expect(edits).toHaveLength(1);
  });

  test("Done and Escape hand the edit over and go back to reading", () => {
    const edits: string[] = [];
    tab({ onEdit: (p) => edits.push(p) });
    fireEvent.click(screen.getByLabelText("Edit plan"));
    fireEvent.change(screen.getByLabelText("Plan"), { target: { value: "# One" } });
    fireEvent.click(screen.getByText("Done"));
    expect(edits).toEqual(["# One"]);
    expect(screen.queryByLabelText("Plan")).toBeNull();

    fireEvent.click(screen.getByLabelText("Edit plan"));
    fireEvent.change(screen.getByLabelText("Plan"), { target: { value: "# Two" } });
    fireEvent.keyDown(screen.getByLabelText("Plan"), { key: "Escape" });
    expect(edits).toEqual(["# One", "# Two"]);
    expect(screen.queryByLabelText("Plan")).toBeNull();
  });

  test("with no plan yet, one can be written from the empty panel", () => {
    tab({ plan: null });
    expect(screen.queryByText("Checklist")).toBeNull();
    fireEvent.click(screen.getByText("write your own"));
    expect(screen.getByLabelText("Plan")).toBeTruthy();
  });

  test("while a turn runs the plan cannot be edited", () => {
    tab({ locked: true });
    expect((screen.getByLabelText("Edit plan") as HTMLButtonElement).disabled).toBe(true);
  });

  test("the handover is offered only with a plan to hand over", () => {
    let handed = 0;
    const { unmount } = tab({ plan: null, onImplement: () => handed++ });
    expect(screen.queryByText("Implement in Agent mode")).toBeNull();
    unmount();
    tab({ onImplement: () => handed++ });
    fireEvent.click(screen.getByText("Implement in Agent mode"));
    expect(handed).toBe(1);
  });

  test("the checklist shows where the work stands", () => {
    tab({
      checklist: [
        { id: "1", title: "read", status: "completed" },
        { id: "2", title: "fix", status: "inProgress" },
        { id: "3", title: "rename", status: "cancelled", note: "not needed" },
      ],
    });
    expect(screen.getByText("1/3")).toBeTruthy();
    expect(screen.getByText("not needed")).toBeTruthy();
    expect(screen.getByText("fix").closest("li")?.className).toContain("inProgress");
    expect(screen.getByLabelText("Done")).toBeTruthy();
  });
});

test("a writePlan call in the transcript is shown by its title", () => {
  const block = tool("p1", "writePlan", { content: "# Fix the parser\n\n1. read" }) as Extract<Block, { kind: "tool" }>;
  const shown = describeTool({ ...block, result: { result: "planWritten", lines: 3 } });
  expect(shown).toMatchObject({ name: "Plan", arg: "Fix the parser", meta: "3 lines" });
});

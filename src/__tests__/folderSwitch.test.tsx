import { afterAll, afterEach, describe, expect, mock, test } from "bun:test";
import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import type { ProcessView } from "../lib/chat";

// Opening another folder stops the background processes and would file a
// running turn's chat in the wrong folder. The user is told before either.

let processes: ProcessView[] = [];
mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string) => Promise.resolve(command === "processes_list" ? processes : null),
  transformCallback: (callback: unknown) => callback,
}));
(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
afterAll(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});
afterEach(() => {
  processes = [];
});

const { useFolderSwitch } = await import("../hooks/useFolderSwitch");
const { FolderSwitchDialog } = await import("../components/FolderSwitchDialog");

const proc = (id: number, command: string, state: ProcessView["state"]): ProcessView => ({
  id,
  command,
  cwd: "/work/laika",
  state,
  tail: "",
});

describe("switching folders", () => {
  test("with nothing running, it just switches", async () => {
    processes = [proc(1, "bun test", { state: "exited", code: 0 })];
    let went = 0;
    const { result } = renderHook(() => useFolderSwitch(false));
    await act(() => result.current.guard(() => went++));
    expect(went).toBe(1);
    expect(result.current.blocked).toBeNull();
  });

  test("with processes running, it asks first and names only the running ones", async () => {
    processes = [proc(1, "bun run dev", { state: "running" }), proc(2, "bun test", { state: "exited", code: 0 })];
    let went = 0;
    const { result } = renderHook(() => useFolderSwitch(false));
    await act(() => result.current.guard(() => went++));
    expect(went).toBe(0);
    const blocked = result.current.blocked;
    expect(blocked?.kind === "processes" && blocked.processes.map((p) => p.command)).toEqual(["bun run dev"]);
  });

  /// The turn's chat is saved into whatever folder is open when it ends.
  test("while the agent works it is refused, whatever runs", async () => {
    let went = 0;
    const { result } = renderHook(() => useFolderSwitch(true));
    await act(() => result.current.guard(() => went++));
    expect(went).toBe(0);
    expect(result.current.blocked).toEqual({ kind: "agent" });
  });
});

describe("the dialog", () => {
  test("switches only when the user agrees to stop the processes", () => {
    let went = 0;
    let closed = 0;
    const blocked = { kind: "processes" as const, processes: [proc(1, "bun run dev", { state: "running" })], go: () => went++ };
    const { unmount } = render(
      <FolderSwitchDialog blocked={blocked} folder="/work/laika" onStopAgent={() => {}} onClose={() => closed++} />,
    );
    expect(screen.getByText("bun run dev")).toBeTruthy();
    fireEvent.click(screen.getByText("Cancel"));
    expect(went).toBe(0);
    fireEvent.click(screen.getByText("Stop them and switch"));
    expect(went).toBe(1);
    expect(closed).toBe(2);
    unmount();
  });

  test("offers to stop the agent, not to switch", () => {
    let stopped = 0;
    render(
      <FolderSwitchDialog blocked={{ kind: "agent" }} folder="/work/laika" onStopAgent={() => stopped++} onClose={() => {}} />,
    );
    expect(screen.getByRole("dialog").textContent).toContain("laika");
    fireEvent.click(screen.getByText("Stop the agent"));
    expect(stopped).toBe(1);
  });

  test("nothing blocked, nothing shown", () => {
    render(<FolderSwitchDialog blocked={null} folder="/work/laika" onStopAgent={() => {}} onClose={() => {}} />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

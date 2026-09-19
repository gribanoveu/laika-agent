import { afterAll, beforeEach, describe, expect, mock, test } from "bun:test";
import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import type { ProcessView } from "../lib/chat";

// The Terminal tab: the agent's background processes, asked for while the tab
// is open, newest first, with the end of what each wrote and a Stop for the
// ones still running.

let listed: ProcessView[];
let calls: { command: string; args?: Record<string, unknown> }[] = [];

mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    if (command === "processes_list") return Promise.resolve(structuredClone(listed));
    if (command === "process_stop") {
      listed = listed.map((p) => (p.id === args!.id ? { ...p, state: { state: "stopped" as const } } : p));
      return Promise.resolve(structuredClone(listed));
    }
    return Promise.resolve(null);
  },
  transformCallback: (callback: unknown) => callback,
}));

(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
afterAll(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

const { useProcesses } = await import("../hooks/useProcesses");
const { ProcessList } = await import("../components/ProcessList");
const settle = () => act(() => new Promise((resolve) => setTimeout(resolve, 0)));

beforeEach(() => {
  listed = [
    { id: 2, command: "npm run dev", cwd: "web", state: { state: "running" }, tail: "ready on :5173\n" },
    { id: 1, command: "cargo build", cwd: ".", state: { state: "exited", code: 101 }, tail: "error[E0433]\n" },
  ];
  calls = [];
});

describe("useProcesses", () => {
  test("asks while the tab is open, again a second later, and not once it closes", async () => {
    const { result, rerender } = renderHook(({ visible }) => useProcesses(visible), { initialProps: { visible: false } });
    await settle();
    expect(calls).toEqual([]);
    rerender({ visible: true });
    await settle();
    expect(result.current.processes.map((p) => p.id)).toEqual([2, 1]);
    await act(() => new Promise((resolve) => setTimeout(resolve, 1100)));
    expect(calls.filter((c) => c.command === "processes_list").length).toBe(2);
    rerender({ visible: false });
    await act(() => new Promise((resolve) => setTimeout(resolve, 1100)));
    expect(calls.filter((c) => c.command === "processes_list").length).toBe(2);
  });

  test("a stop is settled by what the backend says", async () => {
    const { result } = renderHook(() => useProcesses(true));
    await settle();
    await act(() => result.current.stop(2));
    expect(calls.at(-1)).toEqual({ command: "process_stop", args: { id: 2 } });
    expect(result.current.processes[0].state).toEqual({ state: "stopped" });
  });
});

describe("the process list", () => {
  test("shows each process and how it stands, the newest open, and Stop only for a running one", () => {
    const stopped: number[] = [];
    render(<ProcessList processes={listed} error={null} onStop={(id) => stopped.push(id)} />);
    expect(screen.getByText("npm run dev")).toBeTruthy();
    expect(screen.getByText("running")).toBeTruthy();
    expect(screen.getByText("exit 101")).toBeTruthy();
    expect(screen.getByText("ready on :5173")).toBeTruthy();
    expect(screen.queryByText("error[E0433]")).toBeNull();

    const stops = screen.getAllByText("Stop");
    expect(stops).toHaveLength(1);
    fireEvent.click(stops[0]);
    expect(stopped).toEqual([2]);
    // Stop does not fold the row it sits in.
    expect(screen.getByText("ready on :5173")).toBeTruthy();

    fireEvent.click(screen.getByText("cargo build"));
    expect(screen.getByText("error[E0433]")).toBeTruthy();
    expect(screen.queryByText("ready on :5173")).toBeNull();

    // A second click folds it: nothing open is a choice too.
    fireEvent.click(screen.getByText("cargo build"));
    expect(screen.queryByText("error[E0433]")).toBeNull();
    expect(screen.queryByText("ready on :5173")).toBeNull();
  });

  test("with nothing running it says what would", () => {
    render(<ProcessList processes={[]} error={null} onStop={() => {}} />);
    expect(screen.getByText(/No background processes/)).toBeTruthy();
  });
});

import { afterAll, beforeEach, describe, expect, mock, test } from "bun:test";
import { act, render, renderHook, screen } from "@testing-library/react";
import type { HooksView } from "../lib/chat";

// The hooks file is the user's: the tab re-reads it and lists each command
// with what it runs on, and a hook that will not run says why rather than
// sitting there looking configured.

let disk: HooksView;
let calls: string[] = [];

mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => {
    calls.push(command);
    if (command === "hooks_config_get") return Promise.resolve(structuredClone(disk));
    if (command === "hooks_config_save") {
      const text = args!.text as string;
      try {
        JSON.parse(text);
      } catch {
        return Promise.reject("the hooks configuration is not valid: EOF");
      }
      disk = { ...disk, text };
      return Promise.resolve(structuredClone(disk));
    }
    return Promise.resolve(null);
  },
  transformCallback: (callback: unknown) => callback,
}));

(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
afterAll(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

const { useHooks } = await import("../hooks/useHooks");
const { AsidePanel } = await import("../components/AsidePanel");
const settle = () => act(() => new Promise((resolve) => setTimeout(resolve, 0)));

beforeEach(() => {
  disk = {
    path: "/home/.atlas-desktop/hooks.json",
    text: '{\n  "hooks": {}\n}\n',
    hooks: [
      { event: "PreToolUse", matcher: "runCommand", command: "guard.sh", timeoutSecs: 60, problem: null },
      { event: "PostToolUse", matcher: "", command: "prettier --write", timeoutSecs: 30, problem: null },
      { event: "Stop", matcher: "", command: "say done", timeoutSecs: 60, problem: null },
      { event: "PreToolUse", matcher: "Bash", command: "from-claude", timeoutSecs: 60, problem: "names none of this app's tools" },
    ],
  };
  calls = [];
});

describe("useHooks", () => {
  test("reads the file when shown, and not while hidden", async () => {
    const { result, rerender } = renderHook(({ visible }) => useHooks(visible), { initialProps: { visible: false } });
    await settle();
    expect(calls).toEqual([]);
    rerender({ visible: true });
    await settle();
    expect(result.current.view?.hooks).toHaveLength(4);
  });

  test("a refused save says why and reports it was not stored", async () => {
    const { result } = renderHook(() => useHooks(true));
    await settle();
    let stored = true;
    await act(async () => {
      stored = await result.current.save("{");
    });
    expect(stored).toBe(false);
    expect(result.current.error).toContain("not valid");
    await act(async () => {
      stored = await result.current.save('{"hooks":{}}');
    });
    expect(stored).toBe(true);
    expect(result.current.error).toBeNull();
  });
});

describe("the Hooks tab", () => {
  test("lists what each hook runs on, and one that will not run says why", () => {
    let edits = 0;
    render(
      <AsidePanel
        tab="hooks"
        onTabChange={() => {}}
        onNotify={() => {}}
        mcp={null}
        mcpError={null}
        onMcpToggle={() => {}}
        onMcpEdit={() => {}}
        hooks={disk}
        hooksError={null}
        onHooksEdit={() => edits++}
        skills={null}
        skillsError={null}
        onSkillToggle={() => {}}
        rules={[]}
        rulesError={null}
        onRuleToggle={() => {}}
        plan={null}
        checklist={[]}
        onPlanEdit={() => {}}
        planLocked={false}
      />,
    );
    expect(screen.getByText("PreToolUse · runCommand")).toBeTruthy();
    expect(screen.getByText("PostToolUse · every tool")).toBeTruthy();
    expect(screen.getByText("Stop")).toBeTruthy();
    expect(screen.getByText("Stopped after 30 s")).toBeTruthy();
    expect(screen.getByText("won't run")).toBeTruthy();
    expect(screen.getByText("names none of this app's tools")).toBeTruthy();
    expect(screen.getByText("3/4")).toBeTruthy();
    act(() => screen.getByText("Edit hooks").click());
    expect(edits).toBe(1);
  });
});

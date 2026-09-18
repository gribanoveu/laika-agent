import { afterAll, beforeEach, describe, expect, mock, test } from "bun:test";
import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import type { McpView } from "../lib/chat";

// The MCP file is the user's: the tab re-reads it, a switch is settled by
// what was saved, and a config that does not parse leaves the file alone and
// the editor open with the reason.

let disk: McpView;
let calls: string[] = [];

mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => {
    calls.push(command);
    if (command === "mcp_config_get") return Promise.resolve(structuredClone(disk));
    if (command === "mcp_config_save") {
      const text = args!.text as string;
      try {
        JSON.parse(text);
      } catch {
        return Promise.reject("the MCP configuration is not valid: EOF");
      }
      disk = { ...disk, text, servers: [{ name: "pasted", command: "npx x", enabled: true, error: null }] };
      return Promise.resolve(structuredClone(disk));
    }
    if (command === "mcp_server_set_enabled") {
      disk.servers = disk.servers.map((s) => (s.name === args!.name ? { ...s, enabled: args!.enabled as boolean } : s));
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

const { useMcp } = await import("../hooks/useMcp");
const { AsidePanel } = await import("../components/AsidePanel");
const { McpConfig } = await import("../components/McpConfig");
const settle = () => act(() => new Promise((resolve) => setTimeout(resolve, 0)));

beforeEach(() => {
  disk = {
    path: "/home/.atlas-desktop/mcp.json",
    text: '{\n  "mcpServers": {}\n}\n',
    servers: [
      { name: "github", command: "npx -y server-github", enabled: true, error: null },
      { name: "remote", command: "", enabled: true, error: "HTTP servers are not supported yet" },
    ],
  };
  calls = [];
});

describe("useMcp", () => {
  test("reads the file when shown, and not while hidden", async () => {
    const { result, rerender } = renderHook(({ visible }) => useMcp(visible), { initialProps: { visible: false } });
    await settle();
    expect(calls).toEqual([]);
    rerender({ visible: true });
    await settle();
    expect(result.current.view?.servers.map((s) => s.name)).toEqual(["github", "remote"]);
  });

  test("a switch is what the backend saved", async () => {
    const { result } = renderHook(() => useMcp(true));
    await settle();
    await act(() => result.current.setEnabled("github", false));
    expect(result.current.view?.servers[0].enabled).toBe(false);
    expect(calls).toContain("mcp_server_set_enabled");
  });

  test("a refused save says why and reports it was not stored", async () => {
    const { result } = renderHook(() => useMcp(true));
    await settle();
    let stored = true;
    await act(async () => {
      stored = await result.current.save("{");
    });
    expect(stored).toBe(false);
    expect(result.current.error).toContain("not valid");
  });
});

describe("the MCP tab", () => {
  const panel = (view: McpView, onEdit = () => {}, onToggle = (_: string, __: boolean) => {}) =>
    render(
      <AsidePanel
        tab="mcp"
        onTabChange={() => {}}
        collapsed={false}
        onToggleCollapse={() => {}}
        onNotify={() => {}}
        mcp={view}
        mcpError={null}
        onMcpToggle={onToggle}
        onMcpEdit={onEdit}
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

  test("lists the servers, and one that cannot start says why and has no switch", () => {
    const toggled: [string, boolean][] = [];
    panel(disk, () => {}, (name, on) => toggled.push([name, on]));

    expect(screen.getByText("npx -y server-github")).toBeTruthy();
    expect(screen.getByText("won't start")).toBeTruthy();
    expect(screen.getByText("HTTP servers are not supported yet")).toBeTruthy();
    const switches = screen.getAllByRole("button", { pressed: true });
    expect(switches).toHaveLength(1);
    fireEvent.click(switches[0]);
    expect(toggled).toEqual([["github", false]]);
  });

  test("the button opens the editor, and says add when there is nothing yet", () => {
    let edits = 0;
    panel({ ...disk, servers: [] }, () => edits++);
    fireEvent.click(screen.getByText("Add MCP server"));
    expect(edits).toBe(1);
  });
});

describe("the MCP editor", () => {
  test("a pasted config is saved and the editor closes", async () => {
    const saved: string[] = [];
    let closed = false;
    render(
      <McpConfig
        view={disk}
        error={null}
        onSave={async (text) => (saved.push(text), true)}
        onClose={() => (closed = true)}
      />,
    );
    const box = screen.getByLabelText("MCP configuration") as HTMLTextAreaElement;
    expect(box.value).toBe(disk.text);

    fireEvent.change(box, { target: { value: '{"mcpServers":{"x":{"command":"y"}}}' } });
    await act(async () => {
      fireEvent.click(screen.getByText("Save"));
    });
    expect(saved).toEqual(['{"mcpServers":{"x":{"command":"y"}}}']);
    expect(closed).toBe(true);
  });

  test("a refused save keeps the editor open with what was typed", async () => {
    let closed = false;
    const { rerender } = render(
      <McpConfig view={disk} error={null} onSave={async () => false} onClose={() => (closed = true)} />,
    );
    const box = screen.getByLabelText("MCP configuration") as HTMLTextAreaElement;
    fireEvent.change(box, { target: { value: "{" } });
    await act(async () => {
      fireEvent.click(screen.getByText("Save"));
    });
    rerender(<McpConfig view={disk} error="not valid: EOF" onSave={async () => false} onClose={() => (closed = true)} />);

    expect(closed).toBe(false);
    expect(box.value).toBe("{");
    expect(screen.getByText("not valid: EOF")).toBeTruthy();
  });
});

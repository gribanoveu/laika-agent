import { afterAll, beforeEach, describe, expect, mock, test } from "bun:test";
import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import type { SkillsView } from "../lib/chat";

// The skills folder is edited outside the app, so the tab re-reads it; a
// switch is shown at once and then settled by what the backend saved.

let disk: SkillsView = { dir: "/home/.atlas-desktop/skills", skills: [] };
let calls: string[] = [];
let failToggle = false;

mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: { name: string; enabled: boolean }) => {
    calls.push(command);
    if (command === "skills_list") return Promise.resolve(structuredClone(disk));
    if (command === "skills_set_enabled") {
      if (failToggle) return Promise.reject("settings.json is not valid");
      disk.skills = disk.skills.map((s) => (s.name === args!.name ? { ...s, enabled: args!.enabled } : s));
      return Promise.resolve();
    }
    return Promise.resolve(null);
  },
  transformCallback: (callback: unknown) => callback,
}));

(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
afterAll(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

const { useSkills } = await import("../hooks/useSkills");
const { AsidePanel } = await import("../components/AsidePanel");
const settle = () => act(() => new Promise((resolve) => setTimeout(resolve, 0)));

const skill = (name: string, enabled = true) => ({ name, description: `${name} does a thing`, enabled, error: null });

beforeEach(() => {
  disk = { dir: "/home/.atlas-desktop/skills", skills: [skill("release"), skill("review")] };
  calls = [];
  failToggle = false;
});

describe("useSkills", () => {
  test("reads the folder each time the tab opens, and not while it is hidden", async () => {
    const { result, rerender } = renderHook(({ visible }) => useSkills(visible), {
      initialProps: { visible: false },
    });
    await settle();
    expect(calls).toEqual([]);

    rerender({ visible: true });
    await settle();
    expect(result.current.view?.skills.map((s) => s.name)).toEqual(["release", "review"]);

    disk.skills.push(skill("added"));
    rerender({ visible: false });
    rerender({ visible: true });
    await settle();
    expect(result.current.view?.skills).toHaveLength(3);
  });

  test("a switch is saved and the list re-read", async () => {
    const { result } = renderHook(() => useSkills(true));
    await settle();

    await act(() => result.current.setEnabled("release", false));

    expect(calls).toEqual(["skills_list", "skills_set_enabled", "skills_list"]);
    expect(result.current.view?.skills[0].enabled).toBe(false);
  });

  test("a switch that fails says why and shows what is really saved", async () => {
    failToggle = true;
    const { result } = renderHook(() => useSkills(true));
    await settle();

    await act(() => result.current.setEnabled("release", false));

    expect(result.current.error).toContain("settings.json is not valid");
    expect(result.current.view?.skills[0].enabled).toBe(true);
  });
});

describe("the skills tab", () => {
  const panel = (view: SkillsView | null, onSkillToggle = (_: string, __: boolean) => {}) =>
    render(
      <AsidePanel
        tab="skills"
        onTabChange={() => {}}
        collapsed={false}
        onToggleCollapse={() => {}}
        onNotify={() => {}}
        skills={view}
        skillsError={null}
        onSkillToggle={onSkillToggle}
      />,
    );

  test("lists the skills with their switches and counts the ones on", () => {
    const toggled: [string, boolean][] = [];
    panel({ dir: "/d", skills: [skill("release"), skill("review", false)] }, (name, on) => toggled.push([name, on]));

    expect(screen.getByText("1/2")).toBeTruthy();
    const switches = screen.getAllByRole("button", { pressed: true });
    expect(switches).toHaveLength(1);
    fireEvent.click(switches[0]);
    expect(toggled).toEqual([["release", false]]);
  });

  test("a broken skill shows its reason and has no switch", () => {
    panel({
      dir: "/d",
      skills: [{ name: "broken", description: "", enabled: false, error: "SKILL.md is missing YAML frontmatter" }],
    });

    expect(screen.getByText("invalid")).toBeTruthy();
    expect(screen.getByText("SKILL.md is missing YAML frontmatter")).toBeTruthy();
    expect(screen.queryAllByRole("button", { pressed: false })).toHaveLength(0);
  });

  test("an empty folder says where skills go", () => {
    panel({ dir: "/home/.atlas-desktop/skills", skills: [] });
    expect(screen.getByText(/SKILL\.md in \/home\/\.atlas-desktop\/skills/)).toBeTruthy();
  });
});

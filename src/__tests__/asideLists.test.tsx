import { afterAll, beforeEach, describe, expect, mock, test } from "bun:test";
import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import type { RuleListItem, SkillsView } from "../lib/chat";

// The skills folder and a repository's AGENTS.md are edited outside the app,
// so each tab re-reads them; a switch is shown at once and then settled by
// what the backend saved.

let disk: SkillsView = { dir: "/home/.atlas-desktop/skills", skills: [] };
let calls: string[] = [];
let failToggle = false;
let ruleFiles: RuleListItem[] = [];

mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: { name: string; enabled: boolean }) => {
    calls.push(command);
    if (command === "skills_list") return Promise.resolve(structuredClone(disk));
    if (command === "rules_list") return Promise.resolve(structuredClone(ruleFiles));
    if (command === "rules_set_enabled") {
      const { path, enabled } = args as unknown as { path: string; enabled: boolean };
      ruleFiles = ruleFiles.map((r) => (r.path === path ? { ...r, enabled } : r));
      return Promise.resolve();
    }
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
const { useRules } = await import("../hooks/useRules");
const { AsidePanel } = await import("../components/AsidePanel");
const settle = () => act(() => new Promise((resolve) => setTimeout(resolve, 0)));

const skill = (name: string, enabled = true) => ({ name, description: `${name} does a thing`, enabled, error: null });

beforeEach(() => {
  disk = { dir: "/home/.atlas-desktop/skills", skills: [skill("release"), skill("review")] };
  calls = [];
  failToggle = false;
  ruleFiles = [rule("AGENTS.md")];
});

function rule(name: string, extra: Partial<RuleListItem> = {}): RuleListItem {
  return { name, path: `/repo/${name}`, enabled: true, content: "Run the tests.\nUse bun.", truncated: false, error: null, ...extra };
}

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
        rules={[]}
        rulesError={null}
        onRuleToggle={() => {}}
        tab="skills"
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

describe("useRules", () => {
  test("reads the files when the tab opens and again when the folder changes", async () => {
    const { result, rerender } = renderHook(({ visible, folder }) => useRules(visible, folder), {
      initialProps: { visible: false, folder: "/repo" as string | null },
    });
    await settle();
    expect(calls).toEqual([]);

    rerender({ visible: true, folder: "/repo" });
    await settle();
    expect(result.current.rules.map((r) => r.name)).toEqual(["AGENTS.md"]);

    ruleFiles = [rule("CLAUDE.md")];
    rerender({ visible: true, folder: "/other" });
    await settle();
    expect(result.current.rules.map((r) => r.name)).toEqual(["CLAUDE.md"]);
  });

  test("a switch is saved by path and the list re-read", async () => {
    const { result } = renderHook(() => useRules(true, "/repo"));
    await settle();

    await act(() => result.current.setEnabled("/repo/AGENTS.md", false));

    expect(calls).toEqual(["rules_list", "rules_set_enabled", "rules_list"]);
    expect(result.current.rules[0].enabled).toBe(false);
  });
});

describe("the rules tab", () => {
  const panel = (rules: RuleListItem[], onRuleToggle = (_: string, __: boolean) => {}) =>
    render(
      <AsidePanel
        tab="rules"
        onNotify={() => {}}
        skills={null}
        skillsError={null}
        onSkillToggle={() => {}}
        rules={rules}
        rulesError={null}
        onRuleToggle={onRuleToggle}
      />,
    );

  test("a file shows its size, its text on expand, and a switch keyed by path", () => {
    const toggled: [string, boolean][] = [];
    panel([rule("AGENTS.md"), rule("CLAUDE.md", { enabled: false })], (path, on) => toggled.push([path, on]));

    expect(screen.getByText("1/2")).toBeTruthy();
    fireEvent.click(screen.getByText("AGENTS.md"));
    expect(screen.getByText(/Run the tests\./)).toBeTruthy();
    fireEvent.click(screen.getAllByRole("button", { pressed: true })[0]);
    expect(toggled).toEqual([["/repo/AGENTS.md", false]]);
  });

  test("a cut file says so", () => {
    panel([rule("AGENTS.md", { truncated: true })]);
    expect(screen.getByText("cut")).toBeTruthy();
    expect(screen.getByText("2 lines sent, the rest cut")).toBeTruthy();
  });

  test("a file that cannot be sent says why and has no switch", () => {
    panel([rule("AGENTS.md", { enabled: false, content: "", error: "links outside the open folder" })]);
    expect(screen.getByText("not sent")).toBeTruthy();
    expect(screen.getByText("links outside the open folder")).toBeTruthy();
    expect(screen.queryAllByRole("button", { pressed: false })).toHaveLength(0);
  });

  test("a folder without them says which files count", () => {
    panel([]);
    expect(screen.getByText(/No AGENTS\.md or CLAUDE\.md/)).toBeTruthy();
  });
});

import { describe, expect, test } from "bun:test";
import { render, screen, fireEvent } from "@testing-library/react";
import { Settings } from "../components/Settings";
import type { SkillsView } from "../lib/chat";

// The dialog's own rules: one section at a time, and the request log — which
// writes whole conversations to disk — stays off until someone turns it on.

const SOURCES: SkillsView["sources"] = [
  { id: "project", path: "/repo", enabled: true },
  { id: "app", path: "/Users/me/.laika/skills", enabled: true },
  { id: "agents", path: "/Users/me/.agents/skills", enabled: false },
  { id: "claude", path: "/Users/me/.claude/skills", enabled: true },
];

const dialog = (debugLogging = false) => {
  const logging: boolean[] = [];
  const sizes: string[] = [];
  const sources: [string, boolean][] = [];
  let logOpened = 0;
  render(
    <Settings
      skills={{
        view: { dir: "/Users/me/.laika/skills", skills: [], sources: SOURCES },
        error: null,
        onToggle: (id, on) => sources.push([id, on]),
      }}
      provider={{
        settings: { providers: [], activeProviderId: null, debugLogging },
        busy: false,
        error: null,
        onSave: () => {},
        onRemove: () => {},
        onSelect: () => {},
      }}
      debugLogging={debugLogging}
      onDebugLogging={(enabled) => logging.push(enabled)}
      theme="system"
      onTheme={() => {}}
      fontSize="large"
      onFontSize={(size) => sizes.push(size)}
      onOpenLog={() => logOpened++}
      policy={{ provider: null, debugLogging, mcpServers: [], hooks: [] }}
    />,
  );
  return { logging, sizes, sources, logOpened: () => logOpened };
};

describe("the settings dialog", () => {
  test("opens on the model provider, one section at a time", () => {
    dialog();
    expect(screen.getByText("Model provider")).toBeDefined();
    expect(screen.queryByRole("radiogroup", { name: "Theme" })).toBeNull();

    fireEvent.click(screen.getByText("Appearance"));
    expect(screen.getByRole("radiogroup", { name: "Theme" })).toBeDefined();
    expect(screen.queryByText("Model provider")).toBeNull();
  });

  test("Skills lists the folders skills come from, in the order a name is looked up, each with a switch", () => {
    const { sources } = dialog();
    fireEvent.click(screen.getByText("Skills"));
    const titles = ["The repository's", "Laika's", "Codex and other agents'", "Claude Code's"].map((t) => screen.getByText(t));
    for (let i = 1; i < titles.length; i++) {
      expect(titles[i - 1].compareDocumentPosition(titles[i]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    }
    expect(screen.getByText("/Users/me/.agents/skills")).toBeDefined();
    expect(screen.getByText("3/4")).toBeDefined();

    fireEvent.click(screen.getAllByRole("button", { pressed: false })[0]);
    fireEvent.click(screen.getAllByRole("button", { pressed: true })[0]);
    expect(sources).toEqual([["agents", true], ["project", false]]);
  });

  test("the font size is picked under Appearance", () => {
    const { sizes } = dialog();
    fireEvent.click(screen.getByText("Appearance"));

    expect(screen.getByRole("radio", { name: "large" }).getAttribute("aria-checked")).toBe("true");
    fireEvent.click(screen.getByRole("radio", { name: "small" }));
    expect(sizes).toEqual(["small"]);
  });

  test("the request log is off unless it was turned on", () => {
    const { logging } = dialog();
    fireEvent.click(screen.getByText("Privacy"));

    expect(screen.getByRole("radio", { name: "off" }).getAttribute("aria-checked")).toBe("true");
    fireEvent.click(screen.getByRole("radio", { name: "on" }));
    expect(logging).toEqual([true]);
  });

  test("privacy leads to the tool log", () => {
    const { logOpened } = dialog();
    fireEvent.click(screen.getByText("Privacy"));
    fireEvent.click(screen.getByText("Open the log"));
    expect(logOpened()).toBe(1);
  });
});

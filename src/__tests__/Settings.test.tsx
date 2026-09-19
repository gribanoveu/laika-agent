import { describe, expect, test } from "bun:test";
import { render, screen, fireEvent } from "@testing-library/react";
import { Settings } from "../components/Settings";

// The dialog's own rules: one section at a time, and the request log — which
// writes whole conversations to disk — stays off until someone turns it on.

const dialog = (debugLogging = false) => {
  const logging: boolean[] = [];
  let logOpened = 0;
  render(
    <Settings
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
      onOpenLog={() => logOpened++}
      policy={{ provider: null, debugLogging, mcpServers: [], hooks: [] }}
    />,
  );
  return { logging, logOpened: () => logOpened };
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

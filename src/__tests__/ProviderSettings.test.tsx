import { describe, expect, test } from "bun:test";
import { render, screen, fireEvent } from "@testing-library/react";
import { ProviderSettings } from "../components/ProviderSettings";
import type { LlmSettings, ProviderConfig } from "../lib/chat";

// The form's one real rule: an untouched key field must leave the stored key
// alone. Everything else here is about not editing a provider that is no
// longer there.

const settings = (over: Partial<LlmSettings> = {}): LlmSettings => ({
  providers: [
    { id: "local", baseUrl: "http://127.0.0.1:1234/v1", model: "qwen", hasApiKey: true },
    { id: "openai", baseUrl: "https://api.openai.com/v1", hasApiKey: false },
  ],
  activeProviderId: "local",
  debugLogging: false,
  ...over,
});

type Saved = { provider: ProviderConfig; apiKey: string | null };

const form = (over: Partial<LlmSettings> = {}) => {
  const saved: Saved[] = [];
  const removed: string[] = [];
  const logging: boolean[] = [];
  render(
    <ProviderSettings
      settings={settings(over)}
      busy={false}
      error={null}
      onSave={(provider, apiKey) => saved.push({ provider, apiKey })}
      onRemove={(id) => removed.push(id)}
      onSelect={() => {}}
      onDebugLogging={(enabled) => logging.push(enabled)}
    />,
  );
  return { saved, removed, logging };
};

/** The fields are labelled but not `for`-linked, the way the prototype draws them. */
const field = (label: string) => {
  const fields = Array.from(document.querySelectorAll(".modal-field"));
  const found = fields.find((el) => el.querySelector("label")?.textContent === label);
  return found?.querySelector("input") as HTMLInputElement;
};

describe("editing a provider", () => {
  test("opens on the active one, with its values", () => {
    form();

    expect(field("Name").value).toBe("local");
    expect(field("Base URL").value).toBe("http://127.0.0.1:1234/v1");
    expect(field("Model").value).toBe("qwen");
  });

  /// The whole reason the key field starts empty rather than pre-filled: there
  /// is nothing to pre-fill it with, and sending its empty value on every save
  /// would delete the key the user just stored.
  test("an untouched key field leaves the stored key alone", () => {
    const { saved } = form();

    fireEvent.change(field("Model"), { target: { value: "qwen3" } });
    fireEvent.click(screen.getByText("Save"));

    expect(saved[0]?.apiKey).toBeNull();
    expect(saved[0]?.provider.model).toBe("qwen3");
  });

  test("a typed key is sent to be sealed", () => {
    const { saved } = form();

    fireEvent.change(field("API key"), { target: { value: "sk-new" } });
    fireEvent.click(screen.getByText("Save"));

    expect(saved[0]?.apiKey).toBe("sk-new");
  });

  test("the key field says whether one is already stored", () => {
    form();
    expect(field("API key").placeholder).toContain("stored");
  });

  test("an empty model means auto, not an empty string", () => {
    const { saved } = form();

    fireEvent.change(field("Model"), { target: { value: "  " } });
    fireEvent.click(screen.getByText("Save"));

    expect(saved[0]?.provider.model).toBeNull();
  });

  /// Found by a mutation: without clearing the field on the way out, a key
  /// typed for one provider is sealed under the next one's id — the wrong
  /// endpoint is handed a credential, and the right one still has none.
  test("a key typed for one provider does not follow you to another", () => {
    const { saved } = form();

    fireEvent.change(field("API key"), { target: { value: "sk-for-local" } });
    fireEvent.click(screen.getByText("openai"));
    fireEvent.click(screen.getByText("Save"));

    expect(saved[0]?.provider.id).toBe("openai");
    expect(saved[0]?.apiKey).toBeNull();
  });

  test("adding one starts from a blank form", () => {
    const { saved } = form();

    fireEvent.click(screen.getByTitle("Add another provider"));
    expect(field("Name").value).toBe("");

    fireEvent.change(field("Name"), { target: { value: "openrouter" } });
    fireEvent.change(field("Base URL"), { target: { value: "https://openrouter.ai/api/v1" } });
    fireEvent.click(screen.getByText("Save"));

    expect(saved[0]?.provider.id).toBe("openrouter");
  });

  test("removing names the provider being edited", () => {
    const { removed } = form();
    fireEvent.click(screen.getByText("Remove"));
    expect(removed).toEqual(["local"]);
  });

  /// It writes whole conversations, including the contents of every file the
  /// agent read, so it is off until someone says otherwise.
  test("the request log is off unless it was turned on", () => {
    const { logging } = form();

    expect(screen.getByText("off").getAttribute("aria-checked")).toBe("true");
    fireEvent.click(screen.getByText("on"));
    expect(logging).toEqual([true]);
  });
});

describe("with nothing configured", () => {
  test("there is no provider picker, only a blank form", () => {
    render(
      <ProviderSettings
        settings={{ providers: [], activeProviderId: null, debugLogging: false }}
        busy={false}
        error={null}
        onSave={() => {}}
        onRemove={() => {}}
        onSelect={() => {}}
        onDebugLogging={() => {}}
      />,
    );

    expect(screen.queryByText("Remove")).toBeNull();
    expect(field("Name").value).toBe("");
  });
});

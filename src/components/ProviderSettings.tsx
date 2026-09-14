import { useEffect, useState } from "react";
import type { LlmSettings, ProviderConfig } from "../lib/chat";
import "./ProviderSettings.css";

// The provider form. A segmented control rather than the app's dropdown,
// because a popup menu is clipped by the modal's own overflow — the same
// reason the theme picker is segmented.

const BLANK = { id: "", baseUrl: "", model: "" };

type Props = {
  settings: LlmSettings | null;
  busy: boolean;
  error: string | null;
  onSave: (provider: ProviderConfig, apiKey: string | null) => void;
  onRemove: (id: string) => void;
  onSelect: (id: string) => void;
  onDebugLogging: (enabled: boolean) => void;
};

export function ProviderSettings({
  settings,
  busy,
  error,
  onSave,
  onRemove,
  onSelect,
  onDebugLogging,
}: Props) {
  const providers = settings?.providers ?? [];
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState(BLANK);
  // `null` means the field was never touched, which is not the same as an
  // empty one: an empty one deletes the stored key.
  const [apiKey, setApiKey] = useState<string | null>(null);

  // Follow the backend rather than remember a stale copy of it: a save that
  // renamed or removed a provider must not leave the form editing a ghost.
  useEffect(() => {
    if (!settings) return;
    const active = settings.activeProviderId ?? providers[0]?.id ?? null;
    setEditing(active);
    const chosen = providers.find((p) => p.id === active);
    setDraft(
      chosen ? { id: chosen.id, baseUrl: chosen.baseUrl, model: chosen.model ?? "" } : BLANK,
    );
    setApiKey(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings]);

  const chosen = providers.find((p) => p.id === editing);
  const pick = (id: string | null) => {
    setEditing(id);
    setApiKey(null);
    const next = providers.find((p) => p.id === id);
    setDraft(next ? { id: next.id, baseUrl: next.baseUrl, model: next.model ?? "" } : BLANK);
    if (id) onSelect(id);
  };

  const save = () =>
    onSave(
      {
        ...chosen,
        id: draft.id.trim(),
        baseUrl: draft.baseUrl.trim(),
        model: draft.model.trim() || null,
      },
      apiKey,
    );

  return (
    <div className="provider-settings">
      {providers.length > 0 && (
        <div className="modal-field">
          <label>Provider</label>
          <div className="segmented" role="radiogroup" aria-label="Provider">
            {providers.map((provider) => (
              <button
                key={provider.id}
                type="button"
                role="radio"
                aria-checked={provider.id === editing}
                className={`segment${provider.id === editing ? " active" : ""}`}
                onClick={() => pick(provider.id)}
              >
                {provider.id}
              </button>
            ))}
            <button
              type="button"
              className={`segment${editing === null ? " active" : ""}`}
              onClick={() => pick(null)}
              title="Add another provider"
            >
              +
            </button>
          </div>
        </div>
      )}

      <div className="modal-field">
        <label>Name</label>
        <input
          type="text"
          value={draft.id}
          placeholder="openai"
          onChange={(e) => setDraft({ ...draft, id: e.target.value })}
        />
      </div>
      <div className="modal-field">
        <label>Base URL</label>
        <input
          type="text"
          value={draft.baseUrl}
          placeholder="https://api.openai.com/v1"
          onChange={(e) => setDraft({ ...draft, baseUrl: e.target.value })}
        />
      </div>
      <div className="modal-field">
        <label>Model</label>
        <input
          type="text"
          value={draft.model}
          placeholder="auto — the first one the provider lists"
          onChange={(e) => setDraft({ ...draft, model: e.target.value })}
        />
      </div>
      <div className="modal-field">
        <label>API key</label>
        <input
          type="password"
          value={apiKey ?? ""}
          placeholder={chosen?.hasApiKey ? "stored — type to replace" : "not configured"}
          onChange={(e) => setApiKey(e.target.value)}
        />
      </div>

      <div className="modal-field">
        <label>Log every request to disk</label>
        <div className="segmented" role="radiogroup" aria-label="Debug logging">
          {[
            ["off", false],
            ["on", true],
          ].map(([label, value]) => (
            <button
              key={String(label)}
              type="button"
              role="radio"
              aria-checked={settings?.debugLogging === value}
              className={`segment${settings?.debugLogging === value ? " active" : ""}`}
              onClick={() => onDebugLogging(Boolean(value))}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      <div className="provider-actions">
        <button className="btn btn-primary" type="button" disabled={busy} onClick={save}>
          Save
        </button>
        {chosen && (
          <button
            className="btn btn-ghost"
            type="button"
            disabled={busy}
            onClick={() => onRemove(chosen.id)}
          >
            Remove
          </button>
        )}
      </div>

      {error && <p className="modal-note provider-error">{error}</p>}
      <p className="modal-note">
        The key is sealed on disk and never leaves the backend — nothing here can read it back.
        Turning the request log on writes whole conversations, including the contents of every
        file the agent read, to a file in your home directory.
      </p>
    </div>
  );
}

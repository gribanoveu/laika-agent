import { useState, type ComponentProps } from "react";
import { Bot, Palette, Shield } from "lucide-react";
import { ProviderSettings } from "./ProviderSettings";
import { DataPolicy } from "./DataPolicy";
import { THEMES, type ThemePreference } from "../hooks/useTheme";
import "./Settings.css";

// The settings dialog: one subject per section, picked on the left, so the
// provider form is not scrolled past to reach the theme.

const SECTIONS = [
  { id: "models", label: "Models", icon: Bot },
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "privacy", label: "Privacy", icon: Shield },
] as const;

type Section = (typeof SECTIONS)[number]["id"];

type Props = {
  provider: ComponentProps<typeof ProviderSettings>;
  debugLogging: boolean;
  onDebugLogging: (enabled: boolean) => void;
  theme: ThemePreference;
  onTheme: (theme: ThemePreference) => void;
  onOpenLog: () => void;
  policy: ComponentProps<typeof DataPolicy>;
};

export function Settings({ provider, debugLogging, onDebugLogging, theme, onTheme, onOpenLog, policy }: Props) {
  const [section, setSection] = useState<Section>("models");

  return (
    <div className="settings">
      <nav className="settings-nav" aria-label="Settings sections">
        {SECTIONS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            className={`settings-nav-item${section === id ? " active" : ""}`}
            aria-current={section === id ? "page" : undefined}
            onClick={() => setSection(id)}
          >
            <Icon size={14} />
            {label}
          </button>
        ))}
      </nav>

      <div className="settings-pane">
        {section === "models" && (
          <>
            <h3 className="settings-title">Model provider</h3>
            <ProviderSettings {...provider} />
          </>
        )}

        {section === "appearance" && (
          <>
            <h3 className="settings-title">Appearance</h3>
            <div className="modal-field">
              <label>Theme</label>
              <div className="segmented" role="radiogroup" aria-label="Theme">
                {THEMES.map((name) => (
                  <button
                    key={name}
                    type="button"
                    role="radio"
                    aria-checked={theme === name}
                    className={`segment${theme === name ? " active" : ""}`}
                    onClick={() => onTheme(name)}
                  >
                    {name}
                  </button>
                ))}
              </div>
            </div>
          </>
        )}

        {section === "privacy" && (
          <>
            <h3 className="settings-title">Privacy</h3>
            <div className="modal-field">
              <label>Request log</label>
              <div className="segmented" role="radiogroup" aria-label="Request log">
                {[
                  ["off", false],
                  ["on", true],
                ].map(([label, value]) => (
                  <button
                    key={String(label)}
                    type="button"
                    role="radio"
                    aria-checked={debugLogging === value}
                    className={`segment${debugLogging === value ? " active" : ""}`}
                    onClick={() => onDebugLogging(Boolean(value))}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>
            <p className="modal-note settings-hint">
              Writes whole conversations, including every file the agent read, to a file in your home directory.
            </p>
            <div className="modal-field">
              <label>Tool calls</label>
              <div>
                <button className="btn btn-ghost" type="button" onClick={onOpenLog}>
                  Open the log
                </button>
              </div>
            </div>
            <DataPolicy {...policy} />
          </>
        )}
      </div>
    </div>
  );
}

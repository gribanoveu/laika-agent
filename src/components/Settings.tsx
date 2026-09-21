import { useState, type ComponentProps } from "react";
import { Bot, Palette, Shield, Sparkles } from "lucide-react";
import { ProviderSettings } from "./ProviderSettings";
import { DataPolicy } from "./DataPolicy";
import { ItemList } from "./ItemList";
import type { SkillSourceItem, SkillsView } from "../lib/chat";
import type { PanelItem } from "../types";
import { THEMES, type ThemePreference } from "../hooks/useTheme";
import { FONT_SIZES, type FontSize } from "../hooks/useChatFontSize";
import "./Settings.css";

// The settings dialog: one subject per section, picked on the left, so the
// provider form is not scrolled past to reach the theme.

const SECTIONS = [
  { id: "models", label: "Models", icon: Bot },
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "skills", label: "Skills", icon: Sparkles },
  { id: "privacy", label: "Privacy", icon: Shield },
] as const;

type Section = (typeof SECTIONS)[number]["id"];

/** What each skills folder is called here, and what reading it means. */
const SOURCES: Record<SkillSourceItem["id"], { badge: string; title: string; desc: string }> = {
  project: {
    badge: "PR",
    title: "The repository's",
    desc: ".claude/skills and .agents/skills, from the open folder up to the git root",
  },
  app: { badge: "LA", title: "Laika's", desc: "This app's own skills folder" },
  agents: { badge: "AG", title: "Codex and other agents'", desc: "Where skill installers put them" },
  claude: { badge: "CL", title: "Claude Code's", desc: "Your personal Claude Code skills" },
};

function sourceItems(sources: SkillSourceItem[]): PanelItem[] {
  return sources.map((source) => ({
    id: source.id,
    kind: "skill",
    ...SOURCES[source.id],
    enabled: source.enabled,
    meta: source.path || "No folder open",
  }));
}

type Props = {
  provider: ComponentProps<typeof ProviderSettings>;
  /** The skills folders: which of them are read at all. */
  skills: { view: SkillsView | null; error: string | null; onToggle: (id: SkillSourceItem["id"], enabled: boolean) => void };
  debugLogging: boolean;
  onDebugLogging: (enabled: boolean) => void;
  theme: ThemePreference;
  onTheme: (theme: ThemePreference) => void;
  fontSize: FontSize;
  onFontSize: (size: FontSize) => void;
  onOpenLog: () => void;
  policy: ComponentProps<typeof DataPolicy>;
};

export function Settings({
  provider,
  skills,
  debugLogging,
  onDebugLogging,
  theme,
  onTheme,
  fontSize,
  onFontSize,
  onOpenLog,
  policy,
}: Props) {
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
            <div className="modal-field">
              <label>Chat text size</label>
              <div className="segmented" role="radiogroup" aria-label="Chat text size">
                {(Object.keys(FONT_SIZES) as FontSize[]).map((name) => (
                  <button
                    key={name}
                    type="button"
                    role="radio"
                    aria-checked={fontSize === name}
                    className={`segment${fontSize === name ? " active" : ""}`}
                    onClick={() => onFontSize(name)}
                  >
                    {name}
                  </button>
                ))}
              </div>
            </div>
          </>
        )}

        {section === "skills" && (
          <>
            <h3 className="settings-title">Skills</h3>
            <ItemList
              label="Where skills come from"
              count={`${(skills.view?.sources ?? []).filter((s) => s.enabled).length}/${skills.view?.sources.length ?? 0}`}
              items={sourceItems(skills.view?.sources ?? [])}
              emptyLabel="Reading the folders…"
              onToggle={(id, enabled) => skills.onToggle(id as SkillSourceItem["id"], enabled)}
            />
            {skills.error && <p className="modal-note settings-error">{skills.error}</p>}
            <p className="modal-note settings-list-note">
              A folder switched off is not read at all, in any repository. When two folders have a skill of the same
              name, the one higher in this list is used. Single skills are switched off in the Skills panel.
            </p>
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

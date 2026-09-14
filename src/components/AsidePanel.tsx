import {
  BookText,
  FolderClosed,
  PanelRight,
  Plug,
  Sparkles,
  SquareTerminal,
  Table2,
} from "lucide-react";
import { ChangesPanel } from "./ChangesPanel";
import { ItemList } from "./ItemList";
import type { AsideTab, PanelItem } from "../types";
import "./AsidePanel.css";

const TABS: { id: AsideTab; label: string; icon: typeof Table2 }[] = [
  { id: "changes", label: "Changes", icon: Table2 },
  { id: "mcp", label: "MCP", icon: Plug },
  { id: "skills", label: "Skills", icon: Sparkles },
  { id: "rules", label: "Rules", icon: BookText },
  { id: "files", label: "Files", icon: FolderClosed },
  { id: "terminal", label: "Terminal", icon: SquareTerminal },
];

// Each list is filled by its own command wrapper once that command exists.
const MCP_SERVERS: PanelItem[] = [];
const SKILLS: PanelItem[] = [];
const RULES: PanelItem[] = [];
const WORKSPACE_FILES: string[] = [];

type Props = {
  tab: AsideTab;
  onTabChange: (tab: AsideTab) => void;
  collapsed: boolean;
  onToggleCollapse: () => void;
  onNotify: (msg: string) => void;
};

export function AsidePanel({
  tab,
  onTabChange,
  collapsed,
  onToggleCollapse,
  onNotify,
}: Props) {
  return (
    <aside className="aside">
      <div className="aside-head">
        <button
          className="aside-toggle"
          type="button"
          title="Collapse panel"
          onClick={onToggleCollapse}
        >
          <PanelRight size={15} />
        </button>
        <div className="tabs" role="tablist">
          {TABS.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              role="tab"
              title={label}
              aria-selected={tab === id}
              className={`tab${tab === id ? " active" : ""}`}
              onClick={() => onTabChange(id)}
            >
              <Icon size={15} />
            </button>
          ))}
        </div>
      </div>

      <div className="aside-rail" aria-hidden={!collapsed}>
        {TABS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            title={label}
            className={`rail-btn${tab === id ? " active" : ""}`}
            onClick={() => {
              onTabChange(id);
              if (collapsed) onToggleCollapse();
            }}
          >
            <Icon size={16} />
          </button>
        ))}
      </div>

      <div className="aside-body">
        <div className="tabpanel" role="tabpanel">
          {tab === "changes" && <ChangesPanel onNotify={onNotify} />}
          {tab === "mcp" && (
            <ItemList
              label="Connected servers"
              count={String(MCP_SERVERS.length)}
              items={MCP_SERVERS}
              emptyLabel="No MCP servers connected."
              addLabel="Add MCP server"
              onAdd={() => onNotify("MCP setup is not wired yet")}
            />
          )}
          {tab === "skills" && (
            <ItemList
              label="Active skills"
              count={String(SKILLS.length)}
              items={SKILLS}
              emptyLabel="No skills installed."
              addLabel="Install skill"
              onAdd={() => onNotify("Skill install is not wired yet")}
            />
          )}
          {tab === "rules" && (
            <ItemList
              label="Project rules"
              count={String(RULES.length)}
              items={RULES}
              emptyLabel="No rule files found."
            />
          )}
          {tab === "files" && (
            <div className="panel-section">
              <div className="section-label">
                <span>Workspace</span>
                <span className="count">{WORKSPACE_FILES.length}</span>
              </div>
              {WORKSPACE_FILES.length === 0 ? (
                <div className="empty">No workspace indexed.</div>
              ) : (
                WORKSPACE_FILES.map((path) => (
                  <div className="file" key={path}>
                    <span>{path}</span>
                  </div>
                ))
              )}
            </div>
          )}
          {tab === "terminal" && (
            <div className="panel-section">
              <div className="section-label">
                <span>Last command</span>
              </div>
              <div className="empty">Nothing has run yet.</div>
            </div>
          )}
        </div>
      </div>
    </aside>
  );
}

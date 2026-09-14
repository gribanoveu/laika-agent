import {
  BookText,
  FolderClosed,
  PanelRight,
  Plug,
  Sparkles,
  SquareTerminal,
  Table2,
} from "lucide-react";
import { ContextPanel } from "./ContextPanel";
import { ItemList } from "./ItemList";
import { LAST_COMMAND, MCP_SERVERS, RULES, SKILLS, WORKSPACE_FILES } from "../mock/data";
import type { AsideTab } from "../types";
import "./AsidePanel.css";

const TABS: { id: AsideTab; label: string; icon: typeof Table2; badge?: "ok" | "warn" }[] = [
  { id: "context", label: "Context", icon: Table2 },
  { id: "mcp", label: "MCP", icon: Plug, badge: "warn" },
  { id: "skills", label: "Skills", icon: Sparkles },
  { id: "rules", label: "Rules", icon: BookText },
  { id: "files", label: "Files", icon: FolderClosed },
  { id: "terminal", label: "Terminal", icon: SquareTerminal },
];

type Props = {
  tab: AsideTab;
  onTabChange: (tab: AsideTab) => void;
  collapsed: boolean;
  onToggleCollapse: () => void;
  onNotify: (msg: string) => void;
};

export function AsidePanel({ tab, onTabChange, collapsed, onToggleCollapse, onNotify }: Props) {
  return (
    <aside className="aside">
      <div className="aside-head">
        <button
          className="aside-toggle"
          type="button"
          title="Свернуть панель"
          onClick={onToggleCollapse}
        >
          <PanelRight size={15} />
        </button>
        <div className="tabs" role="tablist">
          {TABS.map(({ id, label }) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={tab === id}
              className={`tab${tab === id ? " active" : ""}`}
              onClick={() => onTabChange(id)}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      <div className="aside-rail" aria-hidden={!collapsed}>
        {TABS.map(({ id, label, icon: Icon, badge }) => (
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
            {badge && <span className={`badge ${badge}`} />}
          </button>
        ))}
      </div>

      <div className="aside-body">
        <div className="tabpanel" role="tabpanel">
          {tab === "context" && <ContextPanel onNotify={onNotify} />}
          {tab === "mcp" && (
            <ItemList
              label="Connected servers"
              count={String(MCP_SERVERS.length)}
              items={MCP_SERVERS}
              addLabel="Add MCP server"
              onAdd={() => onNotify("MCP setup is not wired yet")}
            />
          )}
          {tab === "skills" && (
            <ItemList
              label="Active skills"
              count={`${SKILLS.filter((s) => s.enabled).length} / 12`}
              items={SKILLS}
              addLabel="Install skill"
              onAdd={() => onNotify("Skill install is not wired yet")}
            />
          )}
          {tab === "rules" && (
            <ItemList label="Project rules" count={String(RULES.length)} items={RULES} />
          )}
          {tab === "files" && (
            <div className="panel-section">
              <div className="section-label">
                <span>Workspace</span>
                <span className="count">847 files</span>
              </div>
              {WORKSPACE_FILES.map((path) => (
                <div className="file" key={path}>
                  <span>{path}</span>
                </div>
              ))}
            </div>
          )}
          {tab === "terminal" && (
            <div className="panel-section">
              <div className="section-label">
                <span>Last command</span>
              </div>
              <div className="terminal-out">
                <span className="cmd">$ {LAST_COMMAND.cmd}</span>
                {LAST_COMMAND.lines.map((line) => (
                  <div key={line}>{line}</div>
                ))}
                <div className="ok">{LAST_COMMAND.result}</div>
              </div>
            </div>
          )}
        </div>
      </div>
    </aside>
  );
}

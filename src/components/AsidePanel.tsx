import {
  BookText,
  ClipboardList,
  FolderClosed,
  PanelRight,
  Plug,
  Sparkles,
  SquareTerminal,
  Table2,
} from "lucide-react";
import { ChangesPanel } from "./ChangesPanel";
import { ItemList } from "./ItemList";
import { PlanPanel } from "./PlanPanel";
import type { McpView, RuleListItem, SkillsView, Task } from "../lib/chat";
import type { AsideTab, PanelItem } from "../types";
import "./AsidePanel.css";

const TABS: { id: AsideTab; label: string; icon: typeof Table2 }[] = [
  { id: "changes", label: "Changes", icon: Table2 },
  { id: "plan", label: "Plan", icon: ClipboardList },
  { id: "mcp", label: "MCP", icon: Plug },
  { id: "skills", label: "Skills", icon: Sparkles },
  { id: "rules", label: "Rules", icon: BookText },
  { id: "files", label: "Files", icon: FolderClosed },
  { id: "terminal", label: "Terminal", icon: SquareTerminal },
];

// Each list is filled by its own command wrapper once that command exists.
const WORKSPACE_FILES: string[] = [];

type Props = {
  tab: AsideTab;
  onTabChange: (tab: AsideTab) => void;
  collapsed: boolean;
  onToggleCollapse: () => void;
  onNotify: (msg: string) => void;
  mcp: McpView | null;
  mcpError: string | null;
  onMcpToggle: (name: string, enabled: boolean) => void;
  /** Opens the configuration editor. */
  onMcpEdit: () => void;
  skills: SkillsView | null;
  skillsError: string | null;
  onSkillToggle: (name: string, enabled: boolean) => void;
  rules: RuleListItem[];
  rulesError: string | null;
  onRuleToggle: (path: string, enabled: boolean) => void;
  plan: string | null;
  checklist: Task[];
  onPlanEdit: (plan: string) => void;
  onImplement?: () => void;
  planLocked: boolean;
};

/** Shown whole on expand: what the model is told is worth reading. The switch is keyed by path. */
function ruleItems(rules: RuleListItem[]): PanelItem[] {
  return rules.map((rule) =>
    rule.error
      ? {
          id: rule.path,
          badge: "!",
          kind: "rule",
          title: rule.name,
          status: { label: "not sent", tone: "warn" },
          desc: rule.error,
          source: rule.path,
        }
      : {
          id: rule.path,
          badge: rule.name.slice(0, 2).toUpperCase(),
          kind: "rule",
          title: rule.name,
          ...(rule.truncated ? { status: { label: "cut", tone: "warn" as const } } : {}),
          desc: `${rule.content.split("\n").length} lines${rule.truncated ? " sent, the rest cut" : ""}`,
          enabled: rule.enabled,
          note: rule.content,
          source: rule.path,
        },
  );
}

/** A server that cannot start says why and has no switch, like a broken skill. */
function mcpItems(view: McpView | null): PanelItem[] {
  return (view?.servers ?? []).map((server) =>
    server.error
      ? {
          id: server.name,
          badge: "!",
          kind: "mcp",
          title: server.name,
          status: { label: "won't start", tone: "warn" },
          desc: server.error,
          source: server.command || undefined,
        }
      : {
          id: server.name,
          badge: server.name.slice(0, 2).toUpperCase(),
          kind: "mcp",
          title: server.name,
          desc: server.command,
          enabled: server.enabled,
        },
  );
}

/** A broken skill is shown by its folder with the reason, and has no switch: it never reaches the model. */
function skillItems(view: SkillsView | null): PanelItem[] {
  return (view?.skills ?? []).map((skill) =>
    skill.error
      ? {
          id: skill.name,
          badge: "!",
          kind: "skill",
          title: skill.name,
          status: { label: "invalid", tone: "warn" },
          desc: skill.error,
        }
      : {
          id: skill.name,
          badge: skill.name.slice(0, 2).toUpperCase(),
          kind: "skill",
          title: skill.name,
          desc: skill.description,
          enabled: skill.enabled,
          note: skill.description,
        },
  );
}

export function AsidePanel({
  tab,
  onTabChange,
  collapsed,
  onToggleCollapse,
  onNotify,
  mcp,
  mcpError,
  onMcpToggle,
  onMcpEdit,
  skills,
  skillsError,
  onSkillToggle,
  rules,
  rulesError,
  onRuleToggle,
  plan,
  checklist,
  onPlanEdit,
  onImplement,
  planLocked,
}: Props) {
  const ruleList = ruleItems(rules);
  const mcpList = mcpItems(mcp);
  const skillList = skillItems(skills);
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
          {tab === "plan" && (
            <PlanPanel
              plan={plan}
              checklist={checklist}
              onEdit={onPlanEdit}
              onImplement={onImplement}
              locked={planLocked}
            />
          )}
          {tab === "mcp" && (
            <>
              <ItemList
                label="Servers"
                count={`${mcpList.filter((s) => s.enabled).length}/${mcpList.length}`}
                items={mcpList}
                emptyLabel="No MCP servers configured."
                addLabel={mcpList.length ? "Edit servers" : "Add MCP server"}
                onAdd={onMcpEdit}
                onToggle={onMcpToggle}
              />
              {mcpError && <div className="empty">{mcpError}</div>}
            </>
          )}
          {tab === "skills" && (
            <>
              <ItemList
                label="Skills"
                count={`${skillList.filter((s) => s.enabled).length}/${skillList.length}`}
                items={skillList}
                emptyLabel={
                  skills?.dir
                    ? `No skills yet. A skill is a folder with a SKILL.md in ${skills.dir}.`
                    : "No skills yet."
                }
                onToggle={onSkillToggle}
              />
              {skillsError && <div className="empty">{skillsError}</div>}
            </>
          )}
          {tab === "rules" && (
            <>
              <ItemList
                label="Project instructions"
                count={`${ruleList.filter((r) => r.enabled).length}/${ruleList.length}`}
                items={ruleList}
                emptyLabel="No AGENTS.md or CLAUDE.md at the root of the open folder."
                onToggle={onRuleToggle}
              />
              {rulesError && <div className="empty">{rulesError}</div>}
            </>
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

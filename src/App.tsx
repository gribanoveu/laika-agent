import { useState } from "react";
import { Sidebar } from "./components/Sidebar";
import { ChatPanel } from "./components/ChatPanel";
import { Composer } from "./components/Composer";
import { AsidePanel } from "./components/AsidePanel";
import { Modal } from "./components/Modal";
import { ProviderSettings } from "./components/ProviderSettings";
import { ToolLog } from "./components/ToolLog";
import { ConfigFileEditor } from "./components/ConfigFileEditor";
import { PanelResizeHandle } from "./components/PanelResizeHandle";
import { Toast } from "./components/Toast";
import { WindowControls } from "./components/WindowControls";
import { useAgentTurn } from "./hooks/useAgentTurn";
import { DataPolicy } from "./components/DataPolicy";
import { useChatHistory } from "./hooks/useChatHistory";
import { useNarrowCollapse } from "./hooks/useNarrowCollapse";
import { useLlmSettings } from "./hooks/useLlmSettings";
import { useWorkspace } from "./hooks/useWorkspace";
import { useIndexStatus } from "./hooks/useIndexStatus";
import { useSkills } from "./hooks/useSkills";
import { useMcp } from "./hooks/useMcp";
import { useHooks } from "./hooks/useHooks";
import { useProcesses } from "./hooks/useProcesses";
import { useRules } from "./hooks/useRules";
import { useToolLog } from "./hooks/useToolLog";
import { usePanelSizes } from "./hooks/usePanelSizes";
import { useTheme, THEMES } from "./hooks/useTheme";
import { useToast } from "./hooks/useToast";
import { startWindowDrag, toggleMaximizeWindow } from "./lib/window";
import { useBackendSetting } from "./hooks/useBackendSetting";
import { setConversationMode, setUnattended, type ConversationMode } from "./lib/chat";
import type { AsideTab } from "./types";
import "./App.css";

// Titlebar drag: single press drags the window, double press zooms it — the macOS
// titlebar contract, driven explicitly so clicks on the controls stay clicks.
const dragOrMaximize = (e: React.MouseEvent) => {
  if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
  // Without this the webview keeps extending a text selection while the window
  // moves under the cursor, which flickers through whatever it passes over.
  e.preventDefault();
  if (e.detail === 2) toggleMaximizeWindow();
  else startWindowDrag();
};

/** What "Implement in Agent mode" says on the user's behalf. Shown in the transcript like anything they type. */
const IMPLEMENT_PLAN = "Implement the plan above. Work through the checklist in order.";

export default function App() {
  const [collapsed, setCollapsed] = useState(false);
  const [asideCollapsed, setAsideCollapsed] = useState(false);
  const [tab, setTab] = useState<AsideTab>("changes");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [logOpen, setLogOpen] = useState(false);
  const [mcpEditing, setMcpEditing] = useState(false);
  const [hooksEditing, setHooksEditing] = useState(false);
  // What the agent may do this turn, and whether anyone is asked before it
  // does it. Two chips, two questions — and both are enforced on the backend,
  // so these hold only what the chips read back.
  const conversation = useBackendSetting(setConversationMode, "agent" as ConversationMode);
  const unattended = useBackendSetting<boolean>(setUnattended, false);
  const toast = useToast();
  const workspace = useWorkspace();
  const index = useIndexStatus(workspace.path);
  const skills = useSkills(tab === "skills" && !asideCollapsed);
  const rules = useRules(tab === "rules" && !asideCollapsed, workspace.path);
  const toolLog = useToolLog(logOpen);
  const history = useChatHistory(workspace.path);
  // The list is redrawn from disk after every save rather than guessed at
  // here: what belongs in it, and in what order, is the store's rule.
  const agent = useAgentTurn({ onSaved: history.refresh });
  // Servers start with an Agent turn and may stop during one.
  // Settings shows both in "Where your data goes".
  const mcp = useMcp((tab === "mcp" && !asideCollapsed) || mcpEditing || settingsOpen, agent.turn.status);
  const hooks = useHooks((tab === "hooks" && !asideCollapsed) || hooksEditing || settingsOpen);
  const processes = useProcesses(tab === "terminal" && !asideCollapsed);
  const llm = useLlmSettings();
  const theme = useTheme();
  const panels = usePanelSizes({
    sidebar: {
      collapsed,
      collapse: () => setCollapsed(true),
      expand: () => setCollapsed(false),
    },
    aside: {
      collapsed: asideCollapsed,
      collapse: () => setAsideCollapsed(true),
      expand: () => setAsideCollapsed(false),
    },
  });

  // Narrow window: both panels fall back to their rails instead of one squeezing
  // the chat and the other disappearing.
  useNarrowCollapse("(max-width: 760px)", setCollapsed);
  useNarrowCollapse("(max-width: 900px)", setAsideCollapsed);

  const openTab = (next: AsideTab) => {
    setTab(next);
    setAsideCollapsed(false);
  };

  // The conversation just left is already on disk and stays in the sidebar;
  // this only stops pointing at it.
  const newChat = () => agent.reset();

  // The window the meter is drawn against, and the provider a turn talks to.
  const compactNow = async () => {
    if (!(await agent.compact(true))) {
      toast.show(agent.error ?? "Nothing worth folding away yet");
    }
  };

  const pickConversation = async (mode: ConversationMode) => {
    const failed = await conversation.pick(mode);
    if (failed) toast.show(failed);
  };

  // Said out loud in one direction only. Turning confirmations back on needs
  // no warning; turning them off means the next write happens without anyone
  // seeing it, and the chip alone is a small thing to have noticed.
  const pickUnattended = async (next: boolean) => {
    const failed = await unattended.pick(next);
    if (failed) toast.show(failed);
    else if (next) toast.show("Auto — the agent will change files without asking");
  };

  const chooseFolder = async () => {
    const opened = await workspace.pick();
    if (!opened && workspace.error) toast.show(workspace.error);
    return opened;
  };

  // Asking before the first message rather than refusing it — and then sending
  // it: the composer has already cleared the box, so anything not sent here is
  // typed twice.
  const send = async (text: string) => {
    if (!workspace.path && !(await chooseFolder())) return;
    agent.send(text);
  };

  // The mode first, and only then the message: the backend reads the mode
  // when the turn starts, and a turn sent a moment early would still be a
  // plan that cannot write.
  const implement = async () => {
    const failed = await conversation.pick("agent");
    if (failed) return toast.show(failed);
    agent.send(IMPLEMENT_PLAN);
  };

  return (
    <div
      className={`window${collapsed ? " collapsed" : ""}${asideCollapsed ? " aside-collapsed" : ""}`}
      style={
        {
          "--sidebar-width": `${panels.widths.sidebar}px`,
          "--aside-width": `${panels.widths.aside}px`,
        } as React.CSSProperties
      }
    >
      <div className="titlebar" onMouseDown={dragOrMaximize}>
        <WindowControls />
        <span className="titlebar-title">
          atlas-cli{workspace.path && <span> · {workspace.path.split("/").pop()}</span>}
        </span>
      </div>

      <div className="body">
        <Sidebar
          chats={history.chats}
          repo={workspace.path}
          activeChat={agent.chatId}
          onSelectChat={agent.open}
          onNewChat={newChat}
          onToggleCollapse={() => setCollapsed((v) => !v)}
          onOpenSettings={() => setSettingsOpen(true)}
          onOnboardingAction={openTab}
        />

        <PanelResizeHandle
          ariaLabel="Resize the chat list"
          onResize={panels.resizeSidebarBy}
          onResizeEnd={panels.endResize}
        />

        <main className="main">
          <ChatPanel
            workspace={workspace.path}
            index={index}
            turn={agent.turn}
            usage={agent.turn.usage}
            context={agent.context}
            onDecide={agent.decide}
            onOpenRepo={chooseFolder}
            onNewChat={newChat}
            onCompact={compactNow}
            onImplement={conversation.value === "plan" ? implement : undefined}
            onOpenPlan={() => openTab("plan")}
            branchable={agent.branchable}
            onBranch={agent.branch}
          />
          <Composer
            onSend={send}
            onStop={agent.cancel}
            running={agent.turn.status === "running"}
            conversation={conversation.value}
            onConversation={pickConversation}
            unattended={unattended.value}
            onUnattended={pickUnattended}
            draft={agent.draft}
          />
        </main>

        <PanelResizeHandle
          invert
          ariaLabel="Resize the side panel"
          onResize={panels.resizeAsideBy}
          onResizeEnd={panels.endResize}
        />

        <AsidePanel
          tab={tab}
          onTabChange={setTab}
          collapsed={asideCollapsed}
          onToggleCollapse={() => setAsideCollapsed((v) => !v)}
          onNotify={toast.show}
          mcp={mcp.view}
          mcpError={mcpEditing ? null : mcp.error}
          onMcpToggle={mcp.setEnabled}
          onMcpEdit={() => setMcpEditing(true)}
          hooks={hooks.view}
          hooksError={hooksEditing ? null : hooks.error}
          onHooksEdit={() => setHooksEditing(true)}
          processes={processes.processes}
          processesError={processes.error}
          onProcessStop={processes.stop}
          skills={skills.view}
          skillsError={skills.error}
          onSkillToggle={skills.setEnabled}
          rules={rules.rules}
          rulesError={rules.error}
          onRuleToggle={rules.setEnabled}
          plan={agent.plan}
          checklist={agent.checklist}
          onPlanEdit={agent.editPlan}
          onImplement={conversation.value === "plan" ? implement : undefined}
          planLocked={agent.turn.status === "running"}
        />
      </div>

      <Modal
        title="Settings"
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        footer={
          <button className="btn btn-ghost" type="button" onClick={() => setSettingsOpen(false)}>
            Close
          </button>
        }
      >
        <ProviderSettings
          settings={llm.settings}
          busy={llm.busy}
          error={llm.error}
          onSave={llm.save}
          onRemove={llm.remove}
          onSelect={llm.select}
          onDebugLogging={llm.debugLogging}
        />
        <div className="modal-field">
          <label>Theme</label>
          <div className="segmented" role="radiogroup" aria-label="Theme">
            {THEMES.map((name) => (
              <button
                key={name}
                type="button"
                role="radio"
                aria-checked={theme.preference === name}
                className={`segment${theme.preference === name ? " active" : ""}`}
                onClick={() => theme.setPreference(name)}
              >
                {name}
              </button>
            ))}
          </div>
        </div>
        <div className="modal-field">
          <label>Tool calls</label>
          <button
            className="btn btn-ghost"
            type="button"
            onClick={() => {
              setSettingsOpen(false);
              setLogOpen(true);
            }}
          >
            Open the log
          </button>
        </div>
        <DataPolicy
          provider={llm.settings?.providers.find((p) => p.id === llm.settings?.activeProviderId) ?? null}
          debugLogging={llm.settings?.debugLogging ?? false}
          mcpServers={mcp.view?.servers ?? null}
          hooks={hooks.view?.hooks ?? null}
        />
      </Modal>

      <Modal title="Tool calls" wide open={logOpen} onClose={() => setLogOpen(false)}>
        <ToolLog
          rows={toolLog.rows}
          total={toolLog.total}
          filter={toolLog.filter}
          onFilter={toolLog.setFilter}
          onMore={toolLog.more}
          onClear={toolLog.clear}
          enabled={toolLog.enabled}
          onToggle={toolLog.toggle}
          error={toolLog.error}
        />
      </Modal>

      <Modal title="MCP servers" open={mcpEditing} onClose={() => setMcpEditing(false)}>
        <ConfigFileEditor
          label="MCP configuration"
          text={mcp.view?.text}
          error={mcp.error}
          onSave={mcp.save}
          onClose={() => setMcpEditing(false)}
          note={
            <>
              The <code>mcpServers</code> format of Claude Desktop and Cursor: paste a server's snippet as it is.
              Optional per server: <code>weight</code> (cost of a call in the turn's budget, 3 by default) and{" "}
              <code>timeoutSecs</code> (120). Kept in {mcp.view?.path || "the app directory"}, readable only by you —
              it may hold tokens.
            </>
          }
        />
      </Modal>

      <Modal title="Hooks" open={hooksEditing} onClose={() => setHooksEditing(false)}>
        <ConfigFileEditor
          label="Hooks configuration"
          text={hooks.view?.text}
          error={hooks.error}
          onSave={hooks.save}
          onClose={() => setHooksEditing(false)}
          note={
            <>
              Claude Code's <code>hooks</code> format: <code>PreToolUse</code>, <code>PostToolUse</code> and{" "}
              <code>Stop</code> run here. The command gets the event as JSON on stdin; exit code 2 refuses the call,
              or sends the agent back from Stop, with stderr as the reason. A <code>matcher</code> names this app's
              tools — <code>runCommand</code>, <code>editFile</code> — not Claude Code's. Kept in{" "}
              {hooks.view?.path || "the app directory"}.
            </>
          }
        />
      </Modal>

      <Toast message={toast.message} />
    </div>
  );
}

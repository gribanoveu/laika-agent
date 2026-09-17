import { useState } from "react";
import { Sidebar } from "./components/Sidebar";
import { ChatPanel } from "./components/ChatPanel";
import { Composer } from "./components/Composer";
import { AsidePanel } from "./components/AsidePanel";
import { Modal } from "./components/Modal";
import { ProviderSettings } from "./components/ProviderSettings";
import { PanelResizeHandle } from "./components/PanelResizeHandle";
import { Toast } from "./components/Toast";
import { WindowControls } from "./components/WindowControls";
import { useAgentTurn } from "./hooks/useAgentTurn";
import { useChatHistory } from "./hooks/useChatHistory";
import { useNarrowCollapse } from "./hooks/useNarrowCollapse";
import { useLlmSettings } from "./hooks/useLlmSettings";
import { useWorkspace } from "./hooks/useWorkspace";
import { usePanelSizes } from "./hooks/usePanelSizes";
import { useTheme, THEMES } from "./hooks/useTheme";
import { useToast } from "./hooks/useToast";
import { startWindowDrag, toggleMaximizeWindow } from "./lib/window";
import { useConversationMode } from "./hooks/useConversationMode";
import type { ConversationMode } from "./lib/chat";
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

export default function App() {
  const [collapsed, setCollapsed] = useState(false);
  const [asideCollapsed, setAsideCollapsed] = useState(false);
  const [tab, setTab] = useState<AsideTab>("changes");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const conversation = useConversationMode();
  const toast = useToast();
  const workspace = useWorkspace();
  const history = useChatHistory(workspace.path);
  // The list is redrawn from disk after every save rather than guessed at
  // here: what belongs in it, and in what order, is the store's rule.
  const agent = useAgentTurn({ onSaved: history.refresh });
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
  const activeProvider =
    llm.settings?.providers.find((p) => p.id === llm.settings?.activeProviderId) ??
    llm.settings?.providers[0];

  const compactNow = async () => {
    if (!(await agent.compact(true))) {
      toast.show(agent.error ?? "Nothing worth folding away yet");
    }
  };

  const pickConversation = async (mode: ConversationMode) => {
    const failed = await conversation.pick(mode);
    if (failed) toast.show(failed);
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
            turn={agent.turn}
            usage={agent.turn.usage}
            contextLimit={activeProvider?.contextLimit ?? null}
            onDecide={agent.decide}
            onOpenRepo={chooseFolder}
            onNewChat={newChat}
            onCompact={compactNow}
          />
          <Composer
            onSend={send}
            onStop={agent.cancel}
            running={agent.turn.status === "running"}
            onNotify={toast.show}
            conversation={conversation.mode}
            onConversation={pickConversation}
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
      </Modal>

      <Toast message={toast.message} />
    </div>
  );
}

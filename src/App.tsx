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
import { useNarrowCollapse } from "./hooks/useNarrowCollapse";
import { useLlmSettings } from "./hooks/useLlmSettings";
import { useWorkspace } from "./hooks/useWorkspace";
import { usePanelSizes } from "./hooks/usePanelSizes";
import { useTheme, THEMES } from "./hooks/useTheme";
import { useToast } from "./hooks/useToast";
import { startWindowDrag, toggleMaximizeWindow } from "./lib/window";
import type { AsideTab, ChatSummary } from "./types";
import "./App.css";

// The chat list waits on the store that would hold it (stage 3, F-3.8); the
// conversation itself is live.
const chats: ChatSummary[] = [];

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
  const [activeChat, setActiveChat] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [openFolder, setOpenFolder] = useState(false);
  const [folderPath, setFolderPath] = useState("");
  const toast = useToast();
  const workspace = useWorkspace();
  const agent = useAgentTurn();
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

  // Nothing to tell the backend: it keeps no conversation of its own, so
  // forgetting this one here is the whole of starting over.
  const newChat = () => agent.reset();

  const send = (text: string) => {
    if (!workspace.path) {
      setOpenFolder(true);
      return;
    }
    agent.send(text);
  };

  const chooseFolder = async () => {
    if (await workspace.open(folderPath)) {
      setOpenFolder(false);
      setFolderPath("");
    } else if (workspace.error) {
      toast.show(workspace.error);
    }
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
          chats={chats}
          repo={workspace.path}
          activeChat={activeChat}
          onSelectChat={setActiveChat}
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
            onDecide={agent.decide}
            onOpenRepo={() => setOpenFolder(true)}
            onNewChat={newChat}
          />
          <Composer
            onSend={send}
            onStop={agent.cancel}
            running={agent.turn.status === "running"}
            onNotify={toast.show}
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

      <Modal
        title="Open folder"
        open={openFolder}
        onClose={() => setOpenFolder(false)}
        footer={
          <>
            <button className="btn btn-ghost" type="button" onClick={() => setOpenFolder(false)}>
              Cancel
            </button>
            <button className="btn btn-primary" type="button" onClick={chooseFolder}>
              Open
            </button>
          </>
        }
      >
        <div className="modal-field">
          <label>Folder</label>
          {/* A typed path until the file-dialog plugin lands: the app draws its
              own dialogs, and a native `prompt()` would arrive looking like a
              different program. */}
          <input
            type="text"
            value={folderPath}
            placeholder="/path/to/project"
            autoFocus
            onChange={(e) => setFolderPath(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && chooseFolder()}
          />
        </div>
        <p className="modal-note">
          The agent reads and writes inside this folder. A command it runs is not
          confined to it — that is what the approval prompts are for.
        </p>
      </Modal>

      <Toast message={toast.message} />
    </div>
  );
}

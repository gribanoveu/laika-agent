import { useState } from "react";
import { Sidebar } from "./components/Sidebar";
import { ChatPanel } from "./components/ChatPanel";
import { Composer } from "./components/Composer";
import { AsidePanel } from "./components/AsidePanel";
import { Modal } from "./components/Modal";
import { PanelResizeHandle } from "./components/PanelResizeHandle";
import { Toast } from "./components/Toast";
import { WindowControls } from "./components/WindowControls";
import { useNarrowCollapse } from "./hooks/useNarrowCollapse";
import { usePanelSizes } from "./hooks/usePanelSizes";
import { useTheme, THEMES } from "./hooks/useTheme";
import { useToast } from "./hooks/useToast";
import { startWindowDrag, toggleMaximizeWindow } from "./lib/window";
import type { AsideTab, ChatSummary, Session, Turn } from "./types";
import "./App.css";

// Nothing is wired to the backend yet. These are the seams: each one becomes a
// hook calling a typed wrapper from src/lib/ once the command behind it exists.
const session: Session | null = null;
const chats: ChatSummary[] = [];
const turns: Turn[] = [];

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
  const toast = useToast();
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

  const newChat = () => toast.show("Starting a chat is not wired yet");

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
          atlas-cli{session && <span> · {session.repo}</span>}
        </span>
      </div>

      <div className="body">
        <Sidebar
          chats={chats}
          repo={session?.repo ?? null}
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
            session={session}
            turns={turns}
            onPickBranch={() => toast.show("Branch picker is not wired yet")}
            onOpenRepo={() => toast.show("Opening a repository is not wired yet")}
            onNewChat={newChat}
          />
          <Composer onNotify={toast.show} />
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
        <div className="modal-field">
          <label>Git user.name</label>
          <input type="text" value="" placeholder="Not configured" readOnly />
        </div>
        <div className="modal-field">
          <label>Git user.email</label>
          <input type="text" value="" placeholder="Not configured" readOnly />
        </div>
        <div className="modal-field">
          <label>LLM provider</label>
          <input type="text" value="" placeholder="Not configured" readOnly />
        </div>
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
        <p className="modal-note">Settings are read-only until the backend commands land.</p>
      </Modal>

      <Toast message={toast.message} />
    </div>
  );
}

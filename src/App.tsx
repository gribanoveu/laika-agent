import { useState } from "react";
import { Sidebar } from "./components/Sidebar";
import { ChatPanel } from "./components/ChatPanel";
import { Composer } from "./components/Composer";
import { AsidePanel } from "./components/AsidePanel";
import { Modal } from "./components/Modal";
import { Toast } from "./components/Toast";
import { WindowControls } from "./components/WindowControls";
import { useToast } from "./hooks/useToast";
import { startWindowDrag, toggleMaximizeWindow } from "./lib/window";
import { CHATS, SESSION } from "./mock/data";
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
  const [tab, setTab] = useState<AsideTab>("context");
  const [activeChat, setActiveChat] = useState(CHATS[0].id);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const toast = useToast();

  const openTab = (next: AsideTab) => {
    setTab(next);
    setAsideCollapsed(false);
  };

  return (
    <div
      className={`window${collapsed ? " collapsed" : ""}${asideCollapsed ? " aside-collapsed" : ""}`}
    >
      <div className="titlebar" onMouseDown={dragOrMaximize}>
        <WindowControls />
        <span className="titlebar-title">
          atlas-cli · <span>{SESSION.repo}</span>
        </span>
      </div>

      <div className="body">
        <Sidebar
          activeChat={activeChat}
          onSelectChat={setActiveChat}
          onToggleCollapse={() => setCollapsed((v) => !v)}
          onOpenSettings={() => setSettingsOpen(true)}
          onOnboardingAction={openTab}
        />

        <main className="main">
          <ChatPanel onPickBranch={() => toast.show("Branch picker is not wired yet")} />
          <Composer onNotify={toast.show} />
        </main>

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
          <input type="text" value="Eugene" readOnly />
        </div>
        <div className="modal-field">
          <label>Git user.email</label>
          <input type="text" value="eugene@example.com" readOnly />
        </div>
        <div className="modal-field">
          <label>LLM provider</label>
          <input type="text" value="OpenRouter" readOnly />
        </div>
        <p className="modal-note">Skeleton — поля появятся после настроек в backend.</p>
      </Modal>

      <Toast message={toast.message} />
    </div>
  );
}

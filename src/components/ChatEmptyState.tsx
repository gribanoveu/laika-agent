import { FolderGit2, MessageSquarePlus } from "lucide-react";
import type { Session } from "../types";
import "./ChatEmptyState.css";

type Props = {
  session: Session | null;
  onOpenRepo: () => void;
  onNewChat: () => void;
};

/** Fills the thread while there is nothing to show: no workspace, or no messages. */
export function ChatEmptyState({ session, onOpenRepo, onNewChat }: Props) {
  const Icon = session ? MessageSquarePlus : FolderGit2;

  return (
    <div className="chat-empty">
      <span className="chat-empty-ico">
        <Icon size={22} />
      </span>
      <h2 className="chat-empty-title">
        {session ? "Start the conversation" : "Nothing is open"}
      </h2>
      <p className="chat-empty-text">
        {session
          ? `Describe a task and the agent works through ${session.repo}, showing every tool call as it goes.`
          : "Open a git repository to give the agent a workspace, then describe what needs doing."}
      </p>
      <div className="chat-empty-actions">
        {!session && (
          <button className="btn btn-primary" type="button" onClick={onOpenRepo}>
            <FolderGit2 size={13} />
            Open repository…
          </button>
        )}
        <button className="btn btn-ghost" type="button" onClick={onNewChat}>
          <MessageSquarePlus size={13} />
          New chat
        </button>
      </div>
      <p className="chat-empty-hint">
        <kbd>Enter</kbd> to send
        <span className="sep">·</span>
        <kbd>Shift</kbd>
        <span className="plus">+</span>
        <kbd>Enter</kbd> for a new line
      </p>
    </div>
  );
}

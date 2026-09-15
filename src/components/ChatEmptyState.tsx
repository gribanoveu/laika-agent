import { FolderGit2, MessageSquarePlus } from "lucide-react";
import "./ChatEmptyState.css";

type Props = {
  /** The open folder's display path, or `null` when nothing is open. */
  workspace: string | null;
  onOpenRepo: () => void;
  onNewChat: () => void;
};

/** Fills the thread while there is nothing to show: no workspace, or no messages. */
export function ChatEmptyState({ workspace, onOpenRepo, onNewChat }: Props) {
  const Icon = workspace ? MessageSquarePlus : FolderGit2;
  const name = workspace?.split("/").filter(Boolean).pop() ?? "";

  return (
    <div className="chat-empty">
      <span className="chat-empty-ico">
        <Icon size={22} />
      </span>
      <h2 className="chat-empty-title">
        {workspace ? "Start the conversation" : "Nothing is open"}
      </h2>
      <p className="chat-empty-text">
        {workspace
          ? `Describe a task and the agent works through ${name}, showing every tool call as it goes.`
          : "Open a folder to give the agent a workspace. It reads and writes inside that folder; a command it runs is not confined to it, which is what the approval prompts are for."}
      </p>
      <div className="chat-empty-actions">
        {!workspace && (
          <button className="btn btn-primary" type="button" onClick={onOpenRepo}>
            <FolderGit2 size={13} />
            Open folder…
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

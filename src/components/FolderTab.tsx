import { Folder, FolderPlus, GitBranch } from "lucide-react";
import { Dropdown } from "./Dropdown";
import { IndexBadge } from "./IndexBadge";
import type { ChangeTotals } from "../lib/chat";
import type { IndexState } from "../lib/indexStatus";
import "./FolderTab.css";

/** Not a path: no folder is called this. */
const PICK = "\u0000pick";

const folderName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

type Props = {
  /** The open folder; `null` when nothing is open. */
  path: string | null;
  /** Folders opened lately, the last one first. */
  recent: string[];
  onOpenFolder: (path: string) => void;
  onPickFolder: () => void;
  /** The git branch checked out in the folder; `null` outside a repository. */
  branch?: string | null;
  /** The folder's index; `null` until anything is known about it. */
  index?: IndexState | null;
  /** Lines added and removed since the last commit; `null` outside a repository. */
  changes?: ChangeTotals | null;
  onOpenChanges: () => void;
};

/** Where the next message will be worked on: the folder, its branch, its
    index and what it holds against its last commit. Sits on the composer's
    top edge — this is read before sending, not looked up in the sidebar. */
export function FolderTab({ path, recent, onOpenFolder, onPickFolder, branch = null, index = null, changes = null, onOpenChanges }: Props) {
  return (
    <div className={`folder-tab${path ? "" : " empty"}`}>
      <Dropdown
        title={path ?? "Choose the folder the agent works in"}
        heading="Folder"
        label={
          <span className="folder-tab-name">
            {path ? <Folder size={13} /> : <FolderPlus size={13} />}
            {path ? folderName(path) : "Select folder"}
          </span>
        }
        value={path ?? ""}
        options={[
          ...recent.map((one) => ({ value: one, label: folderName(one), hint: one })),
          { value: PICK, label: "Open folder…" },
        ]}
        onPick={(value) => (value === PICK ? onPickFolder() : value !== path && onOpenFolder(value))}
      />
      {path && branch && (
        <span className="folder-tab-branch" title="Checked-out branch">
          <GitBranch size={12} />
          <span>{branch}</span>
        </span>
      )}
      {path && index && <IndexBadge state={index} />}
      {path && changes && changes.files > 0 && (
        <button
          type="button"
          className="folder-tab-changes"
          title={`${changes.files} ${changes.files === 1 ? "file" : "files"} changed since the last commit`}
          onClick={onOpenChanges}
        >
          <span className="add">+{changes.add}</span>
          <span className="del">−{changes.del}</span>
        </button>
      )}
    </div>
  );
}

import { Check, Folder, FolderPlus, GitBranch, GitFork } from "lucide-react";
import { Dropdown } from "./Dropdown";
import { IndexBadge } from "./IndexBadge";
import type { BranchRef, ChangeTotals, RecentFolder } from "../lib/chat";
import type { IndexState } from "../lib/indexStatus";
import "./FolderTab.css";

/** Not paths: no folder is called these. */
const PICK = "\u0000pick";
const REMOVE = "\u0000remove";

const folderName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

/**
 * The folder menu's rows: each folder the last one first, and a worktree
 * straight under the folder it was made from, marked as belonging to it. One
 * whose folder is not in the list stays where it falls, said to be one but
 * not drawn under a folder it does not belong to.
 */
export function folderOptions(recent: RecentFolder[]) {
  const trimmed = (path: string) => path.replace(/[\\/]+$/, "");
  const listed = new Set(recent.map((one) => trimmed(one.path)));
  const row = (one: RecentFolder, nested = false) =>
    one.worktreeOf
      ? { value: one.path, label: folderName(one.path), hint: `worktree of ${folderName(one.worktreeOf)}`, nested }
      : { value: one.path, label: folderName(one.path), hint: one.path };
  return recent.flatMap((one) => {
    if (one.worktreeOf) return listed.has(trimmed(one.worktreeOf)) ? [] : [row(one)];
    const own = recent.filter((other) => other.worktreeOf && trimmed(other.worktreeOf) === trimmed(one.path));
    return [row(one), ...own.map((other) => row(other, true))];
  });
}

type Props = {
  /** The open folder; `null` when nothing is open. */
  path: string | null;
  /** Folders opened lately, the last one first. */
  recent: RecentFolder[];
  onOpenFolder: (path: string) => void;
  onPickFolder: () => void;
  /** Offered while the open folder is a worktree. */
  onRemoveWorktree?: () => void;
  /** The main working tree, when the folder is a git worktree of it. */
  worktreeOf?: string | null;
  /** The git branch checked out in the folder; `null` outside a repository. */
  branch?: string | null;
  /** The folder's index; `null` until anything is known about it. */
  index?: IndexState | null;
  /** Lines added and removed since the last commit; `null` outside a repository. */
  changes?: ChangeTotals | null;
  onOpenChanges: () => void;
  /** Offered while the chat has not started: switch the branch, or start a
      worktree from one. Without it the branch is only shown. */
  branchPicker?: {
    branches: BranchRef[];
    onOpen: () => void;
    onPick: (name: string) => void;
    worktree: boolean;
    /** Where a worktree starts, shown in place of the branch while Worktree is on. */
    base?: string | null;
    onWorktree: (on: boolean) => void;
  };
};

/** Where the next message will be worked on: the folder, its branch, its
    index and what it holds against its last commit. Sits on the composer's
    top edge — this is read before sending, not looked up in the sidebar. */
export function FolderTab({
  path,
  recent,
  onOpenFolder,
  onPickFolder,
  onRemoveWorktree,
  branch = null,
  worktreeOf = null,
  index = null,
  changes = null,
  onOpenChanges,
  branchPicker,
}: Props) {
  const shownBranch = (branchPicker?.worktree && branchPicker.base) || branch || "";
  return (
    <div className={`folder-tab${path ? "" : " no-folder"}`}>
      <Dropdown
        title={
          path && worktreeOf
            ? `A worktree of ${worktreeOf}\nThis folder: ${path}`
            : path ?? "Choose the folder the agent works in"
        }
        heading="Folder"
        label={
          path && worktreeOf ? (
            // Named after the repository it belongs to: the worktree's own
            // folder name says no more than the branch beside it.
            <span className="folder-tab-name in-worktree">
              <GitFork size={13} />
              <span className="folder-tab-text">{folderName(worktreeOf)}</span>
              <span className="folder-tab-tag">worktree</span>
            </span>
          ) : (
            <span className="folder-tab-name">
              {path ? <Folder size={13} /> : <FolderPlus size={13} />}
              <span className="folder-tab-text">{path ? folderName(path) : "Select folder"}</span>
            </span>
          )
        }
        value={path ?? ""}
        options={[
          ...folderOptions(recent),
          { value: PICK, label: "Open folder…" },
          ...(path && worktreeOf && onRemoveWorktree ? [{ value: REMOVE, label: "Remove this worktree…" }] : []),
        ]}
        onPick={(value) => {
          if (value === PICK) onPickFolder();
          else if (value === REMOVE) onRemoveWorktree?.();
          else if (value !== path) onOpenFolder(value);
        }}
      />
      {path && branch && branchPicker && (
        <>
          <Dropdown
            title={branchPicker.worktree ? "The branch a new worktree starts from" : "Switch the folder's branch"}
            heading={branchPicker.worktree ? "Start a worktree from" : "Branch"}
            label={
              <span className="folder-tab-name">
                <GitBranch size={12} />
                <span className="folder-tab-text">{shownBranch}</span>
              </span>
            }
            value={shownBranch}
            options={branchPicker.branches.map((one) => ({ value: one.name, hint: one.remote ? "remote" : undefined }))}
            emptyLabel="Reading branches…"
            onOpen={branchPicker.onOpen}
            onPick={branchPicker.onPick}
          />
          <button
            type="button"
            role="checkbox"
            className={`folder-tab-worktree${branchPicker.worktree ? " on" : ""}`}
            aria-checked={branchPicker.worktree}
            aria-label="Worktree"
            title={`The first message starts a worktree from ${shownBranch}: its own folder on a new branch, this one left as it is`}
            onClick={() => branchPicker.onWorktree(!branchPicker.worktree)}
          >
            <span className="folder-tab-check" aria-hidden="true">
              {branchPicker.worktree && <Check size={9} strokeWidth={3.5} />}
            </span>
            <GitFork size={12} />
            <span className="folder-tab-narrow-hide">Worktree</span>
          </button>
        </>
      )}
      {path && branch && !branchPicker && (
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

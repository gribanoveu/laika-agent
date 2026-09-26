import { Modal } from "./Modal";
import type { BranchConflict } from "../hooks/useBranchPicker";
import "./BranchConflictDialog.css";

type Props = {
  conflict: BranchConflict | null;
  onWorktree: (branch: string) => void;
  onClose: () => void;
};

/** A branch switch that was refused, and why. Nothing in the folder was touched. */
export function BranchConflictDialog({ conflict, onWorktree, onClose }: Props) {
  return (
    <Modal
      title="Changes are in the way"
      open={conflict !== null}
      onClose={onClose}
      footer={
        <>
          <button type="button" className="btn btn-ghost" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="btn btn-primary" onClick={() => conflict && onWorktree(conflict.branch)}>
            Start a worktree
          </button>
        </>
      }
    >
      {conflict && (
        <>
          <p className="conflict-text">
            Switching to <b>{conflict.branch}</b> would overwrite uncommitted changes in:
          </p>
          <ul className="conflict-paths">
            {conflict.paths.map((path) => (
              <li key={path}>{path}</li>
            ))}
          </ul>
          <p className="conflict-text">
            Nothing was changed. Commit or stash them first — or ask the agent to — or start a worktree from{" "}
            <b>{conflict.branch}</b>, which leaves this folder as it is.
          </p>
        </>
      )}
    </Modal>
  );
}

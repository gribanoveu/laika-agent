import { Modal } from "./Modal";
import type { SwitchBlock } from "../hooks/useFolderSwitch";
import "./FolderSwitchDialog.css";

type Props = {
  blocked: SwitchBlock | null;
  /** The folder open now — the one that would be left. */
  folder: string | null;
  onStopAgent: () => void;
  onClose: () => void;
};

/** Says what opening another folder would end, before it does. */
export function FolderSwitchDialog({ blocked, folder, onStopAgent, onClose }: Props) {
  const name = folder?.split(/[\\/]/).filter(Boolean).pop() ?? "this folder";
  const agent = blocked?.kind === "agent";

  return (
    <Modal
      title={agent ? "The agent is still working" : "Stop what runs here?"}
      open={blocked !== null}
      onClose={onClose}
      footer={
        <>
          <button type="button" className="btn btn-ghost" onClick={onClose}>
            Cancel
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => {
              if (blocked?.kind === "agent") onStopAgent();
              else blocked?.go();
              onClose();
            }}
          >
            {agent ? "Stop the agent" : "Stop them and switch"}
          </button>
        </>
      }
    >
      {blocked?.kind === "agent" ? (
        <p className="switch-text">
          It is working in <b>{name}</b>, and its chat is saved there when it finishes. Stop it first, then open the
          other folder.
        </p>
      ) : (
        blocked && (
          <>
            <p className="switch-text">
              Opening another folder stops what runs in <b>{name}</b>:
            </p>
            <ul className="switch-processes">
              {blocked.processes.map((p) => (
                <li key={`p${p.id}`}>{p.command}</li>
              ))}
              {blocked.terminals.map((t) => (
                <li key={`t${t.id}`}>terminal: {t.shell}</li>
              ))}
            </ul>
          </>
        )
      )}
    </Modal>
  );
}

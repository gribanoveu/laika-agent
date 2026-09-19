import { useEffect, useState, type ReactNode } from "react";
import "./ConfigFileEditor.css";

// A settings file edited whole, in one box, because its format is one that
// others already write — an MCP server's README, a hook shared for Claude
// Code: pasting that snippet is the fastest way in, and a field-by-field form
// would have to be translated back into it.

type Props = {
  /** Names the box for assistive technology. */
  label: string;
  /** The file as it stands on disk. */
  text: string | undefined;
  error: string | null;
  /** What the format is and where the file lives. */
  note: ReactNode;
  /** Resolves to whether it was stored. */
  onSave: (text: string) => Promise<boolean>;
  onClose: () => void;
};

export function ConfigFileEditor({ label, text, error, note, onSave, onClose }: Props) {
  const [draft, setDraft] = useState(text ?? "");
  // Follows the file until the user starts typing, so the box is never a
  // stale copy of what is on disk.
  const [touched, setTouched] = useState(false);
  useEffect(() => {
    if (!touched) setDraft(text ?? "");
  }, [text, touched]);

  const save = async () => {
    if (await onSave(draft)) {
      setTouched(false);
      onClose();
    }
  };

  return (
    <div className="config-file">
      <textarea
        className="config-file-text"
        aria-label={label}
        spellCheck={false}
        value={draft}
        onChange={(e) => {
          setDraft(e.target.value);
          setTouched(true);
        }}
      />
      {error && <p className="modal-note config-file-error">{error}</p>}
      <p className="modal-note">{note}</p>
      <div className="config-file-actions">
        <button className="btn btn-primary" type="button" onClick={save}>
          Save
        </button>
        <button className="btn btn-ghost" type="button" onClick={onClose}>
          Cancel
        </button>
      </div>
    </div>
  );
}

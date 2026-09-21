import { useMemo } from "react";
import { diffRows } from "../lib/diffRows";
import "./DiffView.css";

const SIGN = { context: " ", add: "+", del: "-" } as const;

/**
 * A unified diff drawn as lines with both line numbers, and within an edited
 * line the words that changed. Scrolls inside itself, so a long diff does not
 * push away whatever sits under it.
 */
export function DiffView({ unified }: { unified: string }) {
  const rows = useMemo(() => diffRows(unified), [unified]);

  // Nothing the parser could read: the text as it came, still better than blank.
  if (rows.length === 0) return <pre className="diff-view diff-view-raw">{unified}</pre>;

  return (
    <div className="diff-view">
      {rows.map((row, i) =>
        !("parts" in row) ? (
          <div key={i} className={`diff-row diff-${row.kind}`}>
            {row.text}
          </div>
        ) : (
          <div key={i} className={`diff-row diff-${row.kind}`}>
            <span className="diff-no">{row.oldNo ?? ""}</span>
            <span className="diff-no">{row.newNo ?? ""}</span>
            <span className="diff-sign">{SIGN[row.kind]}</span>
            <span className="diff-text">
              {row.parts.map((part, j) =>
                part.changed ? <mark key={j}>{part.text}</mark> : part.text,
              )}
            </span>
          </div>
        ),
      )}
    </div>
  );
}

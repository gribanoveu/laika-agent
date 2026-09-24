import { useMemo, type CSSProperties } from "react";
import { diffRows, type DiffRow } from "../lib/diffRows";
import "./DiffView.css";

const SIGN = { context: " ", add: "+", del: "-" } as const;

/**
 * A unified diff drawn as lines with both line numbers, and within an edited
 * line the words that changed — or rows already made, as the file viewer has. Scrolls inside itself, so a long diff does not
 * push away whatever sits under it.
 */
export function DiffView({ unified, rows: given }: { unified?: string; rows?: DiffRow[] }) {
  const parsed = useMemo(() => (given ? null : diffRows(unified ?? "")), [unified, given]);
  const rows = given ?? parsed ?? [];

  // Nothing the parser could read: the text as it came, still better than blank.
  if (!given && rows.length === 0) return <pre className="diff-view diff-view-raw">{unified}</pre>;

  // Every row's numbers as wide as the longest, so the text lines up.
  const digits = String(rows.reduce((most, row) => ("parts" in row ? Math.max(most, row.oldNo ?? 0, row.newNo ?? 0) : most), 0)).length;
  return (
    <div className="diff-view" style={{ "--diff-digits": `${Math.max(3, digits)}ch` } as CSSProperties}>
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

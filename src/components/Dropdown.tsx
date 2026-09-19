import { useEffect, useRef, useState, type ReactNode } from "react";
import { ChevronDown } from "lucide-react";
import "./Dropdown.css";

/** `label` when what the caller sends and what the reader sees differ — a wire
    value like "plan" is not a word to put in a menu. */
type Option = { value: string; label?: string; hint?: string };

type Props = {
  label: ReactNode;
  title?: string;
  options: Option[];
  value: string;
  onPick: (value: string) => void;
  mono?: boolean;
  emptyLabel?: string;
  /** Where the menu opens; above by default — the composer sits at the bottom. */
  below?: boolean;
};

/** Trigger + role="listbox" menu — the app draws its own dropdowns, never <select>. */
export function Dropdown({ label, title, options, value, onPick, mono, emptyLabel, below }: Props) {
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div className="dropdown-wrap" ref={wrap}>
      <button
        type="button"
        className={`chip${open ? " open" : ""}`}
        title={title}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <span className={mono ? "chip-model" : undefined}>{label}</span>
        <ChevronDown className="chip-chev" size={10} />
      </button>
      {open && (
        <div className={`dropdown-menu${below ? " below" : ""}`} role="listbox">
          {options.length === 0 && (
            <div className="dropdown-empty">{emptyLabel ?? "Nothing here yet"}</div>
          )}
          {options.map((opt) => (
            <button
              key={opt.value}
              type="button"
              role="option"
              aria-selected={opt.value === value}
              className={`dropdown-item${opt.value === value ? " active" : ""}`}
              onClick={() => {
                onPick(opt.value);
                setOpen(false);
              }}
            >
              {opt.label ?? opt.value}
              {opt.hint && <span className="hint">{opt.hint}</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

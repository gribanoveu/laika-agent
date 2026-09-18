import { useState } from "react";
import { ChevronRight, Plus } from "lucide-react";
import type { PanelItem } from "../types";
import "./ItemList.css";

function Item({ item, onToggle }: { item: PanelItem; onToggle?: (id: string, enabled: boolean) => void }) {
  const [open, setOpen] = useState(false);
  const enabled = item.enabled ?? false;

  return (
    <div className={`item${open ? " open" : ""}`}>
      <div className="item-head" onClick={() => setOpen((v) => !v)}>
        <span className={`item-ico ${item.kind}`}>{item.badge}</span>
        <div className="item-body">
          <div className="item-title">
            {item.title}
            {item.status && (
              <span className={`badge-pill ${item.status.tone}`}>{item.status.label}</span>
            )}
          </div>
          <div className="item-desc">{item.desc}</div>
          {(item.meta || item.tags) && (
            <div className="item-meta">
              {item.meta}
              {item.tags?.map((tag) => (
                <span className="skill-tag" key={tag}>
                  {tag}
                </span>
              ))}
            </div>
          )}
        </div>
        <div className="item-actions">
          {item.enabled !== undefined && onToggle && (
            <button
              type="button"
              className={`toggle${enabled ? " on" : ""}`}
              aria-pressed={enabled}
              aria-label={enabled ? "Enabled" : "Disabled"}
              onClick={(e) => {
                e.stopPropagation();
                onToggle(item.id, !enabled);
              }}
            />
          )}
          <ChevronRight className="item-chev" size={12} />
        </div>
      </div>
      {open && (
        <div className="item-detail">
          {item.rows?.map((row) => (
            <div className="tool-row" key={row.name}>
              <span className="tname">{row.name}</span>
              <span className="tdesc">{row.desc}</span>
            </div>
          ))}
          {item.note &&
            (item.kind === "rule" ? (
              <div className="rule-block">{item.note}</div>
            ) : (
              <div className="empty item-note">{item.note}</div>
            ))}
          {item.source && <div className="rule-source">{item.source}</div>}
        </div>
      )}
    </div>
  );
}

type Props = {
  label: string;
  count: string;
  items: PanelItem[];
  emptyLabel: string;
  addLabel?: string;
  onAdd?: () => void;
  /** Without it the items have no switch: the list cannot change what it shows. */
  onToggle?: (id: string, enabled: boolean) => void;
};

export function ItemList({ label, count, items, emptyLabel, addLabel, onAdd, onToggle }: Props) {
  return (
    <div className="panel-section">
      <div className="section-label">
        <span>{label}</span>
        <span className="count">{count}</span>
      </div>
      {items.length === 0 && <div className="empty">{emptyLabel}</div>}
      {items.map((item) => (
        <Item key={item.id} item={item} onToggle={onToggle} />
      ))}
      {addLabel && (
        <button className="add-btn" type="button" onClick={onAdd}>
          <Plus size={12} />
          {addLabel}
        </button>
      )}
    </div>
  );
}

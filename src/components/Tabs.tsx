import "./Tabs.css";

export type TabItem<T extends string> = {
  id: T;
  label: string;
  /** Shown beside the label when above zero. */
  count?: number;
  title?: string;
  disabled?: boolean;
};

/** A panel's own views, side by side in one segmented strip. */
export function Tabs<T extends string>({
  label,
  tabs,
  value,
  onChange,
}: {
  label: string;
  tabs: TabItem<T>[];
  value: T;
  onChange: (id: T) => void;
}) {
  return (
    <div className="tabs" role="tablist" aria-label={label}>
      {tabs.map((tab) => (
        <button
          key={tab.id}
          type="button"
          role="tab"
          aria-selected={value === tab.id}
          className={`tab${value === tab.id ? " active" : ""}`}
          title={tab.title}
          disabled={tab.disabled}
          onClick={() => onChange(tab.id)}
        >
          {tab.label}
          {!!tab.count && <span className="tab-count">{tab.count}</span>}
        </button>
      ))}
    </div>
  );
}

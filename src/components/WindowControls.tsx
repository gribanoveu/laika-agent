import { closeWindow, minimizeWindow, toggleMaximizeWindow } from "../lib/window";
import "./WindowControls.css";

export function WindowControls() {
  return (
    <div className="window-controls">
      <button className="dot r" type="button" title="Close" aria-label="Close" onClick={closeWindow}>
        <svg viewBox="0 0 10 10" aria-hidden="true">
          <path d="M3 3l4 4M7 3l-4 4" />
        </svg>
      </button>
      <button
        className="dot y"
        type="button"
        title="Minimize"
        aria-label="Minimize"
        onClick={minimizeWindow}
      >
        <svg viewBox="0 0 10 10" aria-hidden="true">
          <path d="M2.6 5h4.8" />
        </svg>
      </button>
      <button
        className="dot g"
        type="button"
        title="Maximize"
        aria-label="Maximize"
        onClick={toggleMaximizeWindow}
      >
        <svg viewBox="0 0 10 10" aria-hidden="true">
          <path d="M5 2.4v5.2M2.4 5h5.2" />
        </svg>
      </button>
    </div>
  );
}

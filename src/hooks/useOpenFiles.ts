import { useEffect, useReducer } from "react";
import type { FileTarget } from "../lib/chat";

/** The viewer's tabs, in the order they were opened, and the one showing. */
export type OpenFiles = { files: FileTarget[]; active: FileTarget | null };

type Action =
  | { kind: "open"; target: FileTarget }
  | { kind: "close"; target: FileTarget }
  | { kind: "closeAll" };

/** The same file on the same side: one tab, however often it is opened. */
export const sameFile = (a: FileTarget, b: FileTarget) => a.path === b.path && a.side === b.side;

const none: OpenFiles = { files: [], active: null };

/**
 * Opening a file already open shows its tab; closing the tab showing moves to
 * the one after it, or before it when it was the last.
 */
export function openFilesReducer(state: OpenFiles, action: Action): OpenFiles {
  switch (action.kind) {
    case "open": {
      const open = state.files.find((f) => sameFile(f, action.target));
      return open ? { ...state, active: open } : { files: [...state.files, action.target], active: action.target };
    }
    case "close": {
      const at = state.files.findIndex((f) => sameFile(f, action.target));
      if (at < 0) return state;
      const files = state.files.filter((_, i) => i !== at);
      const showing = state.active && sameFile(state.active, action.target);
      return { files, active: showing ? (files[at] ?? files[at - 1] ?? null) : state.active };
    }
    case "closeAll":
      return none;
  }
}

/** The files open in the viewer beside the chat; a folder opened elsewhere closes them all. */
export function useOpenFiles(workspace: string | null) {
  const [state, dispatch] = useReducer(openFilesReducer, none);
  useEffect(() => dispatch({ kind: "closeAll" }), [workspace]);
  return {
    ...state,
    open: (target: FileTarget) => dispatch({ kind: "open", target }),
    close: (target: FileTarget) => dispatch({ kind: "close", target }),
    closeAll: () => dispatch({ kind: "closeAll" }),
  };
}

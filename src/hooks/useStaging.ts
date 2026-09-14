import { useState } from "react";
import type { ChangedFile } from "../types";

const byName = (a: ChangedFile, b: ChangedFile) => a.name.localeCompare(b.name);

/** Staging area state. Seed both lists from a git status wrapper once it exists. */
export function useStaging() {
  const [unstaged, setUnstaged] = useState<ChangedFile[]>([]);
  const [staged, setStaged] = useState<ChangedFile[]>([]);

  const move = (name: string, from: ChangedFile[], to: ChangedFile[]) => {
    const file = from.find((f) => f.name === name);
    if (!file) return null;
    return {
      from: from.filter((f) => f.name !== name),
      to: [...to, file].sort(byName),
    };
  };

  return {
    unstaged,
    staged,
    stage(name: string) {
      const next = move(name, unstaged, staged);
      if (!next) return;
      setUnstaged(next.from);
      setStaged(next.to);
    },
    unstage(name: string) {
      const next = move(name, staged, unstaged);
      if (!next) return;
      setStaged(next.from);
      setUnstaged(next.to);
    },
    stageAll() {
      setStaged((s) => [...s, ...unstaged].sort(byName));
      setUnstaged([]);
    },
  };
}

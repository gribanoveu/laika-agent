import { useEffect, useState } from "react";
import { currentBranch } from "../lib/chat";

/**
 * The branch checked out in the open folder, for the chat header. Read again
 * when the folder changes and whenever a turn ends — the agent can switch
 * branches itself, with a command.
 */
export function useGitBranch(workspace: string | null, turnStatus: string) {
  const [branch, setBranch] = useState<string | null>(null);

  useEffect(() => {
    if (!workspace || turnStatus === "running") return;
    let live = true;
    currentBranch()
      .then((name) => live && setBranch(name))
      // A branch that cannot be read shows the folder instead; not worth a message.
      .catch(() => live && setBranch(null));
    return () => {
      live = false;
    };
  }, [workspace, turnStatus]);

  return workspace ? branch : null;
}

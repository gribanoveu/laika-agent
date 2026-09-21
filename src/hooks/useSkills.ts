import { useCallback, useEffect, useState } from "react";
import { setSkillEnabled, skillsList, type SkillsView } from "../lib/chat";

/**
 * The skills — the open folder's and the user's — re-read whenever the tab
 * showing them opens or another folder is opened: skills are files edited
 * outside the app, so a list read once at start-up would go stale without
 * anyone noticing.
 */
export function useSkills(visible: boolean, workspace: string | null = null) {
  const [view, setView] = useState<SkillsView | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setView(await skillsList());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    if (visible) reload();
  }, [visible, workspace, reload]);

  const setEnabled = useCallback(
    async (key: string, enabled: boolean) => {
      // Shown at once; the reload that follows is what the file now says —
      // including which skill of a name now wins.
      setError(null);
      setView((v) => v && { ...v, skills: v.skills.map((s) => (s.key === key ? { ...s, enabled } : s)) });
      try {
        await setSkillEnabled(key, enabled);
      } catch (e) {
        setError(String(e));
      }
      await reload();
    },
    [reload],
  );

  return { view, error, setEnabled };
}

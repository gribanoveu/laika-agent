import { useCallback, useEffect, useState } from "react";
import { setSkillEnabled, skillsList, type SkillsView } from "../lib/chat";

/**
 * The skills folder, re-read whenever the tab showing it opens: skills are
 * files the user edits outside the app, so a list read once at start-up
 * would go stale without anyone noticing.
 */
export function useSkills(visible: boolean) {
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
  }, [visible, reload]);

  const setEnabled = useCallback(
    async (name: string, enabled: boolean) => {
      // Shown at once; the reload that follows is what the file now says.
      setError(null);
      setView((v) => v && { ...v, skills: v.skills.map((s) => (s.name === name ? { ...s, enabled } : s)) });
      try {
        await setSkillEnabled(name, enabled);
      } catch (e) {
        setError(String(e));
      }
      await reload();
    },
    [reload],
  );

  return { view, error, setEnabled };
}

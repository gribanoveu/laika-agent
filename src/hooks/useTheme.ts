import { useEffect, useState } from "react";

export const THEMES = ["system", "dark", "light"] as const;
export type ThemePreference = (typeof THEMES)[number];

const KEY = "atlas-cli-theme";

/**
 * Resolves the preference to a concrete theme and writes it to <html
 * data-theme>, which is what src/styles/tokens.css keys its palettes off. A new
 * palette added there becomes selectable by adding its name to THEMES.
 */
export function useTheme() {
  const [preference, setPreference] = useState<ThemePreference>(
    () => (localStorage.getItem(KEY) as ThemePreference | null) ?? "system",
  );

  useEffect(() => {
    localStorage.setItem(KEY, preference);
    const mql = window.matchMedia("(prefers-color-scheme: light)");
    const apply = () => {
      document.documentElement.dataset.theme =
        preference === "system" ? (mql.matches ? "light" : "dark") : preference;
    };
    apply();
    if (preference !== "system") return;
    mql.addEventListener("change", apply);
    return () => mql.removeEventListener("change", apply);
  }, [preference]);

  return { preference, setPreference };
}

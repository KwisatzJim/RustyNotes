export type Theme = "light" | "dark";
export const THEME_KEY = "rustynotes.theme";

export function initialTheme(read: () => string | null, prefersDark: () => boolean): Theme {
  try {
    const saved = read();
    if (saved === "light" || saved === "dark") return saved;
  } catch { /* Storage may be unavailable; fall back to the system. */ }
  try { return prefersDark() ? "dark" : "light"; }
  catch { return "light"; }
}

export function oppositeTheme(theme: Theme): Theme {
  return theme === "dark" ? "light" : "dark";
}

export function saveTheme(theme: Theme, write: (value: Theme) => void): boolean {
  try { write(theme); return true; } catch { return false; }
}

export function initializeTheme(): Theme {
  const theme = initialTheme(() => localStorage.getItem(THEME_KEY),
    () => window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = theme;
  return theme;
}

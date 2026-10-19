/** Reader/app theme preference: follow the system, or force light/dark. */

export const THEME_PREFS = ["system", "light", "dark"] as const;
export type ThemePref = (typeof THEME_PREFS)[number];

export const THEME_LABELS: Record<ThemePref, string> = {
  system: "跟随系统",
  light: "浅色",
  dark: "深色",
};

export function isThemePref(v: string): v is ThemePref {
  return (THEME_PREFS as readonly string[]).includes(v);
}

function resolveTheme(
  pref: ThemePref,
  systemDark: boolean,
): "light" | "dark" {
  if (pref === "dark") return "dark";
  if (pref === "light") return "light";
  return systemDark ? "dark" : "light";
}

const QUERY = "(prefers-color-scheme: dark)";
let detach: (() => void) | null = null;

/** Apply the preference to `<html data-theme>`, tracking system changes. */
export function applyTheme(pref: ThemePref) {
  if (typeof window === "undefined") return;
  detach?.();
  detach = null;

  const mql = window.matchMedia(QUERY);
  const set = () => {
    document.documentElement.dataset.theme = resolveTheme(pref, mql.matches);
  };
  set();

  if (pref === "system") {
    mql.addEventListener("change", set);
    detach = () => mql.removeEventListener("change", set);
  }
}

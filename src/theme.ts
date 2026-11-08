// Appearance: dark (the original warm charcoal), light (the website hero's warm paper and
// charcoal ink), or the system's choice. The choice is a per-computer convenience in local
// storage; without storage the app follows the system.
import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { local } from "./api";

export type ThemeChoice = "system" | "light" | "dark";
export type Theme = "light" | "dark";

const KEY = "invectus.theme";
const CHANGE = "invectus-theme-change";
const dark = () => window.matchMedia?.("(prefers-color-scheme: dark)");

export function themeChoice(): ThemeChoice {
  const v = local.get<string>(KEY, "system");
  return v === "light" || v === "dark" ? v : "system";
}

export function resolveTheme(choice = themeChoice()): Theme {
  if (choice !== "system") return choice;
  return dark()?.matches === false ? "light" : "dark";
}

/** Set `data-theme` on <html>, and the native window's appearance (the macOS sidebar
 *  material follows it; `null` follows the system). */
function apply(choice: ThemeChoice) {
  const theme = resolveTheme(choice);
  if (document.documentElement.dataset.theme !== theme) {
    document.documentElement.dataset.theme = theme;
    window.dispatchEvent(new Event(CHANGE));
  }
  if ("__TAURI_INTERNALS__" in window) {
    getCurrentWindow().setTheme(choice === "system" ? null : choice).catch(() => undefined);
  }
}

export function setThemeChoice(choice: ThemeChoice) {
  local.set(KEY, choice === "system" ? null : choice);
  apply(choice);
}

/** Called once before the first render, so the first frame has the right colours. */
export function installTheme() {
  apply(themeChoice());
  dark()?.addEventListener("change", () => { if (themeChoice() === "system") apply("system"); });
}

/** The theme in effect, re-rendering when it changes. */
export function useTheme(): Theme {
  const [theme, setTheme] = useState<Theme>(() => (document.documentElement.dataset.theme as Theme) ?? resolveTheme());
  useEffect(() => {
    const update = () => setTheme(document.documentElement.dataset.theme === "light" ? "light" : "dark");
    window.addEventListener(CHANGE, update);
    return () => window.removeEventListener(CHANGE, update);
  }, []);
  return theme;
}

/** Colours for canvas drawing, read from the CSS tokens of the theme in effect. */
export function canvasColors() {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
  const inkRgb = v("--ink-rgb", "209, 203, 194");
  return {
    ink: v("--ink", "#d1cbc2"),
    accent: v("--accent", "#6edcaa"),
    bg: v("--bg", "#141414"),
    inkA: (a: number) => `rgba(${inkRgb}, ${a})`,
  };
}

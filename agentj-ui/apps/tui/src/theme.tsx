import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { palettes, type Palette } from "@agentj/tokens";

export type ThemeMode = "dark" | "light"; // TUI is two-state; system-follow is Web (C7)

export function nextTheme(mode: ThemeMode): ThemeMode {
  return mode === "dark" ? "light" : "dark";
}

// The palette is now the full semantic token record — 15 keys from @agentj/tokens:
// bg/sidebar/panel/raised/text/textMuted/accent/accentOnAccent/userBubble/border/codeBg/
// inlineCodeBg/success/error/warning. No live consumer read the old keys (only toggle was
// consumed), so they are deleted. Future readers wanting old semantics: background → bg,
// foreground → text.
export type ThemePalette = Palette;
export const darkPalette: ThemePalette = palettes.dark;
export const lightPalette: ThemePalette = palettes.light;

export function getThemeConfigDir(): string {
  const xdg = process.env.XDG_CONFIG_HOME;
  if (xdg && xdg.length > 0) return path.join(xdg, "agentj");
  try {
    return path.join(os.homedir(), ".config", "agentj");
  } catch {
    return path.join("/tmp", "agentj");
  }
}

export function getThemePath(): string {
  return path.join(getThemeConfigDir(), "theme.json");
}

export function loadTheme(): ThemeMode {
  try {
    const p = getThemePath();
    if (!fs.existsSync(p)) return "dark";
    const raw = fs.readFileSync(p, "utf8");
    const parsed: unknown = JSON.parse(raw);
    if (parsed && typeof parsed === "object") {
      const mode = (parsed as Record<string, unknown>).mode;
      if (mode === "dark" || mode === "light") return mode;
    }
    if (parsed === "dark" || parsed === "light") return parsed;
    return "dark";
  } catch {
    return "dark";
  }
}

export function saveTheme(mode: ThemeMode): void {
  try {
    const p = getThemePath();
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, JSON.stringify({ mode }, null, 2), "utf8");
  } catch {}
}

export type ThemeContextValue = {
  mode: ThemeMode;
  palette: ThemePalette;
  toggle: () => void;
  setMode: (m: ThemeMode) => void;
};

export const ThemeContext = createContext<ThemeContextValue | null>(null);

export function ThemeProvider(props: { children: React.ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(() => loadTheme());
  const setMode = useCallback((m: ThemeMode) => {
    setModeState(m);
    saveTheme(m);
  }, []);
  const toggle = useCallback(() => {
    setModeState((prev) => {
      const n = nextTheme(prev);
      saveTheme(n);
      return n;
    });
  }, []);
  // persist on mount in case file didn't exist
  useEffect(() => {
    saveTheme(mode);
  }, []);
  const palette = mode === "dark" ? darkPalette : lightPalette;
  const value = useMemo(() => ({ mode, palette, toggle, setMode }), [mode, palette, toggle, setMode]);
  return <ThemeContext.Provider value={value}>{props.children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const v = useContext(ThemeContext);
  if (!v) throw new Error("useTheme must be used within ThemeProvider");
  return v;
}

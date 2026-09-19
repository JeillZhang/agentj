// displayPrefsStore.ts — persisted per-zone display prefs (fonts, line height,
// density, reduced motion). Mirrors sidebarPrefsStore.ts recipe: readRaw/writeRaw/
// safeParse + field-level enum/boolean validation + malformed tolerance +
// same-tab CustomEvent + cross-tab storage listener. Stored as a single JSON
// object under DISPLAY_PREFS_KEY.

export type FontScale = "sm" | "md" | "lg" | "xl";
export type DisplayLineHeight = "compact" | "default" | "relaxed";
export type DisplayDensity = "default" | "compact";

export type DisplayPrefs = {
  fontConversation: FontScale;
  fontLeft: FontScale;
  fontRight: FontScale;
  fontCode: FontScale;
  lineHeight: DisplayLineHeight;
  density: DisplayDensity;
  reducedMotion: boolean;
};

export const DISPLAY_PREFS_KEY = "agentj.displayPrefs";
export const DISPLAY_PREFS_EVENT = "agentj:display-prefs-updated";
export const DISPLAY_PREFS_DEFAULTS: DisplayPrefs = {
  fontConversation: "md",
  fontLeft: "md",
  fontRight: "md",
  fontCode: "md",
  lineHeight: "default",
  density: "default",
  reducedMotion: false,
};

const FONT_SCALES: readonly FontScale[] = ["sm", "md", "lg", "xl"];
const LINE_HEIGHTS: readonly DisplayLineHeight[] = ["compact", "default", "relaxed"];
const DENSITIES: readonly DisplayDensity[] = ["default", "compact"];

function readRaw(): string | null {
  try {
    return localStorage.getItem(DISPLAY_PREFS_KEY);
  } catch {
    return null;
  }
}

function writeRaw(value: string): void {
  try {
    localStorage.setItem(DISPLAY_PREFS_KEY, value);
  } catch {}
}

function safeParse<T>(raw: string | null, fallback: T): T {
  if (raw == null) return fallback;
  try {
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

function isFontScale(value: unknown): value is FontScale {
  return typeof value === "string" && (FONT_SCALES as readonly string[]).includes(value);
}

function isLineHeight(value: unknown): value is DisplayLineHeight {
  return typeof value === "string" && (LINE_HEIGHTS as readonly string[]).includes(value);
}

function isDensity(value: unknown): value is DisplayDensity {
  return typeof value === "string" && (DENSITIES as readonly string[]).includes(value);
}

function isBoolean(value: unknown): value is boolean {
  return typeof value === "boolean";
}

/** Reads the persisted prefs; unknown/malformed fields fall back field-by-field. */
export function loadDisplayPrefs(): DisplayPrefs {
  const parsed = safeParse<unknown>(readRaw(), null);
  if (parsed == null || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { ...DISPLAY_PREFS_DEFAULTS };
  }
  const obj = parsed as Record<string, unknown>;
  return {
    fontConversation: isFontScale(obj.fontConversation)
      ? obj.fontConversation
      : DISPLAY_PREFS_DEFAULTS.fontConversation,
    fontLeft: isFontScale(obj.fontLeft) ? obj.fontLeft : DISPLAY_PREFS_DEFAULTS.fontLeft,
    fontRight: isFontScale(obj.fontRight) ? obj.fontRight : DISPLAY_PREFS_DEFAULTS.fontRight,
    fontCode: isFontScale(obj.fontCode) ? obj.fontCode : DISPLAY_PREFS_DEFAULTS.fontCode,
    lineHeight: isLineHeight(obj.lineHeight) ? obj.lineHeight : DISPLAY_PREFS_DEFAULTS.lineHeight,
    density: isDensity(obj.density) ? obj.density : DISPLAY_PREFS_DEFAULTS.density,
    reducedMotion: isBoolean(obj.reducedMotion)
      ? obj.reducedMotion
      : DISPLAY_PREFS_DEFAULTS.reducedMotion,
  };
}

/** Merges a patch into the stored prefs, persists it and notifies same-tab subscribers. */
export function saveDisplayPrefs(patch: Partial<DisplayPrefs>): DisplayPrefs {
  const current = loadDisplayPrefs();
  const next: DisplayPrefs = {
    fontConversation: isFontScale(patch.fontConversation)
      ? patch.fontConversation
      : current.fontConversation,
    fontLeft: isFontScale(patch.fontLeft) ? patch.fontLeft : current.fontLeft,
    fontRight: isFontScale(patch.fontRight) ? patch.fontRight : current.fontRight,
    fontCode: isFontScale(patch.fontCode) ? patch.fontCode : current.fontCode,
    lineHeight: isLineHeight(patch.lineHeight) ? patch.lineHeight : current.lineHeight,
    density: isDensity(patch.density) ? patch.density : current.density,
    reducedMotion: isBoolean(patch.reducedMotion) ? patch.reducedMotion : current.reducedMotion,
  };
  writeRaw(JSON.stringify(next));
  if (typeof window !== "undefined") {
    try {
      window.dispatchEvent(new CustomEvent<DisplayPrefs>(DISPLAY_PREFS_EVENT, { detail: next }));
    } catch {}
  }
  return next;
}

/** Subscribes to same-tab (`agentj:display-prefs-updated`) and cross-tab (`storage`) changes. */
export function subscribeDisplayPrefs(cb: (prefs: DisplayPrefs) => void): () => void {
  if (typeof window === "undefined") return () => {};
  const onUpdated = (): void => cb(loadDisplayPrefs());
  const onStorage = (event: StorageEvent): void => {
    if (event.key === DISPLAY_PREFS_KEY) cb(loadDisplayPrefs());
  };
  window.addEventListener(DISPLAY_PREFS_EVENT, onUpdated);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(DISPLAY_PREFS_EVENT, onUpdated);
    window.removeEventListener("storage", onStorage);
  };
}

// displayPrefsStore.ts — persisted per-zone display prefs (fonts, line height,
// density, reduced motion). Mirrors sidebarPrefsStore.ts recipe: readRaw/writeRaw/
// safeParse + field-level validation + malformed tolerance + same-tab CustomEvent
// + cross-tab storage listener. Stored as a single JSON object under
// DISPLAY_PREFS_KEY.
//
// Font sizes are pixel numbers (v0.17.2) validated/clamped against per-zone
// FONT_SIZE_LIMITS; the pre-v0.17.2 sm/md/lg/xl scale names migrate to the
// matching pixel value on load.

export type FontSizeKey = "fontConversation" | "fontLeft" | "fontRight" | "fontCode";
export type DisplayLineHeight = "compact" | "default" | "relaxed";
export type DisplayDensity = "default" | "compact";

export type FontSizeLimit = { min: number; max: number; default: number };

/** Per-zone pixel limits — single source for sliders, inputs and validation. */
export const FONT_SIZE_LIMITS: Record<FontSizeKey, FontSizeLimit> = {
  fontConversation: { min: 12, max: 24, default: 15 },
  fontLeft: { min: 10, max: 20, default: 13 },
  fontRight: { min: 10, max: 20, default: 13 },
  fontCode: { min: 10, max: 20, default: 12 },
};

export type DisplayPrefs = {
  fontConversation: number;
  fontLeft: number;
  fontRight: number;
  fontCode: number;
  lineHeight: DisplayLineHeight;
  density: DisplayDensity;
  reducedMotion: boolean;
};

export const DISPLAY_PREFS_KEY = "agentj.displayPrefs";
export const DISPLAY_PREFS_EVENT = "agentj:display-prefs-updated";
export const DISPLAY_PREFS_DEFAULTS: DisplayPrefs = {
  fontConversation: FONT_SIZE_LIMITS.fontConversation.default,
  fontLeft: FONT_SIZE_LIMITS.fontLeft.default,
  fontRight: FONT_SIZE_LIMITS.fontRight.default,
  fontCode: FONT_SIZE_LIMITS.fontCode.default,
  lineHeight: "default",
  density: "default",
  reducedMotion: false,
};

const FONT_SIZE_KEYS: readonly FontSizeKey[] = [
  "fontConversation",
  "fontLeft",
  "fontRight",
  "fontCode",
];

/** Pre-v0.17.2 scale names → px, preserving the old rendering per zone. */
const LEGACY_FONT_SCALES: Record<FontSizeKey, Record<string, number>> = {
  fontConversation: { sm: 13, md: 15, lg: 17, xl: 19 },
  fontLeft: { sm: 12, md: 13, lg: 15, xl: 16 },
  fontRight: { sm: 12, md: 13, lg: 15, xl: 16 },
  fontCode: { sm: 11, md: 12, lg: 14, xl: 15 },
};

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

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}

/** Numeric px clamps to the zone limits; legacy scale names migrate; else null. */
function normalizeFontSize(key: FontSizeKey, value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) {
    const { min, max } = FONT_SIZE_LIMITS[key];
    return clamp(Math.round(value), min, max);
  }
  if (
    typeof value === "string" &&
    Object.prototype.hasOwnProperty.call(LEGACY_FONT_SCALES[key], value)
  ) {
    return LEGACY_FONT_SCALES[key][value];
  }
  return null;
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
  const fonts = {} as Pick<DisplayPrefs, FontSizeKey>;
  for (const key of FONT_SIZE_KEYS) {
    fonts[key] = normalizeFontSize(key, obj[key]) ?? DISPLAY_PREFS_DEFAULTS[key];
  }
  return {
    ...fonts,
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
  const fonts = {} as Pick<DisplayPrefs, FontSizeKey>;
  for (const key of FONT_SIZE_KEYS) {
    fonts[key] = normalizeFontSize(key, patch[key]) ?? current[key];
  }
  const next: DisplayPrefs = {
    ...fonts,
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

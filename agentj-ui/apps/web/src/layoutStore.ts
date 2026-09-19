// layoutStore.ts — three-column workbench persistence (mirrors projectStore.ts patterns:
// safeParse + try/catch + defaults). Additive storage keys; existing keys keep their meaning.

export const LAYOUT_LIMITS = { leftMin: 200, leftMax: 420, rightMin: 260, rightMax: 480 } as const;

export type LeftTab = "sessions" | "files";
export type LayoutState = {
  leftWidth: number;
  rightWidth: number;
  leftTab: LeftTab;
  rightCollapsed: boolean;
};

export type ComposerAgent = "build" | "plan" | "explore";
export type ComposerPrefs = {
  model: string | null;
  agent: ComposerAgent;
  entropy: number;
};

export type RightSectionsState = { todo: boolean; changes: boolean };
export type RightSectionId = keyof RightSectionsState;

export const LAYOUT_DEFAULTS: LayoutState = {
  leftWidth: 256,
  rightWidth: 320,
  leftTab: "sessions",
  rightCollapsed: false,
};
export const COMPOSER_DEFAULTS: ComposerPrefs = { model: null, agent: "build", entropy: 0.7 };
export const RIGHT_SECTIONS_DEFAULTS: RightSectionsState = { todo: true, changes: true };

export const LAYOUT_KEYS = {
  leftWidth: "agentj.layout.leftWidth",
  rightWidth: "agentj.layout.rightWidth",
  leftTab: "agentj.layout.leftTab",
  rightCollapsed: "agentj.layout.rightCollapsed",
} as const;

export const COMPOSER_KEYS = {
  model: "agentj.composer.model",
  agent: "agentj.composer.agent",
  entropy: "agentj.composer.entropy",
} as const;

export const ASK_COLLAPSED_KEY = "agentj.ask.collapsed";
export const FAVORITES_KEY = "agentj.favorites";
export const RIGHT_SECTIONS_KEY = "agentj.layout.rightSections";
export const LAYOUT_UPDATED_EVENT = "agentj:layout-updated";

const LAYOUT_TABS: readonly LeftTab[] = ["sessions", "files"];
const COMPOSER_AGENTS: readonly ComposerAgent[] = ["build", "plan", "explore"];
const ENTROPY_MIN = 0;
const ENTROPY_MAX = 2;

function readRaw(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeRaw(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
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

function readNumber(key: string, fallback: number, min: number, max: number): number {
  const raw = readRaw(key);
  if (raw == null || raw.trim() === "") return fallback;
  const n = Number(raw);
  if (!Number.isFinite(n)) return fallback;
  return clamp(n, min, max);
}

function readEnum<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  const raw = readRaw(key);
  for (const value of allowed) {
    if (value === raw) return value;
  }
  return fallback;
}

function readBool(key: string, fallback: boolean): boolean {
  const raw = readRaw(key);
  if (raw === "1" || raw === "true") return true;
  if (raw === "0" || raw === "false") return false;
  return fallback;
}

function notifyLayoutUpdated(): void {
  if (typeof window === "undefined") return;
  try {
    window.dispatchEvent(new CustomEvent(LAYOUT_UPDATED_EVENT));
  } catch {}
}

export function loadLayout(): LayoutState {
  return {
    leftWidth: readNumber(
      LAYOUT_KEYS.leftWidth,
      LAYOUT_DEFAULTS.leftWidth,
      LAYOUT_LIMITS.leftMin,
      LAYOUT_LIMITS.leftMax,
    ),
    rightWidth: readNumber(
      LAYOUT_KEYS.rightWidth,
      LAYOUT_DEFAULTS.rightWidth,
      LAYOUT_LIMITS.rightMin,
      LAYOUT_LIMITS.rightMax,
    ),
    leftTab: readEnum(LAYOUT_KEYS.leftTab, LAYOUT_TABS, LAYOUT_DEFAULTS.leftTab),
    rightCollapsed: readBool(LAYOUT_KEYS.rightCollapsed, LAYOUT_DEFAULTS.rightCollapsed),
  };
}

export function saveLayout(patch: Partial<LayoutState>): void {
  const next: LayoutState = { ...loadLayout(), ...patch };
  writeRaw(LAYOUT_KEYS.leftWidth, String(next.leftWidth));
  writeRaw(LAYOUT_KEYS.rightWidth, String(next.rightWidth));
  writeRaw(LAYOUT_KEYS.leftTab, next.leftTab);
  writeRaw(LAYOUT_KEYS.rightCollapsed, next.rightCollapsed ? "1" : "0");
  notifyLayoutUpdated();
}

export function subscribeLayout(cb: (state: LayoutState) => void): () => void {
  if (typeof window === "undefined") return () => {};
  const onUpdated = (): void => cb(loadLayout());
  const onStorage = (event: StorageEvent): void => {
    if (event.key == null || event.key.startsWith("agentj.layout.")) cb(loadLayout());
  };
  window.addEventListener(LAYOUT_UPDATED_EVENT, onUpdated);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(LAYOUT_UPDATED_EVENT, onUpdated);
    window.removeEventListener("storage", onStorage);
  };
}

export function loadComposerPrefs(): ComposerPrefs {
  const model = readRaw(COMPOSER_KEYS.model);
  return {
    model: model == null || model === "" ? COMPOSER_DEFAULTS.model : model,
    agent: readEnum(COMPOSER_KEYS.agent, COMPOSER_AGENTS, COMPOSER_DEFAULTS.agent),
    entropy: readNumber(COMPOSER_KEYS.entropy, COMPOSER_DEFAULTS.entropy, ENTROPY_MIN, ENTROPY_MAX),
  };
}

export function saveComposerPrefs(patch: Partial<ComposerPrefs>): void {
  const next: ComposerPrefs = { ...loadComposerPrefs(), ...patch };
  writeRaw(COMPOSER_KEYS.model, next.model ?? "");
  writeRaw(COMPOSER_KEYS.agent, next.agent);
  writeRaw(COMPOSER_KEYS.entropy, String(next.entropy));
}

export function loadAskCollapsed(): boolean {
  return readRaw(ASK_COLLAPSED_KEY) === "1";
}

export function saveAskCollapsed(collapsed: boolean): void {
  writeRaw(ASK_COLLAPSED_KEY, collapsed ? "1" : "0");
}

export function loadFavorites(): string[] {
  const parsed = safeParse<unknown>(readRaw(FAVORITES_KEY), []);
  if (!Array.isArray(parsed)) return [];
  return parsed.filter((v): v is string => typeof v === "string");
}

export function saveFavorites(ids: string[]): void {
  writeRaw(FAVORITES_KEY, JSON.stringify(ids));
}

export function toggleFavorite(id: string): string[] {
  const ids = loadFavorites();
  const next = ids.includes(id) ? ids.filter((v) => v !== id) : [...ids, id];
  saveFavorites(next);
  return next;
}

export function loadRightSections(): RightSectionsState {
  const parsed = safeParse<unknown>(readRaw(RIGHT_SECTIONS_KEY), null);
  if (parsed == null || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { ...RIGHT_SECTIONS_DEFAULTS };
  }
  const obj = parsed as Record<string, unknown>;
  return {
    todo: typeof obj.todo === "boolean" ? obj.todo : RIGHT_SECTIONS_DEFAULTS.todo,
    changes: typeof obj.changes === "boolean" ? obj.changes : RIGHT_SECTIONS_DEFAULTS.changes,
  };
}

export function saveRightSections(state: RightSectionsState): void {
  writeRaw(RIGHT_SECTIONS_KEY, JSON.stringify(state));
}

export function setRightSection(id: RightSectionId, open: boolean): void {
  saveRightSections({ ...loadRightSections(), [id]: open });
}

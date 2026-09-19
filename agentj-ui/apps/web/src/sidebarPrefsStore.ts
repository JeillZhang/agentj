// sidebarPrefsStore.ts — persisted sidebar sort/filter prefs.
// Mirrors layoutStore.ts patterns: readRaw/writeRaw/safeParse + enum validation +
// malformed tolerance + same-tab CustomEvent + cross-tab storage listener.
// Stored as a single JSON object under SIDEBAR_PREFS_KEY.

export type SidebarSort = "updated" | "created";
export type SidebarFilter = "all" | "favorites";
export type SidebarPrefs = {
  sort: SidebarSort;
  filter: SidebarFilter;
};

export const SIDEBAR_PREFS_KEY = "agentj.sidebarPrefs";
export const SIDEBAR_PREFS_EVENT = "agentj:sidebar-prefs-updated";
export const SIDEBAR_PREFS_DEFAULTS: SidebarPrefs = { sort: "updated", filter: "all" };

const SORTS: readonly SidebarSort[] = ["updated", "created"];
const FILTERS: readonly SidebarFilter[] = ["all", "favorites"];

function readRaw(): string | null {
  try {
    return localStorage.getItem(SIDEBAR_PREFS_KEY);
  } catch {
    return null;
  }
}

function writeRaw(value: string): void {
  try {
    localStorage.setItem(SIDEBAR_PREFS_KEY, value);
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

function isSort(value: unknown): value is SidebarSort {
  return typeof value === "string" && (SORTS as readonly string[]).includes(value);
}

function isFilter(value: unknown): value is SidebarFilter {
  return typeof value === "string" && (FILTERS as readonly string[]).includes(value);
}

/** Reads the persisted prefs; unknown/malformed fields fall back field-by-field. */
export function loadSidebarPrefs(): SidebarPrefs {
  const parsed = safeParse<unknown>(readRaw(), null);
  if (parsed == null || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { ...SIDEBAR_PREFS_DEFAULTS };
  }
  const obj = parsed as Record<string, unknown>;
  return {
    sort: isSort(obj.sort) ? obj.sort : SIDEBAR_PREFS_DEFAULTS.sort,
    filter: isFilter(obj.filter) ? obj.filter : SIDEBAR_PREFS_DEFAULTS.filter,
  };
}

/** Merges a patch into the stored prefs, persists it and notifies same-tab subscribers. */
export function saveSidebarPrefs(patch: Partial<SidebarPrefs>): SidebarPrefs {
  const current = loadSidebarPrefs();
  const next: SidebarPrefs = {
    sort: isSort(patch.sort) ? patch.sort : current.sort,
    filter: isFilter(patch.filter) ? patch.filter : current.filter,
  };
  writeRaw(JSON.stringify(next));
  if (typeof window !== "undefined") {
    try {
      window.dispatchEvent(new CustomEvent<SidebarPrefs>(SIDEBAR_PREFS_EVENT, { detail: next }));
    } catch {}
  }
  return next;
}

/** Subscribes to same-tab (`agentj:sidebar-prefs-updated`) and cross-tab (`storage`) changes. */
export function subscribeSidebarPrefs(cb: (prefs: SidebarPrefs) => void): () => void {
  if (typeof window === "undefined") return () => {};
  const onUpdated = (): void => cb(loadSidebarPrefs());
  const onStorage = (event: StorageEvent): void => {
    if (event.key === SIDEBAR_PREFS_KEY) cb(loadSidebarPrefs());
  };
  window.addEventListener(SIDEBAR_PREFS_EVENT, onUpdated);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(SIDEBAR_PREFS_EVENT, onUpdated);
    window.removeEventListener("storage", onStorage);
  };
}

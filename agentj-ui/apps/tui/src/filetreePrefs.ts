import { readJsonFile, writeJsonFile } from "./config.ts";

// Per-project file tree expansion state, persisted as <configDir>/filetree.json.
// Contract (design G8): expanded = Record<projectKey, string[]> where projectKey
// is the local project id or "__default__" when the session has none. Paths are
// stored depth-first; save caps each key at 100 (deepest entries dropped) and
// prunes keys whose project no longer exists. Corrupt files are ignored.
export type FileTreePrefs = { expanded: Record<string, string[]> };

const FILE = "filetree.json";
export const DEFAULT_PROJECT_KEY = "__default__";
export const MAX_PATHS_PER_KEY = 100;

function sanitizeExpanded(value: unknown): Record<string, string[]> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return {};
  const out: Record<string, string[]> = {};
  for (const [key, paths] of Object.entries(value)) {
    if (key === "__proto__" || !Array.isArray(paths)) continue;
    out[key] = paths.filter((p): p is string => typeof p === "string");
  }
  return out;
}

// Missing, unreadable or corrupt file → empty state; never throws.
export function loadFileTreePrefs(): FileTreePrefs {
  const raw = readJsonFile<unknown>(FILE);
  const expanded = typeof raw === "object" && raw !== null ? (raw as { expanded?: unknown }).expanded : null;
  return { expanded: sanitizeExpanded(expanded) };
}

// Persists only live project keys plus "__default__", capping each key's path
// list at MAX_PATHS_PER_KEY (paths are depth-ordered, so the deepest fall off).
export function saveFileTreePrefs(prefs: FileTreePrefs, liveProjectIds: string[]): void {
  const live = new Set([...liveProjectIds, DEFAULT_PROJECT_KEY]);
  const expanded: Record<string, string[]> = {};
  for (const [key, paths] of Object.entries(prefs.expanded)) {
    if (!live.has(key) || key === "__proto__") continue;
    expanded[key] = paths.slice(0, MAX_PATHS_PER_KEY);
  }
  writeJsonFile(FILE, { expanded });
}

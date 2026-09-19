export type Project = {
  id: string;
  name: string;
  createdAt: number;
  workDir?: string | null;
  isDefault?: boolean;
  defaultModel?: string | null;
  defaultAgent?: string | null;
};

export type ProjectPatch = Partial<
  Pick<Project, "name" | "workDir" | "isDefault" | "defaultModel" | "defaultAgent">
>;

export const KEY_PROJECT_DEFAULTS = "agentj.projectDefaults";

export type ProjectDefaults = { workDir: string | null };

const KEY_PROJECTS = "agentj.projects";
const KEY_MAP = "agentj.projectMap";
const KEY_COLLAPSED = "agentj.projectCollapsed";
const KEY_CURRENT = "agentj.currentProject";

const DEFAULT_PROJECT: Project = { id: "default", name: "Inbox", createdAt: 0 };

function defaultProject(): Project {
  return { ...DEFAULT_PROJECT, createdAt: Date.now() };
}

function safeParse<T>(raw: string | null, fallback: T): T {
  if (raw == null) return fallback;
  try {
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

function persistProjects(list: Project[]): void {
  try {
    localStorage.setItem(KEY_PROJECTS, JSON.stringify(list));
  } catch {}
}

function notifyProjectsUpdated(): void {
  if (typeof window === "undefined") return;
  try {
    window.dispatchEvent(new CustomEvent("agentj:projects-updated"));
  } catch {}
}

function notifyCurrentProjectUpdated(id: string): void {
  if (typeof window === "undefined") return;
  try {
    window.dispatchEvent(new CustomEvent("agentj:current-project-updated", { detail: { id } }));
  } catch {}
}

export function loadProjects(): Project[] {
  const raw = (() => {
    try {
      return localStorage.getItem(KEY_PROJECTS);
    } catch {
      return null;
    }
  })();
  if (raw == null) {
    const def = [defaultProject()];
    persistProjects(def);
    return def;
  }
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed) || parsed.length === 0) {
      const def = [defaultProject()];
      persistProjects(def);
      return def;
    }
    const list = parsed.filter(
      (v): v is Project =>
        !!v && typeof (v as Project).id === "string" && typeof (v as Project).name === "string",
    );
    if (list.length === 0) {
      const def = [defaultProject()];
      persistProjects(def);
      return def;
    }
    // ensure default exists
    if (!list.some((p) => p.id === "default")) {
      list.unshift(defaultProject());
      persistProjects(list);
    }
    return list;
  } catch {
    const def = [defaultProject()];
    try {
      persistProjects(def);
    } catch {}
    return def;
  }
}

function validateName(name: string): string {
  const t = name.trim();
  if (!t) throw new Error("project name must be non-empty");
  return t;
}

function dedupName(base: string, existing: Set<string>): string {
  if (!existing.has(base)) return base;
  let i = 2;
  while (existing.has(`${base} (${i})`)) i += 1;
  return `${base} (${i})`;
}

function normalizeOptional(value: string | null | undefined): string | null {
  if (value == null) return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function slug(name: string): string {
  return `${name.toLowerCase().replace(/\W+/g, "-").replace(/^-|-$/g, "") || "project"}-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
}

export function createProject(name: string, workDir?: string): Project {
  const trimmed = validateName(name);
  const projects = loadProjects();
  const names = new Set(projects.map((p) => p.name));
  const finalName = dedupName(trimmed, names);
  const proj: Project = { id: slug(finalName), name: finalName, createdAt: Date.now() };
  if (workDir !== undefined) proj.workDir = normalizeOptional(workDir);
  projects.push(proj);
  persistProjects(projects);
  notifyProjectsUpdated();
  return proj;
}

export function updateProject(id: string, patch: ProjectPatch): Project | null {
  const projects = loadProjects();
  const idx = projects.findIndex((p) => p.id === id);
  if (idx === -1) return null;
  const next: Project = { ...projects[idx] };
  if (patch.name !== undefined) {
    const trimmed = validateName(patch.name);
    const otherNames = new Set(projects.filter((p) => p.id !== id).map((p) => p.name));
    next.name = dedupName(trimmed, otherNames);
  }
  if (patch.workDir !== undefined) next.workDir = normalizeOptional(patch.workDir);
  if (patch.defaultModel !== undefined) next.defaultModel = normalizeOptional(patch.defaultModel);
  if (patch.defaultAgent !== undefined) next.defaultAgent = normalizeOptional(patch.defaultAgent);
  if (patch.isDefault !== undefined) next.isDefault = patch.isDefault;

  let list = projects.map((p, i) => (i === idx ? next : p));
  if (patch.isDefault === true) {
    list = list.map((p) => (p.id === id ? p : { ...p, isDefault: false }));
  }
  persistProjects(list);
  notifyProjectsUpdated();
  return next;
}

export function renameProject(id: string, newName: string): void {
  updateProject(id, { name: newName });
}

export function deleteProject(id: string): { reassigned: number } | null {
  if (id === "default") return null;
  const projects = loadProjects();
  if (!projects.some((p) => p.id === id)) return null;

  persistProjects(projects.filter((p) => p.id !== id));

  const map = loadMapRaw();
  let reassigned = 0;
  for (const sid of Object.keys(map)) {
    if (map[sid] === id) {
      map[sid] = "default";
      reassigned += 1;
    }
  }
  try {
    localStorage.setItem(KEY_MAP, JSON.stringify(map));
  } catch {}

  const collapsed = loadCollapsed();
  if (id in collapsed) {
    delete collapsed[id];
    setCollapsed(collapsed);
  }

  let currentWasDeleted = false;
  try {
    currentWasDeleted = localStorage.getItem(KEY_CURRENT) === id;
  } catch {}
  if (currentWasDeleted) {
    const fallback = getDefaultProject().id;
    try {
      localStorage.setItem(KEY_CURRENT, fallback);
    } catch {}
    notifyCurrentProjectUpdated(fallback);
  }

  notifyProjectsUpdated();
  return { reassigned };
}

export function sessionCountForProject(id: string): number {
  const map = loadMapRaw();
  let count = 0;
  for (const pid of Object.values(map)) {
    if (pid === id) count += 1;
  }
  return count;
}

export function getDefaultProject(): Project {
  const projects = loadProjects();
  return projects.find((p) => p.isDefault) ?? projects[0] ?? defaultProject();
}

function loadMapRaw(): Record<string, string> {
  const raw = (() => {
    try {
      return localStorage.getItem(KEY_MAP);
    } catch {
      return null;
    }
  })();
  const parsed = safeParse<unknown>(raw, {});
  if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v === "string") out[k] = v;
    }
    return out;
  }
  return {};
}

export function assignSession(sessionId: string, projectId: string): void {
  const map = loadMapRaw();
  map[sessionId] = projectId;
  try {
    localStorage.setItem(KEY_MAP, JSON.stringify(map));
  } catch {}
}

export function getProjectForSession(sessionId: string): string {
  const map = loadMapRaw();
  const pid = map[sessionId];
  if (!pid) return "default";
  // stale fallback: if project no longer exists, return default
  try {
    const projects = loadProjects();
    if (projects.some((p) => p.id === pid)) return pid;
    return "default";
  } catch {
    return "default";
  }
}

export function loadCollapsed(): Record<string, boolean> {
  const raw = (() => {
    try {
      return localStorage.getItem(KEY_COLLAPSED);
    } catch {
      return null;
    }
  })();
  const parsed = safeParse<unknown>(raw, {});
  if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
    const out: Record<string, boolean> = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v === "boolean") out[k] = v;
    }
    return out;
  }
  return {};
}

export function setCollapsed(v: Record<string, boolean>): void {
  try {
    localStorage.setItem(KEY_COLLAPSED, JSON.stringify(v));
  } catch {}
}

export function loadCurrentProject(): string {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(KEY_CURRENT);
  } catch {
    return "default";
  }
  if (raw) {
    try {
      const projects = loadProjects();
      if (projects.some((p) => p.id === raw)) return raw;
    } catch {}
  }
  try {
    return getDefaultProject().id;
  } catch {
    return "default";
  }
}

export function setCurrentProject(id: string): void {
  try {
    localStorage.setItem(KEY_CURRENT, id);
  } catch {}
  notifyCurrentProjectUpdated(id);
}

export function loadProjectDefaults(): ProjectDefaults {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(KEY_PROJECT_DEFAULTS);
  } catch {
    return { workDir: null };
  }
  const parsed = safeParse<unknown>(raw, null);
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return { workDir: null };
  const workDir = (parsed as Record<string, unknown>).workDir;
  return { workDir: typeof workDir === "string" ? workDir : null };
}

export function saveProjectDefaults(patch: Partial<ProjectDefaults>): ProjectDefaults {
  const next: ProjectDefaults = { ...loadProjectDefaults(), ...patch };
  try {
    localStorage.setItem(KEY_PROJECT_DEFAULTS, JSON.stringify(next));
  } catch {}
  return next;
}

// helper to normalize timestamp to ms
function toMs(ts: unknown): number | null {
  if (ts == null || ts === "") return null;
  if (typeof ts === "number") {
    const n = ts;
    if (Number.isNaN(n)) return null;
    return n < 1e12 ? n * 1000 : n;
  }
  if (typeof ts === "string") {
    if (/^\d+$/.test(ts)) {
      const n = Number(ts);
      if (Number.isNaN(n)) return null;
      return n < 1e12 ? n * 1000 : n;
    }
    const parsed = Date.parse(ts);
    if (Number.isNaN(parsed)) return null;
    return parsed;
  }
  return null;
}

export type GroupedProject<T = unknown> = {
  project: Project;
  sessions: T[];
  subgroups: { Today: T[]; "Previous 7 days": T[]; Older: T[] };
};

export function groupSessionsByProject<T extends { id: string; updatedAt?: unknown; updated_at?: unknown; createdAt?: unknown; created_at?: unknown; timestamp?: unknown }>(
  sessions: T[],
): Record<string, GroupedProject<T>> {
  const projects = loadProjects();
  const byId = new Map(projects.map((p) => [p.id, p]));
  const groups: Record<string, GroupedProject<T>> = {};
  const ensure = (pid: string): GroupedProject<T> => {
    if (!groups[pid]) {
      const finalProj = byId.get(pid) ?? (pid === "default" ? defaultProject() : { id: pid, name: pid, createdAt: 0 });
      groups[pid] = { project: finalProj, sessions: [], subgroups: { Today: [], "Previous 7 days": [], Older: [] } };
      // ensure we store correct project for default even if fallback
      if (pid === "default" && !byId.has("default")) {
        groups[pid].project = defaultProject();
      } else if (byId.has(pid)) {
        groups[pid].project = byId.get(pid)!;
      }
    }
    return groups[pid];
  };

  // always ensure default exists so subgroups inspect works even with empty
  ensure("default");

  const now = Date.now();
  const DAY = 24 * 3600 * 1000;
  const WEEK = 7 * DAY;

  for (const s of sessions ?? []) {
    const pid = getProjectForSession(s.id);
    const g = ensure(pid);
    g.sessions.push(s);
    const rawTs = (s as unknown as Record<string, unknown>).updatedAt ??
      (s as unknown as Record<string, unknown>).updated_at ??
      (s as unknown as Record<string, unknown>).createdAt ??
      (s as unknown as Record<string, unknown>).created_at ??
      (s as unknown as Record<string, unknown>).timestamp ??
      null;
    const ms = toMs(rawTs);
    let bucket: keyof GroupedProject<T>["subgroups"] = "Today";
    if (ms == null) {
      bucket = "Today";
    } else {
      const diff = now - ms;
      if (diff < 0) bucket = "Today";
      else if (diff < DAY) bucket = "Today";
      else if (diff < WEEK) bucket = "Previous 7 days";
      else bucket = "Older";
    }
    g.subgroups[bucket].push(s);
  }

  // also subgroup non-default projects? apply same logic for consistency (only default required)
  // For assigned groups, sessions already subgrouped above if they were encountered via loop.
  // No extra needed.

  return groups;
}

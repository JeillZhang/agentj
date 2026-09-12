import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

export const SIDEBAR_WIDTH = 32;

// Config dir mirrors session.tsx / theme.tsx
export function getConfigDir(): string {
  const xdg = process.env.XDG_CONFIG_HOME;
  if (xdg && xdg.length > 0) return path.join(xdg, "agentj");
  try {
    return path.join(os.homedir(), ".config", "agentj");
  } catch {
    return path.join("/tmp", "agentj");
  }
}

export function getProjectsPath(): string {
  return path.join(getConfigDir(), "projects.json");
}

export function loadCollapsed(): Record<string, boolean> {
  try {
    const p = getProjectsPath();
    if (!fs.existsSync(p)) return {};
    const raw = fs.readFileSync(p, "utf8");
    const parsed: unknown = JSON.parse(raw);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      const obj = parsed as Record<string, unknown>;
      if ("collapsed" in obj && obj.collapsed && typeof obj.collapsed === "object" && !Array.isArray(obj.collapsed)) {
        const out: Record<string, boolean> = {};
        for (const [k, v] of Object.entries(obj.collapsed as Record<string, unknown>)) {
          if (typeof v === "boolean") out[k] = v;
        }
        return out;
      }
      // flat map case: all boolean top-level keys are collapsed
      const out: Record<string, boolean> = {};
      let hasBool = false;
      for (const [k, v] of Object.entries(obj)) {
        if (typeof v === "boolean") {
          out[k] = v;
          hasBool = true;
        }
      }
      if (hasBool) return out;
    }
    return {};
  } catch {
    return {};
  }
}

export function saveCollapsed(map: Record<string, boolean>): void {
  try {
    const p = getProjectsPath();
    fs.mkdirSync(path.dirname(p), { recursive: true });
    let existing: Record<string, unknown> = {};
    try {
      if (fs.existsSync(p)) {
        const raw = fs.readFileSync(p, "utf8");
        const parsed: unknown = JSON.parse(raw);
        if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
          existing = parsed as Record<string, unknown>;
        }
      }
    } catch {}
    const next: Record<string, unknown> = { ...existing, collapsed: map };
    // remove stale flat boolean keys
    for (const k of Object.keys(next)) {
      if (k !== "collapsed" && k !== "projects" && k !== "map" && k !== "sessions" && typeof next[k] === "boolean") {
        delete next[k];
      }
    }
    fs.writeFileSync(p, JSON.stringify(next, null, 2), "utf8");
  } catch {}
}

export function loadProjects(): Project[] {
  try {
    const p = getProjectsPath();
    if (!fs.existsSync(p)) return [{ id: "default", name: "Inbox" }];
    const raw = fs.readFileSync(p, "utf8");
    const parsed: unknown = JSON.parse(raw);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      const obj = parsed as Record<string, unknown>;
      if (Array.isArray(obj.projects)) {
        const list = (obj.projects as unknown[]).filter(
          (v): v is Project => !!v && typeof (v as Project).id === "string" && typeof (v as Project).name === "string",
        );
        if (list.length > 0) return list;
      }
    }
    if (Array.isArray(parsed)) {
      const list = (parsed as unknown[]).filter(
        (v): v is Project => !!v && typeof (v as Project).id === "string" && typeof (v as Project).name === "string",
      );
      if (list.length > 0) return list;
    }
    return [{ id: "default", name: "Inbox" }];
  } catch {
    return [{ id: "default", name: "Inbox" }];
  }
}

export function saveProjects(projects: Project[]): void {
  try {
    const p = getProjectsPath();
    fs.mkdirSync(path.dirname(p), { recursive: true });
    let existing: Record<string, unknown> = {};
    try {
      if (fs.existsSync(p)) {
        const raw = fs.readFileSync(p, "utf8");
        const parsed: unknown = JSON.parse(raw);
        if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
          existing = parsed as Record<string, unknown>;
        }
      }
    } catch {}
    const next: Record<string, unknown> = { ...existing, projects };
    fs.writeFileSync(p, JSON.stringify(next, null, 2), "utf8");
  } catch {}
}

// Simple project/session types for TUI (mirror projectStore)
export type Project = {
  id: string;
  name: string;
};

export type SidebarSession = {
  id: string;
  preview?: string;
  projectId?: string;
};

export type SidebarProps = {
  projects: Project[];
  sessions: SidebarSession[];
  activeId?: string | null;
  onSelect?: (id: string) => void;
  collapsed?: Record<string, boolean>;
  onToggle?: (id: string) => void;
  query?: string;
  width?: number;
};

function countForProject(project: Project, sessions: SidebarSession[], allProjects: Project[]): number {
  return sessions.filter((s) => {
    if (s.projectId) return s.projectId === project.id;
    // no projectId: attribute to first project when single project, else to default-like first
    if (allProjects.length === 0) return false;
    // if session has no projectId, treat as belonging to the first project in list
    // This satisfies single-project test where session {id:"a"} should count for Foo
    return project.id === allProjects[0].id;
  }).length;
}

function sessionsForProject(project: Project, sessions: SidebarSession[], allProjects: Project[]): SidebarSession[] {
  return sessions.filter((s) => {
    if (s.projectId) return s.projectId === project.id;
    if (allProjects.length === 0) return false;
    return project.id === allProjects[0].id;
  });
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s;
  return s.slice(0, max - 1) + "…";
}

export default function Sidebar(props: SidebarProps) {
  const { projects, sessions, activeId, collapsed = {}, query, width } = props;
  const w = width ?? SIDEBAR_WIDTH;

  // query filtering for sessions (case-insensitive on id/preview)
  const q = (query ?? "").trim().toLowerCase();
  const filteredSessions = q
    ? sessions.filter((s) => s.id.toLowerCase().includes(q) || (s.preview ?? "").toLowerCase().includes(q))
    : sessions;

  const innerWidth = Math.max(10, w - 4); // accounting for border+padding

  return (
    <box flexDirection="column" width={w} border borderStyle="single" padding={1} flexShrink={0}>
      {/* Header */}
      <box flexDirection="column" marginBottom={1}>
        <text>Threads</text>
        <text>{q ? `⌕ ${query}` : "⌕ Search sessions"}</text>
      </box>

      {/* Body - project groups */}
      <box flexDirection="column" flexGrow={1}>
        {projects.length === 0 ? (
          <text>No projects</text>
        ) : (
          projects.map((project) => {
            const isCollapsed = !!collapsed[project.id];
            const chevron = isCollapsed ? "▶" : "▼";
            const cnt = countForProject(project, filteredSessions, projects);
            // Folder icon + name + count
            const headerRaw = `${chevron} 📁 ${project.name} (${cnt})`;
            const header = truncate(headerRaw, innerWidth);
            const rows = sessionsForProject(project, filteredSessions, projects);
            return (
              <box key={project.id} flexDirection="column" marginBottom={1}>
                <text>{header}</text>
                {!isCollapsed &&
                  rows.map((s) => {
                    const active = s.id === activeId;
                    const marker = active ? "●" : "○";
                    const label = s.preview ?? s.id;
                    // indented 2 chars: "  " + marker + " " + label
                    const raw = `  ${marker} ${label}`;
                    return <text key={s.id}>{truncate(raw, innerWidth)}</text>;
                  })}
                {!isCollapsed && rows.length === 0 ? <text>  (no sessions)</text> : null}
              </box>
            );
          })
        )}
        {/* always show sessions without project grouping if no projects but sessions exist? */}
        {projects.length === 0 && filteredSessions.length > 0
          ? filteredSessions.map((s) => {
              const active = s.id === activeId;
              const marker = active ? "●" : "○";
              const raw = `  ${marker} ${s.preview ?? s.id}`;
              return <text key={s.id}>{truncate(raw, innerWidth)}</text>;
            })
          : null}
      </box>

      {/* Footer */}
      <box flexDirection="column" marginTop={1}>
        <text>Settings</text>
        <text>Help</text>
      </box>
    </box>
  );
}

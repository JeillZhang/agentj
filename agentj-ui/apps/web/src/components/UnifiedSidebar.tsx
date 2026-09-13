import { useCallback, useEffect, useMemo, useState } from "react";
import { ChevronDown, ChevronRight, Folder, FolderOpen, Search, Star } from "lucide-react";
import {
  METHODS,
  ResponseRouter,
  RpcClient,
  WebSocketTransport,
} from "@agentj/sdk";
import {
  assignSession,
  createProject,
  getProjectForSession,
  groupSessionsByProject,
  loadCollapsed,
  loadCurrentProject,
  loadProjects,
  setCollapsed,
  setCurrentProject,
} from "../projectStore";
import type { Project } from "../projectStore";
import { loadFavorites, loadLayout, saveLayout, toggleFavorite } from "../layoutStore";
import { DEMO_FILE_TREE, isDemoEnabled, subscribeDemo } from "../demoData";
import SidebarTabs from "./SidebarTabs";
import FileTree from "./FileTree";
import "./sidebar.css";
import "./session.css";

type ListRow = {
  id: string;
  preview?: string;
  state?: string;
  updatedAt?: string | number | null;
  createdAt?: string | number | null;
};

type SidebarSort = "updated" | "created";
type SidebarFilter = "all" | "favorites";

const SUBGROUP_TITLES: Array<"Today" | "Previous 7 days" | "Older"> = ["Today", "Previous 7 days", "Older"];
const VIRTUAL_PAGE = 50;

function toMs(ts: unknown): number | null {
  if (ts == null || ts === "") return null;
  if (typeof ts === "number") {
    if (Number.isNaN(ts)) return null;
    return ts < 1e12 ? ts * 1000 : ts;
  }
  if (typeof ts === "string") {
    if (/^\d+$/.test(ts)) {
      const n = Number(ts);
      return Number.isNaN(n) ? null : n < 1e12 ? n * 1000 : n;
    }
    const parsed = Date.parse(ts);
    return Number.isNaN(parsed) ? null : parsed;
  }
  return null;
}

/** `今天 10:42` / `昨天 18:20` / `09-11 16:03` per the mockup meta line. */
function formatTime(ts: unknown): string {
  const ms = toMs(ts);
  if (ms == null) return "";
  const d = new Date(ms);
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  const clock = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  if (d.toDateString() === now.toDateString()) return `今天 ${clock}`;
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (d.toDateString() === yesterday.toDateString()) return `昨天 ${clock}`;
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${clock}`;
}

function SessionListItem({
  row,
  activeSessionId,
  onContinue,
}: {
  row: ListRow;
  activeSessionId: string | null;
  onContinue: (id: string) => void;
}) {
  return (
    <li>
      <button
        type="button"
        tabIndex={0}
        aria-selected={row.id === activeSessionId}
        className={`aj-session-row${row.id === activeSessionId ? " aj-session-row--active" : ""}`}
        onClick={() => void onContinue(row.id)}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            void onContinue(row.id);
          }
        }}
      >
        <span className="aj-session-state" data-state={row.state} />
        <span className="aj-session-id">{row.preview ?? row.id}</span>
      </button>
    </li>
  );
}

function rowsFromList(sessions: Array<Record<string, unknown>>): ListRow[] {
  return (sessions ?? []).map((s) => {
    const sid = String((s as { session_id?: string; id?: string }).session_id ?? (s as { id?: string }).id ?? "");
    const preview = (s as { preview?: string | null }).preview;
    const state = (s as { state?: string }).state ?? "active";
    const updatedAt = ((s as Record<string, unknown>).updated_at ??
      (s as Record<string, unknown>).updatedAt ??
      (s as Record<string, unknown>).timestamp ??
      null) as string | number | null;
    const createdAt = ((s as Record<string, unknown>).created_at ??
      (s as Record<string, unknown>).createdAt ??
      null) as string | number | null;
    return {
      id: sid,
      state,
      preview: preview == null ? undefined : String(preview),
      updatedAt,
      createdAt,
    };
  });
}

function openOpClient(wsUrl: string) {
  const router = new ResponseRouter();
  const transport = new WebSocketTransport(wsUrl, router);
  const client = new RpcClient(transport as unknown as never, router as unknown as never);
  return { transport, client };
}

type Props = {
  wsUrl: string;
  client: unknown;
  activeSessionId: string | null;
  onActiveChange: (id: string) => void;
  unreachable?: boolean;
};

export default function UnifiedSidebar({ wsUrl, client, activeSessionId, onActiveChange, unreachable = false }: Props) {
  const [projects, setProjects] = useState<Project[]>(() => loadProjects());
  const [collapsed, setCollapsedState] = useState<Record<string, boolean>>(() => loadCollapsed());
  const [query, setQuery] = useState("");
  const [listRows, setListRows] = useState<ListRow[]>([]);
  const [favorites, setFavorites] = useState<string[]>(() => loadFavorites());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [currentProjectId, setCurrentProjectId] = useState<string>(() => loadCurrentProject());
  const [tab, setTab] = useState<"sessions" | "files">(() => loadLayout().leftTab);
  const [demo, setDemo] = useState<boolean>(() => isDemoEnabled());
  const [prefs, setPrefs] = useState<{ sort: SidebarSort; filter: SidebarFilter }>({
    sort: "updated",
    filter: "all",
  });
  const [visibleCounts, setVisibleCounts] = useState<Record<string, number>>({});
  const [flatVisible, setFlatVisible] = useState(VIRTUAL_PAGE);

  useEffect(() => subscribeDemo(setDemo), []);

  useEffect(() => {
    const handler = () => {
      try {
        setProjects(loadProjects());
        setCollapsedState(loadCollapsed());
        setFavorites(loadFavorites());
        setCurrentProjectId(loadCurrentProject());
      } catch {}
    };
    window.addEventListener("storage", handler);
    window.addEventListener("agentj:projects-updated", handler as EventListener);
    return () => {
      window.removeEventListener("storage", handler);
      window.removeEventListener("agentj:projects-updated", handler as EventListener);
    };
  }, []);

  useEffect(() => {
    const onPrefs = (event: Event) => {
      const detail = (event as CustomEvent<{ sort?: string; filter?: string }>).detail ?? {};
      setPrefs((prev) => {
        const next = { ...prev };
        if (detail.sort === "created" || detail.sort === "updated") next.sort = detail.sort;
        if (detail.filter === "all" || detail.filter === "favorites") next.filter = detail.filter;
        return next;
      });
    };
    window.addEventListener("agentj:sidebar-prefs", onPrefs as EventListener);
    return () => window.removeEventListener("agentj:sidebar-prefs", onPrefs as EventListener);
  }, []);

  useEffect(() => {
    const onAdd = () => {
      const name = window.prompt("Project name");
      if (name == null) return;
      const t = name.trim();
      if (!t) return;
      try {
        const p = createProject(t);
        setProjects(loadProjects());
        setCurrentProject(p.id);
        setCurrentProjectId(p.id);
      } catch {}
    };
    window.addEventListener("agentj:add-project", onAdd as EventListener);
    return () => window.removeEventListener("agentj:add-project", onAdd as EventListener);
  }, []);

  const handleNewSession = useCallback(
    async (targetProjectId: string) => {
      setBusy(true);
      setError(null);
      try {
        let sid = "";
        const rpc = client as RpcClient | null;
        if (rpc && typeof (rpc as unknown as { request?: unknown }).request === "function") {
          const res = (await (rpc as RpcClient).request(METHODS.sessionNew, {})) as { session_id: string };
          sid = res.session_id;
        } else {
          const { transport, client: tmp } = openOpClient(wsUrl);
          try {
            const res = (await tmp.request(METHODS.sessionNew, {})) as { session_id: string };
            sid = res.session_id;
          } finally {
            (transport as unknown as { close?: () => void }).close?.();
          }
        }
        if (sid) {
          assignSession(sid, targetProjectId);
          const current = loadFavorites();
          if (!current.includes(sid)) setFavorites(toggleFavorite(sid));
          onActiveChange(sid);
        }
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
      }
    },
    [wsUrl, client, onActiveChange],
  );

  // hero New session dispatch + legacy sidebar handlers
  useEffect(() => {
    const onNew = () => {
      void handleNewSession(currentProjectId);
    };
    window.addEventListener("agentj:new-session", onNew as EventListener);
    return () => window.removeEventListener("agentj:new-session", onNew as EventListener);
  }, [handleNewSession, currentProjectId]);

  // fetch session/list
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        let sessionsRaw: Array<Record<string, unknown>> = [];
        const rpc = client as RpcClient | null;
        if (rpc && typeof (rpc as unknown as { request?: unknown }).request === "function") {
          const res = (await (rpc as RpcClient).request(METHODS.sessionList, {})) as {
            sessions?: Array<Record<string, unknown>>;
          } | null;
          sessionsRaw = res?.sessions ?? [];
        } else {
          const { transport, client: tmp } = openOpClient(wsUrl);
          try {
            const res = (await tmp.request(METHODS.sessionList, {})) as {
              sessions?: Array<Record<string, unknown>>;
            } | null;
            sessionsRaw = res?.sessions ?? [];
          } finally {
            (transport as unknown as { close?: () => void }).close?.();
          }
        }
        if (!cancelled) setListRows(rowsFromList(sessionsRaw));
      } catch (err) {
        if (!cancelled) {
          console.debug("session-list", err);
          setListRows([]);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [wsUrl, client]);

  useEffect(() => {
    setFlatVisible(VIRTUAL_PAGE);
  }, [query, prefs]);

  const toggleCollapsed = useCallback((projectId: string) => {
    setCollapsedState((prev) => {
      const next = { ...prev, [projectId]: !prev[projectId] };
      setCollapsed(next);
      return next;
    });
  }, []);

  const handleContinue = useCallback(
    async (sid: string) => {
      const trimmed = sid.trim();
      if (!trimmed) return;
      const rpc = client as RpcClient | null;
      try {
        if (rpc && typeof (rpc as unknown as { request?: unknown }).request === "function") {
          await (rpc as RpcClient).request(METHODS.sessionStatus, { session_id: trimmed });
          await (rpc as RpcClient).request(METHODS.sessionSubscribe, { session_id: trimmed }).catch(() => {});
        } else {
          const { transport, client: tmp } = openOpClient(wsUrl);
          try {
            await tmp.request(METHODS.sessionStatus, { session_id: trimmed });
            await tmp.request(METHODS.sessionSubscribe, { session_id: trimmed }).catch(() => {});
          } finally {
            (transport as unknown as { close?: () => void }).close?.();
          }
        }
        setError(null);
        const current = loadFavorites();
        if (!current.includes(trimmed)) setFavorites(toggleFavorite(trimmed));
        onActiveChange(trimmed);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [wsUrl, client, onActiveChange],
  );

  const grouped = useMemo(() => {
    try {
      return groupSessionsByProject(listRows as never[]);
    } catch {
      return {} as Record<string, { project: Project; sessions: ListRow[]; subgroups: Record<string, ListRow[]> }>;
    }
  }, [listRows, projects]);

  const defaultSubgroupIndex = useMemo(() => {
    const g = (grouped as Record<string, { subgroups?: Record<string, ListRow[]> }>).default;
    const map = new Map<string, string>();
    for (const title of SUBGROUP_TITLES) {
      for (const r of g?.subgroups?.[title] ?? []) map.set(r.id, title);
    }
    return map;
  }, [grouped]);

  const projectsById = useMemo(() => new Map(projects.map((p) => [p.id, p])), [projects]);

  const metaFor = useCallback(
    (row: ListRow): string => {
      const name = projectsById.get(getProjectForSession(row.id))?.name ?? "";
      return [formatTime(row.updatedAt), name].filter(Boolean).join(" · ");
    },
    [projectsById],
  );

  const flatRows = useMemo(() => {
    const q = query.trim().toLowerCase();
    let rows = listRows.slice();
    if (q) {
      rows = rows.filter((r) => r.id.toLowerCase().includes(q) || (r.preview ?? "").toLowerCase().includes(q));
    }
    if (prefs.filter === "favorites") rows = rows.filter((r) => favorites.includes(r.id));
    const key = prefs.sort === "created" ? "createdAt" : "updatedAt";
    rows.sort((a, b) => (toMs(b[key]) ?? 0) - (toMs(a[key]) ?? 0));
    return rows;
  }, [listRows, query, prefs, favorites]);

  const visibleFlat = flatRows.slice(0, flatVisible);
  const hasMoreFlat = flatRows.length > visibleFlat.length;

  const filteredForProject = useCallback(
    (projectId: string): ListRow[] => {
      const g = (grouped as Record<string, { sessions: ListRow[] }>)[projectId];
      const rows = g?.sessions ?? [];
      const q = query.trim().toLowerCase();
      if (!q) return rows;
      return rows.filter((r) => r.id.toLowerCase().includes(q) || (r.preview ?? "").toLowerCase().includes(q));
    },
    [grouped, query],
  );

  const onAddProject = () => {
    window.dispatchEvent(new CustomEvent("agentj:add-project"));
  };

  const showMore = useCallback((projectId: string) => {
    setVisibleCounts((prev) => ({ ...prev, [projectId]: (prev[projectId] ?? VIRTUAL_PAGE) + VIRTUAL_PAGE }));
  }, []);

  const renderProjectGroups = () =>
    projects.map((project) => {
      const isCollapsed = !!collapsed[project.id];
      const rows = filteredForProject(project.id);
      const visible = visibleCounts[project.id] ?? VIRTUAL_PAGE;
      const sliced = rows.length > VIRTUAL_PAGE ? rows.slice(0, visible) : rows;
      const hasMore = rows.length > sliced.length;
      const count = rows.length;
      const bodyId = `aj-project-body-${project.id}`;
      return (
        <div key={project.id} className="aj-project-group">
          <button
            type="button"
            className="aj-project-row"
            aria-expanded={!isCollapsed}
            aria-controls={bodyId}
            aria-label={`${project.name} folder`}
            tabIndex={0}
            onClick={() => toggleCollapsed(project.id)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                toggleCollapsed(project.id);
              }
            }}
          >
            <span className="aj-project-row__chevron" aria-hidden="true">
              {isCollapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
            </span>
            <span className="aj-project-row__icon" aria-hidden="true">
              {isCollapsed ? <Folder size={16} /> : <FolderOpen size={16} />}
            </span>
            <span className="aj-project-row__name">{project.name}</span>
            <span className="aj-project-row__count">{count}</span>
          </button>
          {!isCollapsed && (
            <div className="aj-project-group__body" id={bodyId}>
              {rows.length === 0 ? (
                <div className="aj-project-empty">
                  <p className="aj-empty">No sessions yet</p>
                  <button
                    type="button"
                    className="aj-button aj-button--ghost"
                    onClick={() => void handleNewSession(project.id)}
                    disabled={busy}
                  >
                    New session
                  </button>
                </div>
              ) : (
                <>
                  {project.id === "default" ? (
                    <ul className="aj-session-list" aria-label="session list">
                      {SUBGROUP_TITLES.flatMap((title) => {
                        const bucketRows = sliced.filter((r) => (defaultSubgroupIndex.get(r.id) ?? "Today") === title);
                        if (bucketRows.length === 0) return [];
                        return [
                          <li key={`${title}-header`} className="aj-sidebar__subgroup">
                            <h4 className="aj-sidebar__subgroup-title">{title}</h4>
                          </li>,
                          ...bucketRows.map((r) => (
                            <SessionListItem
                              key={r.id}
                              row={r}
                              activeSessionId={activeSessionId}
                              onContinue={handleContinue}
                            />
                          )),
                        ];
                      })}
                    </ul>
                  ) : (
                    <ul aria-label={`sessions for ${project.name}`} className="aj-session-list">
                      {sliced.map((r) => (
                        <SessionListItem
                          key={r.id}
                          row={r}
                          activeSessionId={activeSessionId}
                          onContinue={handleContinue}
                        />
                      ))}
                    </ul>
                  )}
                  {hasMore && (
                    <button
                      type="button"
                      className="aj-button aj-button--ghost aj-project-more"
                      onClick={() => showMore(project.id)}
                    >
                      显示更多
                    </button>
                  )}
                  <button
                    type="button"
                    className="aj-button aj-button--ghost aj-project-new"
                    onClick={() => void handleNewSession(project.id)}
                    disabled={busy}
                  >
                    New session
                  </button>
                </>
              )}
            </div>
          )}
        </div>
      );
    });

  return (
    <section
      aria-label="sidebar"
      className={`aj-sidebar${unreachable ? " aj-sidebar--unreachable" : ""}`}
      id="aj-sidebar"
      data-unreachable={unreachable ? "true" : undefined}
    >
      <SidebarTabs
        tab={tab}
        onChange={(t) => {
          saveLayout({ leftTab: t });
          setTab(t);
        }}
      />
      {tab === "sessions" ? (
        <div className="aj-sidebar__body">
          <div className="aj-sidebar__search">
            <Search size={14} aria-hidden="true" />
            <input
              aria-label="搜索会话"
              placeholder="搜索会话…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <button
            type="button"
            className="btn-primary"
            onClick={() => void handleNewSession(currentProjectId)}
            disabled={busy}
          >
            + 新建会话
          </button>
          {error && (
            <div role="alert" className="aj-error">
              <p>{error}</p>
              <button
                type="button"
                className="aj-button--ghost"
                onClick={() => void handleNewSession(currentProjectId)}
              >
                New
              </button>
            </div>
          )}
          <ul className="aj-session-list" aria-label="最近会话">
            {visibleFlat.map((r) => (
              <li key={r.id}>
                <div
                  role="button"
                  tabIndex={0}
                  aria-selected={r.id === activeSessionId}
                  className={`aj-sess${r.id === activeSessionId ? " aj-sess--active" : ""}`}
                  onClick={() => void handleContinue(r.id)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      void handleContinue(r.id);
                    }
                  }}
                >
                  <span className="aj-sess__title">{r.preview ?? r.id}</span>
                  <span className="aj-sess__meta">{metaFor(r)}</span>
                  <button
                    type="button"
                    className="aj-star"
                    aria-label="收藏"
                    aria-pressed={favorites.includes(r.id)}
                    onClick={(e) => {
                      e.stopPropagation();
                      setFavorites(toggleFavorite(r.id));
                    }}
                  >
                    <Star size={14} aria-hidden="true" />
                  </button>
                </div>
              </li>
            ))}
          </ul>
          {hasMoreFlat && (
            <button
              type="button"
              className="aj-button aj-button--ghost aj-flat-more"
              onClick={() => setFlatVisible((v) => v + VIRTUAL_PAGE)}
            >
              显示更多
            </button>
          )}
          <div className="divider" />
          <div className="section-label">项目</div>
          {renderProjectGroups()}
          {projects.length === 0 && (
            <div className="aj-project-empty">
              <p className="aj-empty">No sessions yet</p>
              <button
                type="button"
                className="aj-button"
                onClick={() => void handleNewSession(currentProjectId)}
                disabled={busy}
              >
                New session
              </button>
            </div>
          )}
        </div>
      ) : (
        <div className="aj-sidebar__body" aria-label="文件">
          <FileTree nodes={demo ? DEMO_FILE_TREE : []} />
        </div>
      )}
      <div className="aj-sidebar__footer">
        <button type="button" onClick={onAddProject}>
          + 添加项目
        </button>
      </div>
    </section>
  );
}

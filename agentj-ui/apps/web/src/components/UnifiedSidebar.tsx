import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ArrowUpDown,
  ChevronDown,
  ChevronRight,
  Filter,
  Folder,
  FolderOpen,
  HelpCircle,
  LayoutGrid,
  Plus,
  Search,
  Settings,
  Star,
} from "lucide-react";
import {
  METHODS,
  ResponseRouter,
  RpcClient,
  WebSocketTransport,
} from "@agentj/sdk";
import {
  assignSession,
  createProject,
  groupSessionsByProject,
  loadCollapsed,
  loadCurrentProject,
  loadProjects,
  setCollapsed,
  setCurrentProject,
} from "../projectStore";
import type { Project } from "../projectStore";
import ConnectionBar from "../ConnectionBar";
import "./sidebar.css";
import "./session.css";

type ListRow = {
  id: string;
  preview?: string;
  state?: string;
  updatedAt?: string | number | null;
};

const SUBGROUP_TITLES: Array<"Today" | "Previous 7 days" | "Older"> = ["Today", "Previous 7 days", "Older"];

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
        <span className="aj-session-id">{row.id}</span>
        {row.preview !== undefined && <span className="aj-session-preview">{row.preview}</span>}
      </button>
    </li>
  );
}

function rowsFromList(
  sessions: Array<Record<string, unknown>>,
): ListRow[] {
  return (sessions ?? []).map((s) => {
    const sid = String((s as { session_id?: string; id?: string }).session_id ?? (s as { id?: string }).id ?? "");
    const preview = (s as { preview?: string | null }).preview;
    const state = (s as { state?: string }).state ?? "active";
    const updatedAt = ((s as Record<string, unknown>).updated_at ??
      (s as Record<string, unknown>).updatedAt ??
      (s as Record<string, unknown>).created_at ??
      (s as Record<string, unknown>).createdAt ??
      (s as Record<string, unknown>).timestamp ??
      null) as string | number | null;
    return {
      id: sid,
      state,
      preview: preview == null ? undefined : String(preview),
      updatedAt,
    };
  });
}

function loadFavorites(): string[] {
  try {
    const raw = localStorage.getItem("agentj.favorites");
    const arr = raw ? (JSON.parse(raw) as unknown) : [];
    return Array.isArray(arr) ? arr.filter((v) => typeof v === "string") : [];
  } catch {
    return [];
  }
}

function persistFavorites(ids: string[]): void {
  try {
    localStorage.setItem("agentj.favorites", JSON.stringify(ids));
  } catch {}
}

const STALE_KEY = "agentj.favorites.stale";
function loadStale(): Set<string> {
  try {
    const raw = localStorage.getItem(STALE_KEY);
    const arr = raw ? (JSON.parse(raw) as unknown) : [];
    if (!Array.isArray(arr)) return new Set();
    return new Set(arr.filter((v) => typeof v === "string"));
  } catch {
    return new Set();
  }
}
function persistStale(ids: Set<string>): void {
  try {
    localStorage.setItem(STALE_KEY, JSON.stringify([...ids]));
  } catch {}
}

function openOpClient(wsUrl: string) {
  const router = new ResponseRouter();
  const transport = new WebSocketTransport(wsUrl, router);
  const client = new RpcClient(transport as unknown as never, router as unknown as never);
  return { transport, client };
}

function ContinueField({
  activeSessionId,
  onContinue,
  onNew,
  busy,
}: {
  activeSessionId: string | null;
  onContinue: (id: string) => void;
  onNew: () => void;
  busy: boolean;
}) {
  const [input, setInput] = useState(activeSessionId ?? "");
  useEffect(() => {
    if (activeSessionId) setInput(activeSessionId);
  }, [activeSessionId]);
  return (
    <>
      <div className="aj-session-actions">
        <button type="button" className="aj-button" onClick={onNew} disabled={busy}>
          New session
        </button>
      </div>
      <div className="aj-field">
        <label htmlFor="aj-session-id" className="aj-label">
          session id
        </label>
        <input
          id="aj-session-id"
          className="aj-input"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="session id"
        />
        <button
          type="button"
          className="aj-button aj-button--ghost"
          onClick={() => onContinue(input.trim())}
          disabled={busy}
        >
          Continue
        </button>
      </div>
    </>
  );
}

type Props = {
  wsUrl: string;
  client: unknown;
  activeSessionId: string | null;
  onActiveChange: (id: string) => void;
  onConnect?: (host: string, port: string, token: string) => void;
  unreachable?: boolean;
};

const VIRTUAL_PAGE = 50;

export default function UnifiedSidebar({ wsUrl, client, activeSessionId, onActiveChange, onConnect, unreachable = false }: Props) {
  const [projects, setProjects] = useState<Project[]>(() => loadProjects());
  const [collapsed, setCollapsedState] = useState<Record<string, boolean>>(() => loadCollapsed());
  const [query, setQuery] = useState("");
  const [listRows, setListRows] = useState<ListRow[]>([]);
  const [favorites, setFavorites] = useState<string[]>(() => loadFavorites());
  const [stale, setStale] = useState<Set<string>>(() => loadStale());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [currentProjectId, setCurrentProjectId] = useState<string>(() => loadCurrentProject());

  useEffect(() => {
    const handler = () => {
      try {
        setProjects(loadProjects());
        setCollapsedState(loadCollapsed());
        setFavorites(loadFavorites());
        setStale(loadStale());
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

  // handle hero New session dispatch
  useEffect(() => {
    const onNew = () => {
      void handleNewSession(currentProjectId);
    };
    window.addEventListener("agentj:new-session", onNew as EventListener);
    return () => window.removeEventListener("agentj:new-session", onNew as EventListener);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wsUrl, client, currentProjectId]);

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

  const toggleCollapsed = useCallback((projectId: string) => {
    setCollapsedState((prev) => {
      const next = { ...prev, [projectId]: !prev[projectId] };
      setCollapsed(next);
      return next;
    });
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
          setFavorites((prev) => {
            if (prev.includes(sid)) return prev;
            const next = [...prev, sid];
            persistFavorites(next);
            return next;
          });
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
        setFavorites((prev) => {
          if (prev.includes(trimmed)) return prev;
          const next = [...prev, trimmed];
          persistFavorites(next);
          return next;
        });
        setStale((prev) => {
          if (!prev.has(trimmed)) return prev;
          const next = new Set(prev);
          next.delete(trimmed);
          persistStale(next);
          return next;
        });
        onActiveChange(trimmed);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setError(msg);
        if (favorites.includes(trimmed)) {
          setStale((prev) => {
            if (prev.has(trimmed)) return prev;
            const next = new Set(prev);
            next.add(trimmed);
            persistStale(next);
            return next;
          });
        }
      }
    },
    [wsUrl, client, onActiveChange, favorites],
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

  const favoriteFiltered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return favorites;
    return favorites.filter((id) => id.toLowerCase().includes(q));
  }, [favorites, query]);

  const [visibleCounts, setVisibleCounts] = useState<Record<string, number>>({});

  const onAddProject = () => {
    window.dispatchEvent(new CustomEvent("agentj:add-project"));
  };

  const showMore = useCallback((projectId: string) => {
    setVisibleCounts((prev) => ({ ...prev, [projectId]: (prev[projectId] ?? VIRTUAL_PAGE) + VIRTUAL_PAGE }));
  }, []);

  return (
    <section
      aria-label="sidebar"
      className={`aj-sidebar${unreachable ? " aj-sidebar--unreachable" : ""}`}
      id="aj-sidebar"
      data-unreachable={unreachable ? "true" : undefined}
    >
      <div className="aj-sidebar__threads">
        <h2>Threads</h2>
        <div className="aj-sidebar__toolbar">
          <button type="button" aria-label="Filter">
            <Filter size={16} aria-hidden="true" />
          </button>
          <button type="button" aria-label="Sort">
            <ArrowUpDown size={16} aria-hidden="true" />
          </button>
          <button type="button" aria-label="View">
            <LayoutGrid size={16} aria-hidden="true" />
          </button>
          <button type="button" aria-label="Add Project" onClick={onAddProject}>
            <Plus size={16} aria-hidden="true" /> Add Project
          </button>
        </div>
        <div className="aj-sidebar__search">
          <Search size={16} aria-hidden="true" />
          <input aria-label="Search sessions" placeholder="Search sessions" value={query} onChange={(e) => setQuery(e.target.value)} />
        </div>
      </div>
      <div className="aj-sidebar__body">
        {onConnect && (
          <div className="aj-sidebar__connection">
            <ConnectionBar onConnect={onConnect} />
          </div>
        )}
        <ContinueField
          activeSessionId={activeSessionId}
          onContinue={(id) => void handleContinue(id)}
          onNew={() => void handleNewSession(currentProjectId)}
          busy={busy}
        />
        {error && (
          <div role="alert" className="aj-error" style={{ fontSize: 12 }}>
            <p>{error}</p>
            <button type="button" className="aj-button--ghost" onClick={() => void handleNewSession(currentProjectId)}>
              New
            </button>
          </div>
        )}
        {activeSessionId && (
          <p className="aj-chip aj-chip--model" aria-label="active session">
            active: {activeSessionId}
          </p>
        )}
        {favorites.length > 0 && (
          <div className="aj-project-group">
            <h3 className="aj-project-group__header">
              <Star size={14} aria-hidden="true" /> Favorites
            </h3>
            <ul aria-label="favorites" className="aj-session-list">
              {favoriteFiltered.map((id) => (
                <li key={id}>
                  <button
                    type="button"
                    tabIndex={0}
                    aria-selected={id === activeSessionId}
                    className={`aj-session-row${stale.has(id) ? " aj-session-row--stale" : ""}${id === activeSessionId ? " aj-session-row--active" : ""}`}
                    onClick={() => void handleContinue(id)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        void handleContinue(id);
                      }
                    }}
                  >
                    {stale.has(id) ? `${id} (stale)` : id}
                  </button>
                </li>
              ))}
            </ul>
          </div>
        )}
        {/* Project groups */}
        {projects.map((project) => {
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
                      <button type="button" className="aj-button aj-button--ghost" onClick={() => void handleNewSession(project.id)} disabled={busy}>
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
                                <SessionListItem key={r.id} row={r} activeSessionId={activeSessionId} onContinue={handleContinue} />
                              )),
                            ];
                          })}
                        </ul>
                      ) : (
                        <ul aria-label={`sessions for ${project.name}`} className="aj-session-list">
                          {sliced.map((r) => (
                            <SessionListItem key={r.id} row={r} activeSessionId={activeSessionId} onContinue={handleContinue} />
                          ))}
                        </ul>
                      )}
                      {hasMore && (
                        <button type="button" className="aj-button aj-button--ghost aj-project-more" onClick={() => showMore(project.id)}>
                          Show more ({rows.length - sliced.length} more)
                        </button>
                      )}
                      <button type="button" className="aj-button aj-button--ghost aj-project-new" onClick={() => void handleNewSession(project.id)} disabled={busy}>
                        New session
                      </button>
                    </>
                  )}
                </div>
              )}
            </div>
          );
        })}
        {projects.length === 0 && (
          <div className="aj-project-empty">
            <p className="aj-empty">No sessions yet</p>
            <button type="button" className="aj-button" onClick={() => void handleNewSession(currentProjectId)} disabled={busy}>
              New session
            </button>
          </div>
        )}
      </div>
      <div className="aj-sidebar__footer">
        <button type="button" aria-label="Settings">
          <Settings size={16} aria-hidden="true" /> Settings
        </button>
        <button type="button" aria-label="Help">
          <HelpCircle size={16} aria-hidden="true" /> Help
        </button>
      </div>
    </section>
  );
}

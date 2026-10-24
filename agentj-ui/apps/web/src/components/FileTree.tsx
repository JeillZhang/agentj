import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import { ChevronRight, Folder, FolderOpen } from "lucide-react";
import { METHODS, parentPath, toGitLetter } from "@agentj/sdk";
import type { GitLetter, RpcClient } from "@agentj/sdk";
import { DEMO_FILE_TREE } from "../demoData";
import type { FileNode } from "../demoData";
import { fileIconFor } from "../fileIcons";
import { moveSelection, tabIndexFor } from "../hooks/useRovingFocus";
import { visiblePaths } from "./fileTreeNav";
import "./filetree.css";

export type FileTreeProps = {
  client: RpcClient | null;
  sessionId: string | null;
  demo: boolean;
  onFileSelect?: (path: string) => void;
};

/** Imperative surface consumed by the sidebar shortcut scope (`r` refresh, `j/k` move). */
export type FileTreeHandle = {
  /** Move the selection by `delta` visible rows (clamped) and move focus along. */
  moveBy: (delta: number) => void;
  /** Reload the root directory and every currently expanded directory. */
  refresh: () => void;
};

const ROOT = "";

/** One rendered row; `git` is the shared `toGitLetter` normalization of `git_status`. */
type ViewEntry = {
  name: string;
  path: string;
  kind: "file" | "dir";
  git: GitLetter | null;
};

type RawFileEntry = {
  name?: unknown;
  path?: unknown;
  kind?: unknown;
  git_status?: unknown;
};

type FileListReply = { entries?: RawFileEntry[] | null } | null;

type FileTreeStats = {
  files: number;
  changes: number;
};

function toView(entry: RawFileEntry): ViewEntry {
  return {
    name: String(entry.name ?? ""),
    path: String(entry.path ?? ""),
    kind: entry.kind === "dir" ? "dir" : "file",
    // Wire `git_status` is untyped JSON: narrow to string before the shared letter map.
    git: toGitLetter(typeof entry.git_status === "string" ? entry.git_status : null),
  };
}

/** Flattens the nested demo fixture into the same parent-path map the fetched tree uses. */
function flattenDemo(nodes: FileNode[]): Map<string, ViewEntry[]> {
  const map = new Map<string, ViewEntry[]>();
  const walk = (parent: string, list: FileNode[]): void => {
    map.set(
      parent,
      list.map((n) => ({ name: n.name, path: n.path, kind: n.kind, git: n.git ?? null })),
    );
    for (const n of list) {
      if (n.kind === "dir" && n.children) walk(n.path, n.children);
    }
  };
  walk(ROOT, nodes);
  return map;
}

/** Counts files and tracked changes over the loaded map; U stays out of the change count. */
function collectStats(entries: Map<string, ViewEntry[]>): FileTreeStats {
  const stats: FileTreeStats = { files: 0, changes: 0 };
  const walk = (parent: string): void => {
    for (const entry of entries.get(parent) ?? []) {
      if (entry.kind === "dir") {
        walk(entry.path);
        continue;
      }
      stats.files += 1;
      if (entry.git && entry.git !== "U") stats.changes += 1;
    }
  };
  walk(ROOT);
  return stats;
}

function childGroupId(path: string): string {
  return `filetree-${path}-children`;
}

/** Row left inset grows linearly with depth so arbitrarily deep trees stay readable. */
function rowIndent(depth: number): number {
  return depth * 14 + 2;
}

type NodeListProps = {
  entries: Map<string, ViewEntry[]>;
  parent: string;
  depth: number;
  expanded: Set<string>;
  loading: Set<string>;
  errors: Map<string, string>;
  selectedPath: string | null;
  tabbableIndex: number;
  indexByPath: Map<string, number>;
  onToggle: (path: string) => void;
  onRetry: (path: string) => void;
  onSelect: (path: string) => void;
  onFileSelect?: (path: string) => void;
  registerRow: (path: string, el: HTMLElement | null) => void;
};

function StatusCell({ git }: { git: GitLetter }) {
  return <span className={`status-txt status-${git}`}>{git}</span>;
}

/** Renders one loaded level; dirs recurse only while expanded and drive their own load state. */
function NodeList({
  entries,
  parent,
  depth,
  expanded,
  loading,
  errors,
  selectedPath,
  tabbableIndex,
  indexByPath,
  onToggle,
  onRetry,
  onSelect,
  onFileSelect,
  registerRow,
}: NodeListProps) {
  return (
    <>
      {(entries.get(parent) ?? []).map((node) => {
        const isSelected = node.path === selectedPath;
        const rowStyle: CSSProperties = { paddingLeft: rowIndent(depth) };
        if (isSelected) rowStyle.background = "var(--aj-raised)";
        const tabIndex = tabIndexFor(indexByPath.get(node.path) ?? -1, tabbableIndex);
        if (node.kind === "dir") {
          const open = expanded.has(node.path);
          const groupId = childGroupId(node.path);
          const children = entries.get(node.path);
          let body: ReactNode = null;
          if (open) {
            if (loading.has(node.path)) body = <p className="aj-empty">加载中…</p>;
            else if (errors.has(node.path)) {
              body = (
                <div className="aj-empty">
                  <span>加载失败</span>
                  <button type="button" className="aj-button--ghost" onClick={() => onRetry(node.path)}>
                    重试
                  </button>
                </div>
              );
            } else if (children && children.length === 0) body = <p className="aj-empty">空目录</p>;
            else if (children) {
              body = (
                <NodeList
                  entries={entries}
                  parent={node.path}
                  depth={depth + 1}
                  expanded={expanded}
                  loading={loading}
                  errors={errors}
                  selectedPath={selectedPath}
                  tabbableIndex={tabbableIndex}
                  indexByPath={indexByPath}
                  onToggle={onToggle}
                  onRetry={onRetry}
                  onSelect={onSelect}
                  onFileSelect={onFileSelect}
                  registerRow={registerRow}
                />
              );
            }
          }
          return (
            <div className="trow-group" role="none" key={node.path}>
              <button
                type="button"
                role="treeitem"
                className="trow trow-dir"
                style={rowStyle}
                aria-expanded={open}
                aria-selected={isSelected}
                aria-controls={groupId}
                tabIndex={tabIndex}
                data-selected={isSelected ? "true" : undefined}
                ref={(el) => {
                  registerRow(node.path, el);
                }}
                onClick={() => onToggle(node.path)}
                onFocus={() => onSelect(node.path)}
              >
                <ChevronRight className="chev" size={12} aria-hidden="true" />
                {open ? (
                  <FolderOpen className="dir-icon dir-icon--open" size={13} aria-hidden="true" />
                ) : (
                  <Folder className="dir-icon dir-icon--closed" size={13} aria-hidden="true" />
                )}
                <span className="name">{node.name}</span>
                {node.git ? <StatusCell git={node.git} /> : null}
              </button>
              <div id={groupId} role="group">
                {body}
              </div>
            </div>
          );
        }
        const { Icon, category } = fileIconFor(node.name);
        const handleFileClick = () => {
          onSelect(node.path);
          onFileSelect?.(node.path);
        };
        return (
          <div
            className="trow"
            role="treeitem"
            key={node.path}
            style={rowStyle}
            aria-selected={isSelected}
            tabIndex={tabIndex}
            data-selected={isSelected ? "true" : undefined}
            data-path={node.path}
            ref={(el) => {
              registerRow(node.path, el);
            }}
            onFocus={() => onSelect(node.path)}
            onClick={handleFileClick}
          >
            <span className="chev" aria-hidden="true" />
            <Icon className={`file-icon file-icon--${category}`} size={13} aria-hidden="true" />
            <span className="name">{node.name}</span>
            {node.git ? <StatusCell git={node.git} /> : null}
          </div>
        );
      })}
    </>
  );
}

/**
 * Left-panel 文件 tab body: lazily loads directories via `file/list`, caches children,
 * renders git markers and footer stats. Rows form a roving-tabindex tree: arrows move
 * the selection with focus, Home/End jump, Enter/Space toggle dirs and ← jumps to the
 * parent dir. Demo mode uses fixtures with zero requests.
 */
const FileTree = forwardRef<FileTreeHandle, FileTreeProps>(function FileTree(
  { client, sessionId, demo, onFileSelect }: FileTreeProps,
  ref,
) {
  const [fetched, setFetched] = useState<Map<string, ViewEntry[]>>(new Map());
  const [loading, setLoading] = useState<Set<string>>(new Set());
  const [errors, setErrors] = useState<Map<string, string>>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const rowRefs = useRef(new Map<string, HTMLElement>());

  const demoEntries = useMemo(() => flattenDemo(DEMO_FILE_TREE), []);
  const entries = demo ? demoEntries : fetched;

  const loadDir = useCallback(
    async (path: string): Promise<void> => {
      if (!client || !sessionId) return;
      setLoading((prev) => new Set(prev).add(path));
      setErrors((prev) => {
        const next = new Map(prev);
        next.delete(path);
        return next;
      });
      try {
        const reply = (await client.request(METHODS.fileList, {
          session_id: sessionId,
          ...(path ? { path } : {}),
          depth: 1,
        })) as FileListReply;
        const list = (reply?.entries ?? []).map(toView);
        setFetched((prev) => new Map(prev).set(path, list));
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        setErrors((prev) => new Map(prev).set(path, message));
      } finally {
        setLoading((prev) => {
          const next = new Set(prev);
          next.delete(path);
          return next;
        });
      }
    },
    [client, sessionId],
  );

  // Root load on mount / session change; demo mode resets to fixtures with no request.
  useEffect(() => {
    setFetched(new Map());
    setLoading(new Set());
    setErrors(new Map());
    setExpanded(new Set());
    setSelectedPath(null);
    if (demo || !client || !sessionId) return;
    void loadDir(ROOT);
  }, [demo, client, sessionId, loadDir]);

  const toggle = useCallback(
    (path: string): void => {
      setExpanded((prev) => {
        const next = new Set(prev);
        if (next.has(path)) next.delete(path);
        else next.add(path);
        return next;
      });
      if (!demo && !fetched.has(path) && !loading.has(path)) void loadDir(path);
    },
    [demo, fetched, loading, loadDir],
  );

  const paths = useMemo(() => visiblePaths(entries, expanded), [entries, expanded]);
  const indexByPath = useMemo(() => new Map(paths.map((path, index) => [path, index])), [paths]);
  const entryByPath = useMemo(() => {
    const map = new Map<string, ViewEntry>();
    for (const list of entries.values()) for (const entry of list) map.set(entry.path, entry);
    return map;
  }, [entries]);

  // Drop a selection whose row disappeared (e.g. a refreshed tree no longer lists it).
  useEffect(() => {
    if (selectedPath && !indexByPath.has(selectedPath)) setSelectedPath(null);
  }, [selectedPath, indexByPath]);

  const registerRow = useCallback((path: string, el: HTMLElement | null): void => {
    if (el) rowRefs.current.set(path, el);
    else rowRefs.current.delete(path);
  }, []);

  const selectPath = useCallback((path: string): void => {
    setSelectedPath(path);
    rowRefs.current.get(path)?.focus();
  }, []);

  const moveBy = useCallback(
    (delta: number): void => {
      if (paths.length === 0) return;
      const current = selectedPath ? (indexByPath.get(selectedPath) ?? -1) : -1;
      selectPath(paths[moveSelection(current, delta, paths.length)]);
    },
    [paths, indexByPath, selectedPath, selectPath],
  );

  const refresh = useCallback((): void => {
    if (demo || !client || !sessionId) return;
    void loadDir(ROOT);
    for (const path of expanded) void loadDir(path);
  }, [demo, client, sessionId, expanded, loadDir]);

  useImperativeHandle(ref, () => ({ moveBy, refresh }), [moveBy, refresh]);

  const handleTreeKey = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>): void => {
      if (paths.length === 0) return;
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        moveBy(event.key === "ArrowDown" ? 1 : -1);
        return;
      }
      if (event.key === "Home") {
        event.preventDefault();
        selectPath(paths[0]);
        return;
      }
      if (event.key === "End") {
        event.preventDefault();
        selectPath(paths[paths.length - 1]);
        return;
      }
      if (!selectedPath) return;
      const entry = entryByPath.get(selectedPath);
      if (event.key === "Enter" || event.key === " ") {
        if (entry?.kind === "dir") {
          event.preventDefault();
          toggle(selectedPath);
        }
        return;
      }
      if (event.key === "ArrowRight") {
        if (entry?.kind === "dir" && !expanded.has(selectedPath)) {
          event.preventDefault();
          toggle(selectedPath);
        }
        return;
      }
      if (event.key === "ArrowLeft") {
        event.preventDefault();
        if (entry?.kind === "dir" && expanded.has(selectedPath)) {
          toggle(selectedPath);
          return;
        }
        const parent = parentPath(selectedPath);
        if (parent !== null && indexByPath.has(parent)) selectPath(parent);
      }
    },
    [paths, selectedPath, entryByPath, expanded, moveBy, selectPath, toggle, indexByPath],
  );

  const stats = collectStats(entries);
  const rootEntries = entries.get(ROOT);
  const selectedIndex = selectedPath ? (indexByPath.get(selectedPath) ?? -1) : -1;
  const tabbableIndex = selectedIndex >= 0 ? selectedIndex : 0;

  const renderTree = (): ReactNode => (
    <div className="tree" role="tree" aria-label="文件树" tabIndex={-1} onKeyDown={handleTreeKey}>
      <NodeList
        entries={entries}
        parent={ROOT}
        depth={0}
        expanded={expanded}
        loading={loading}
        errors={errors}
        selectedPath={selectedPath}
        tabbableIndex={tabbableIndex}
        indexByPath={indexByPath}
        onToggle={toggle}
        onRetry={(p) => void loadDir(p)}
        onSelect={setSelectedPath}
        onFileSelect={onFileSelect}
        registerRow={registerRow}
      />
    </div>
  );

  let body: ReactNode;
  if (demo) {
    body =
      rootEntries && rootEntries.length > 0 ? renderTree() : <p className="aj-empty">空目录</p>;
  } else if (!sessionId) {
    body = <p className="aj-empty">未选择会话</p>;
  } else if (loading.has(ROOT)) {
    body = <p className="aj-empty">加载中…</p>;
  } else if (errors.has(ROOT)) {
    body = (
      <div className="aj-empty">
        <span>加载失败</span>
        <button type="button" className="aj-button--ghost" onClick={() => void loadDir(ROOT)}>
          重试
        </button>
      </div>
    );
  } else if (rootEntries && rootEntries.length > 0) {
    body = renderTree();
  } else if (rootEntries) {
    body = <p className="aj-empty">空目录</p>;
  } else {
    body = <p className="aj-empty">加载中…</p>;
  }

  return (
    <div className="filetree">
      {body}
      <div className="left-foot">
        <span>{stats.files} 个文件 · {stats.changes} 处变更</span>
        <span>⤢ 展开全部</span>
      </div>
    </div>
  );
});

export default FileTree;

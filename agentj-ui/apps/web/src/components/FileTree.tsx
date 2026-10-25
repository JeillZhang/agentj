import { forwardRef, useCallback, useImperativeHandle, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import { ChevronRight, Folder, FolderOpen } from "lucide-react";
import { parentPath } from "@agentj/sdk";
import type { GitLetter, RpcClient, SearchEntry } from "@agentj/sdk";
import { fileIconFor } from "../fileIcons";
import { moveSelection, tabIndexFor } from "../hooks/useRovingFocus";
import { useFileSearch } from "../hooks/useFileSearch";
import { useFileTree } from "../hooks/useFileTree";
import type { UseFileTreeResult } from "../hooks/useFileTree";
import { ROOT } from "./fileTreeModel";
import { searchHead, type SearchState } from "./fileTreeSearch";
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

function childGroupId(path: string): string {
  return `filetree-${path}-children`;
}

/** Row left inset grows linearly with depth so arbitrarily deep trees stay readable. */
function rowIndent(depth: number): number {
  return depth * 14 + 2;
}

type NodeListProps = {
  tree: UseFileTreeResult;
  tabbableIndex: number;
  parent: string;
  depth: number;
  onFileSelect?: (path: string) => void;
};

function StatusCell({ git }: { git: GitLetter }) {
  return <span className={`status-txt status-${git}`}>{git}</span>;
}

/** Renders one loaded level; dirs recurse only while expanded and drive their own load state. */
function NodeList({ tree, tabbableIndex, parent, depth, onFileSelect }: NodeListProps) {
  const { entries, expanded, loading, errors, selectedPath, indexByPath } = tree;
  const { toggle, reload, setSelectedPath, registerRow } = tree;
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
          if (open && loading.has(node.path)) body = <p className="aj-empty">加载中…</p>;
          else if (open && errors.has(node.path)) {
            body = (
              <div className="aj-empty">
                <span>加载失败</span>
                <button type="button" className="aj-button--ghost" onClick={() => reload(node.path)}>
                  重试
                </button>
              </div>
            );
          } else if (open && children && children.length === 0) body = <p className="aj-empty">空目录</p>;
          else if (open && children) {
            body = (
              <NodeList
                tree={tree}
                tabbableIndex={tabbableIndex}
                parent={node.path}
                depth={depth + 1}
                onFileSelect={onFileSelect}
              />
            );
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
                ref={(el) => registerRow(node.path, el)}
                onClick={() => toggle(node.path)}
                onFocus={() => setSelectedPath(node.path)}
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
            ref={(el) => registerRow(node.path, el)}
            onFocus={() => setSelectedPath(node.path)}
            onClick={() => {
              setSelectedPath(node.path);
              onFileSelect?.(node.path);
            }}
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

type SearchBarProps = { query: string; onChange: (value: string) => void; onEscape: () => void };

/** Debounced search input; Escape hands control back to the tree. */
function SearchBar({ query, onChange, onEscape }: SearchBarProps) {
  return (
    <div className="ft-search">
      <input
        type="search"
        className="aj-input"
        aria-label="搜索文件"
        placeholder="搜索文件…"
        value={query}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key !== "Escape") return;
          event.preventDefault();
          onEscape();
        }}
      />
    </div>
  );
}

type SearchResultsProps = { state: SearchState; onPick: (path: string) => void; onEscape: () => void };

/**
 * Result-row icon: hits are files by contract, but a dir entry must never be drawn as a file.
 */
function SearchRowIcon({ row }: { row: SearchEntry }) {
  if (row.kind === "dir") {
    return <Folder className="dir-icon dir-icon--closed" size={13} aria-hidden="true" />;
  }
  const { Icon, category } = fileIconFor(row.name);
  return <Icon className={`file-icon file-icon--${category}`} size={13} aria-hidden="true" />;
}

/**
 * Result rows: server order, `name` then the dimmed parent dir, git letter last.
 * Rows follow the tree's roving-tabindex contract (one `0`, the rest `-1`);
 * Escape and the arrows sit on the container so they also fire while a result
 * row has focus.
 */
function SearchResults({ state, onPick, onEscape }: SearchResultsProps) {
  /** Roving index tagged with its query: a new query restarts at row 0 without an effect. */
  const [roving, setRoving] = useState({ query: state.query, index: 0 });
  const rowRefs = useRef(new Map<string, HTMLButtonElement>());

  const active = roving.query === state.query ? roving.index : 0;
  const activeIndex = state.rows.length === 0 ? -1 : Math.min(active, state.rows.length - 1);

  const setActive = (index: number): void => {
    setRoving({ query: state.query, index });
  };

  const moveActive = (delta: number): void => {
    if (state.rows.length === 0) return;
    const next = moveSelection(Math.max(activeIndex, 0), delta, state.rows.length);
    setActive(next);
    const el = rowRefs.current.get(state.rows[next].path);
    el?.focus();
    if (el && typeof el.scrollIntoView === "function") el.scrollIntoView({ block: "nearest" });
  };

  const handleKey = (event: ReactKeyboardEvent<HTMLDivElement>): void => {
    if (event.key === "Escape") {
      event.preventDefault();
      onEscape();
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    moveActive(event.key === "ArrowDown" ? 1 : -1);
  };

  return (
    <>
      {state.error ? (
        <p className="ft-search-error">{`搜索失败：${state.error}`}</p>
      ) : (
        <p className="ft-search-head" data-empty={state.rows.length === 0 ? "true" : undefined}>
          {searchHead(state)}
        </p>
      )}
      <div className="tree" role="tree" aria-label="搜索结果" tabIndex={-1} onKeyDown={handleKey}>
        {state.rows.map((row, index) => {
          const parent = parentPath(row.path);
          const isActive = index === activeIndex;
          return (
            <button
              type="button"
              role="treeitem"
              className="trow trow-search"
              key={row.path}
              aria-selected={isActive}
              tabIndex={isActive ? 0 : -1}
              data-path={row.path}
              ref={(el) => {
                if (el) rowRefs.current.set(row.path, el);
                else rowRefs.current.delete(row.path);
              }}
              onFocus={() => setActive(index)}
              onClick={() => onPick(row.path)}
            >
              <span className="chev" aria-hidden="true" />
              <SearchRowIcon row={row} />
              <span className="name">{row.name}</span>
              {parent ? <span className="name-parent">{`${parent}/`}</span> : null}
              {row.gitStatus ? <StatusCell git={row.gitStatus} /> : null}
            </button>
          );
        })}
      </div>
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
  const tree = useFileTree({ client, sessionId, demo });
  const {
    paths,
    indexByPath,
    entryByPath,
    stats,
    rootEntries,
    expanded,
    loading,
    errors,
    selectedPath,
    selectPath,
    toggle,
    reload,
    moveBy: moveTreeSelection,
    refresh: refreshTree,
  } = tree;
  const { state: search, active: searchActive, changeQuery, exitSearch } = useFileSearch({
    client, sessionId, demo,
  });

  const moveBy = useCallback(
    (delta: number): void => {
      // The sidebar scope stays live while the search surface replaces the tree: moves would
      // silently change an invisible selection.
      if (searchActive) return;
      moveTreeSelection(delta);
    },
    [searchActive, moveTreeSelection],
  );

  const refresh = useCallback((): void => {
    if (searchActive) return;
    refreshTree();
  }, [searchActive, refreshTree]);

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

  const selectedIndex = selectedPath ? (indexByPath.get(selectedPath) ?? -1) : -1;
  const tabbableIndex = selectedIndex >= 0 ? selectedIndex : 0;

  const renderTree = (): ReactNode => (
    <div className="tree" role="tree" aria-label="文件树" tabIndex={-1} onKeyDown={handleTreeKey}>
      <NodeList tree={tree} tabbableIndex={tabbableIndex} parent={ROOT} depth={0} onFileSelect={onFileSelect} />
    </div>
  );

  /** Root body ladder: the tree, or the one reason there is no tree to draw. */
  const rootBody = (): ReactNode => {
    if (!sessionId) return <p className="aj-empty">未选择会话</p>;
    if (loading.has(ROOT)) return <p className="aj-empty">加载中…</p>;
    if (errors.has(ROOT)) {
      return (
        <div className="aj-empty">
          <span>加载失败</span>
          <button type="button" className="aj-button--ghost" onClick={() => reload(ROOT)}>
            重试
          </button>
        </div>
      );
    }
    if (rootEntries && rootEntries.length > 0) return renderTree();
    return <p className="aj-empty">{rootEntries ? "空目录" : "加载中…"}</p>;
  };
  const body = demo
    ? rootEntries && rootEntries.length > 0
      ? renderTree()
      : <p className="aj-empty">空目录</p>
    : rootBody();

  const searchBar = !demo && sessionId ? (
    <SearchBar query={search.query} onChange={changeQuery} onEscape={exitSearch} />
  ) : null;
  // Search mode replaces the tree body but never unmounts its state, so Escape restores
  // the fetched/expanded tree exactly as it was.
  const pane = searchActive ? (
    <SearchResults state={search} onPick={(path) => onFileSelect?.(path)} onEscape={exitSearch} />
  ) : (
    body
  );

  return (
    <div className="filetree">
      {searchBar}
      {pane}
      <div className="left-foot">
        <span>{stats.files} 个文件 · {stats.changes} 处变更</span>
        <span>⤢ 展开全部</span>
      </div>
    </div>
  );
});

export default FileTree;

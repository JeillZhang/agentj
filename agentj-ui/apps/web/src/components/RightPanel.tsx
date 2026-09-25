import { Children, useCallback, useEffect, useRef, useState } from "react";
import type {
  CSSProperties,
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
  ReactNode,
} from "react";
import { loadRightSectionSplit, saveRightSectionSplit } from "../layoutStore";
import { useRightSection } from "../rightSections";
import type { FileChangeRow } from "../changes";
import type { TodoView } from "../todoState";
import TodoPanel from "./TodoPanel";
import ChangeList from "./ChangeList";

const RESIZER_H = 6;
const SPLIT_MIN = 0.15;
const SPLIT_MAX = 0.85;

export type RunGroup = {
  runId: string | null;
  changes: FileChangeRow[];
  todos: TodoView[];
};

export type RightPanelProps = {
  collapsed: boolean;
  onToggle: () => void;
  children?: ReactNode;
  currentRunId?: string | null;
  groups?: RunGroup[];
  history?: FileChangeRow[];
  highlightedPath?: string | null;
  onSelectRun?: (runId: string | null) => void;
  changes?: FileChangeRow[];
  todos?: TodoView[];
  todosByRun?: Map<string | null, TodoView[]>;
  changesByRun?: Map<string | null, FileChangeRow[]>;
};

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}

function shortId(id: string): string {
  return id.slice(0, 8);
}

function runLabel(runId: string | null, currentRunId?: string | null): string {
  if (runId === null) return "未分组（历史）";
  if (currentRunId != null && runId === currentRunId) return `本次运行 ${shortId(runId)}`;
  return `运行 ${shortId(runId)}`;
}

export function groupByRunId(
  changes: FileChangeRow[],
  todosByRun?: Map<string | null, TodoView[]>,
  fallbackTodos?: TodoView[],
): RunGroup[] {
  const map = new Map<string | null, RunGroup>();
  const ensure = (runId: string | null): RunGroup => {
    const existing = map.get(runId);
    if (existing) return existing;
    const g: RunGroup = { runId, changes: [], todos: [] };
    map.set(runId, g);
    return g;
  };
  for (const row of changes) ensure((row.runId ?? null) as string | null).changes.push(row);
  if (todosByRun) {
    for (const [runId, items] of todosByRun) ensure(runId).todos = items;
  } else if (fallbackTodos && fallbackTodos.length > 0) {
    ensure(null).todos = fallbackTodos;
  }
  return [...map.values()];
}

function sortGroups(groups: RunGroup[], currentRunId?: string | null): RunGroup[] {
  return [...groups].sort((a, b) => {
    const aIsCurrent = currentRunId != null && a.runId === currentRunId;
    const bIsCurrent = currentRunId != null && b.runId === currentRunId;
    if (aIsCurrent && !bIsCurrent) return -1;
    if (!aIsCurrent && bIsCurrent) return 1;
    if (a.runId === null && b.runId !== null) return 1;
    if (a.runId !== null && b.runId === null) return -1;
    if (a.runId === null && b.runId === null) return 0;
    return 0;
  });
}

/**
 * Third-column container (运行详情). Owns the collapse rail chrome only —
 * collapse state lives in the parent so the layout can persist it.
 * Section/todo/file-change styles live in rightpanel.css (T10), shared with
 * TodoPanel (T8) and ChangeList (T9). When both sections are open it also
 * renders the draggable pane resizer.
 *
 * T7 grouping: when `groups` or `changes`/`todos` props are supplied, renders
 * per-run sections with headers ("本次运行", short id, "未分组（历史）") and
 * per-run empty placeholders. Otherwise falls back to children passthrough
 * (backward compat for existing callers/tests).
 */
export default function RightPanel({
  collapsed,
  onToggle,
  children,
  currentRunId,
  groups,
  history,
  highlightedPath,
  onSelectRun,
  changes,
  todos,
  todosByRun,
}: RightPanelProps) {
  const [todoOpen] = useRightSection("todo");
  const [changesOpen] = useRightSection("changes");
  const [split, setSplit] = useState<number | null>(() => loadRightSectionSplit());
  const [dragging, setDragging] = useState(false);
  const bodyRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ rect: DOMRect; start: number | null } | null>(null);
  const liveRef = useRef<{ active: boolean; last: number | null }>({ active: false, last: null });

  useEffect(() => {
    return () => {
      if (liveRef.current.active && liveRef.current.last != null) {
        saveRightSectionSplit(liveRef.current.last);
      }
    };
  }, []);

  const hasGrouping =
    (groups !== undefined && groups.length > 0) ||
    changes !== undefined ||
    todos !== undefined ||
    todosByRun !== undefined;

  const derivedGroups: RunGroup[] | null = (() => {
    if (!hasGrouping) return null;
    if (groups) return sortGroups(groups, currentRunId ?? null);
    const ch = changes ?? [];
    // include legacy todos split by run if map provided, else flat
    if (todosByRun) return sortGroups(groupByRunId(ch, todosByRun), currentRunId ?? null);
    if (todos && todos.length > 0 && ch.length === 0) {
      // todos only
      const m = new Map<string | null, TodoView[]>();
      m.set(currentRunId ?? null, todos);
      return sortGroups(groupByRunId(ch, m), currentRunId ?? null);
    }
    return sortGroups(groupByRunId(ch, undefined, todos), currentRunId ?? null);
  })();

  const items = Children.toArray(children);
  const bothOpen = todoOpen && changesOpen && items.length >= 2;

  const resetSplit = useCallback((): void => {
    setSplit(null);
    saveRightSectionSplit(null);
  }, []);

  const endDrag = (el: HTMLDivElement, pointerId: number): void => {
    dragRef.current = null;
    liveRef.current.active = false;
    setDragging(false);
    if (typeof el.releasePointerCapture === "function") {
      el.releasePointerCapture(pointerId);
    }
  };

  const handlePointerDown = (e: ReactPointerEvent<HTMLDivElement>): void => {
    const rect = bodyRef.current?.getBoundingClientRect();
    if (!rect || rect.height <= 0) return;
    dragRef.current = { rect, start: split };
    liveRef.current = { active: true, last: null };
    setDragging(true);
    if (typeof e.currentTarget.setPointerCapture === "function") {
      e.currentTarget.setPointerCapture(e.pointerId);
    }
  };

  const handlePointerMove = (e: ReactPointerEvent<HTMLDivElement>): void => {
    const drag = dragRef.current;
    if (!drag) return;
    const ratio = clamp(
      (e.clientY - drag.rect.top - RESIZER_H / 2) / drag.rect.height,
      SPLIT_MIN,
      SPLIT_MAX,
    );
    liveRef.current.last = ratio;
    setSplit(ratio);
  };

  const handlePointerUp = (e: ReactPointerEvent<HTMLDivElement>): void => {
    if (!dragRef.current) return;
    const latest = liveRef.current.last;
    if (latest != null) saveRightSectionSplit(latest);
    endDrag(e.currentTarget, e.pointerId);
  };

  const handlePointerCancel = (e: ReactPointerEvent<HTMLDivElement>): void => {
    const drag = dragRef.current;
    if (!drag) return;
    setSplit(drag.start);
    liveRef.current.active = false;
    endDrag(e.currentTarget, e.pointerId);
  };

  const handleKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>): void => {
    if (e.key === "Home") {
      e.preventDefault();
      resetSplit();
      return;
    }
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
    e.preventDefault();
    const step = (e.shiftKey ? 8 : 2) / 100;
    const base = split ?? 0.5;
    const next = clamp(base + (e.key === "ArrowDown" ? step : -step), SPLIT_MIN, SPLIT_MAX);
    setSplit(next);
    saveRightSectionSplit(next);
  };

  const custom = split !== null;
  const bodyStyle = custom ? ({ "--aj-split": String(split) } as CSSProperties) : undefined;
  const valuenow = custom ? Math.round(split * 100) : 50;

  const bodyContent = (() => {
    if (derivedGroups) {
      if (derivedGroups.length === 0) {
        // no groups: still render empty placeholders per AC (at least one legacy group)
        const label = runLabel(null, currentRunId ?? null);
        return (
          <div className="run-group" data-run-id="__legacy__">
            <div className="run-group__head">{label}</div>
            <p className="aj-empty">暂无任务</p>
            <p className="aj-empty">暂无变更</p>
          </div>
        );
      }
      return derivedGroups.map((group) => {
        const key = group.runId ?? "__legacy__";
        const label = runLabel(group.runId, currentRunId ?? null);
        const emptyGroup = group.changes.length === 0 && group.todos.length === 0;
        const isCurrent = currentRunId != null && group.runId === currentRunId;
        const groupHistory = history
          ? history.filter((r) => (r.runId ?? null) === group.runId)
          : undefined;
        const head = onSelectRun ? (
          <button type="button" className="run-group__head" onClick={() => onSelectRun(group.runId)}>
            {label}
          </button>
        ) : (
          <div className="run-group__head">{label}</div>
        );
        return (
          <div
            key={key}
            className="run-group"
            data-run-id={key}
            data-current={isCurrent ? "true" : undefined}
          >
            {head}
            {emptyGroup ? (
              <>
                <p className="aj-empty">暂无任务</p>
                <p className="aj-empty">暂无变更</p>
              </>
            ) : (
              <>
                {group.todos.length === 0 ? (
                  <p className="aj-empty">暂无任务</p>
                ) : (
                  <TodoPanel
                    items={group.todos}
                    activeRunId={currentRunId ?? null}
                    onSelectRun={onSelectRun}
                  />
                )}
                {group.changes.length === 0 ? (
                  <p className="aj-empty">暂无变更</p>
                ) : (
                  <ChangeList
                    changes={group.changes}
                    history={groupHistory}
                    emptyText="暂无变更"
                    activeRunId={currentRunId ?? null}
                    highlightedPath={highlightedPath}
                    onSelectRun={onSelectRun}
                  />
                )}
              </>
            )}
          </div>
        );
      });
    }
    if (bothOpen) {
      return (
        <>
          {items[0]}
          <div
            className="right-resizer"
            role="separator"
            aria-orientation="horizontal"
            aria-label="调整任务与文件变更高度"
            aria-valuemin={15}
            aria-valuemax={85}
            aria-valuenow={valuenow}
            tabIndex={0}
            data-dragging={dragging ? "true" : undefined}
            onPointerDown={handlePointerDown}
            onPointerMove={handlePointerMove}
            onPointerUp={handlePointerUp}
            onPointerCancel={handlePointerCancel}
            onDoubleClick={resetSplit}
            onKeyDown={handleKeyDown}
          />
          {items.slice(1)}
        </>
      );
    }
    return items;
  })();

  return (
    <aside
      className={`aj-right${collapsed ? " aj-right--collapsed" : ""}`}
      aria-label="运行详情"
    >
      <header className="aj-right__head">
        <span className="aj-right__title">运行详情</span>
        <button
          type="button"
          className="aj-right__collapse"
          aria-expanded={!collapsed}
          aria-label={collapsed ? "展开右栏" : "折叠右栏"}
          onClick={onToggle}
        >
          {collapsed ? "«" : "»"}
        </button>
      </header>
      <div
        ref={bodyRef}
        className="aj-right__body"
        aria-hidden={collapsed || undefined}
        data-both-open={bothOpen ? "true" : undefined}
        data-split={custom ? "custom" : undefined}
        style={bodyStyle}
      >
        {bodyContent}
      </div>
    </aside>
  );
}

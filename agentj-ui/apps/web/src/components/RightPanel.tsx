import { Children, useCallback, useEffect, useRef, useState } from "react";
import type {
  CSSProperties,
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
  ReactNode,
} from "react";
import { loadRightSectionSplit, saveRightSectionSplit } from "../layoutStore";
import { useRightSection } from "../rightSections";

const RESIZER_H = 6;
const SPLIT_MIN = 0.15;
const SPLIT_MAX = 0.85;

export type RightPanelProps = {
  collapsed: boolean;
  onToggle: () => void;
  children: ReactNode;
};

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}

/**
 * Third-column container (运行详情). Owns the collapse rail chrome only —
 * collapse state lives in the parent so the layout can persist it.
 * Section/todo/file-change styles live in rightpanel.css (T10), shared with
 * TodoPanel (T8) and ChangeList (T9). When both sections are open it also
 * renders the draggable pane resizer.
 */
export default function RightPanel({ collapsed, onToggle, children }: RightPanelProps) {
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
        {bothOpen ? (
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
        ) : (
          items
        )}
      </div>
    </aside>
  );
}

import { useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from "react";
import "./split.css";

export type SplitPaneSide = "left" | "right";

export type SplitPaneProps = {
  side: SplitPaneSide;
  width: number;
  min: number;
  max: number;
  defaultWidth: number;
  collapsed?: boolean;
  onResize: (width: number) => void;
  onCommit?: (width: number) => void;
  onReset: () => void;
  onToggleCollapse?: () => void;
};

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}

export default function SplitPane({
  side,
  width,
  min,
  max,
  defaultWidth,
  collapsed,
  onResize,
  onCommit,
  onReset,
  onToggleCollapse,
}: SplitPaneProps) {
  const currentWidth = Number.isFinite(width) ? width : defaultWidth;
  const dragRef = useRef<{ x: number; width: number } | null>(null);
  const lastRef = useRef<number | null>(null);
  const [dragging, setDragging] = useState(false);

  const endDrag = (el: HTMLDivElement, pointerId: number) => {
    dragRef.current = null;
    lastRef.current = null;
    setDragging(false);
    if (typeof el.releasePointerCapture === "function") {
      el.releasePointerCapture(pointerId);
    }
  };

  const handlePointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    dragRef.current = { x: e.clientX, width: currentWidth };
    lastRef.current = null;
    setDragging(true);
    if (typeof e.currentTarget.setPointerCapture === "function") {
      e.currentTarget.setPointerCapture(e.pointerId);
    }
  };

  const handlePointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const start = dragRef.current;
    if (!start) return;
    const dx = e.clientX - start.x;
    const next = clamp(start.width + (side === "left" ? dx : -dx), min, max);
    lastRef.current = next;
    onResize(next);
  };

  const handlePointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    const start = dragRef.current;
    if (!start) return;
    onCommit?.(lastRef.current ?? start.width);
    endDrag(e.currentTarget, e.pointerId);
  };

  const handlePointerCancel = (e: ReactPointerEvent<HTMLDivElement>) => {
    const start = dragRef.current;
    if (!start) return;
    onResize(start.width);
    endDrag(e.currentTarget, e.pointerId);
  };

  const handleKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Home") {
      e.preventDefault();
      onReset();
      return;
    }
    if (e.key === "Enter" || e.key === " ") {
      if (side === "right" && onToggleCollapse) {
        e.preventDefault();
        onToggleCollapse();
      }
      return;
    }
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const step = e.shiftKey ? 32 : 8;
    const positive = side === "left" ? e.key === "ArrowRight" : e.key === "ArrowLeft";
    const next = clamp(currentWidth + (positive ? step : -step), min, max);
    onResize(next);
    onCommit?.(next);
  };

  return (
    <div
      className="aj-splitter"
      data-side={side}
      data-dragging={dragging ? "true" : undefined}
      data-collapsed={collapsed ? "true" : undefined}
      role="separator"
      aria-orientation="vertical"
      tabIndex={0}
      aria-label={side === "left" ? "调整左栏宽度" : "调整右栏宽度"}
      aria-valuenow={currentWidth}
      aria-valuemin={min}
      aria-valuemax={max}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerCancel}
      onDoubleClick={onReset}
      onKeyDown={handleKeyDown}
    >
      <span className="aj-splitter__line" />
      <span className="aj-splitter__grip" aria-hidden="true">
        <i />
        <i />
        <i />
      </span>
    </div>
  );
}

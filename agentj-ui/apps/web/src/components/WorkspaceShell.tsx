import { useCallback, useEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import SplitPane from "./SplitPane";
import {
  LAYOUT_DEFAULTS,
  LAYOUT_LIMITS,
  loadLayout,
  saveLayout,
  subscribeLayout,
} from "../layoutStore";
import type { LayoutState } from "../layoutStore";
import "./layout.css";

const CENTER_MIN = 480;
const SPLITTER_HIT = 7;
const COLUMN_GAP = 12;

export type WorkspaceShellProps = {
  children: [ReactNode, ReactNode];
  right: ReactNode;
  rightCollapsed: boolean;
  onRightCollapsedChange: (collapsed: boolean) => void;
  rightDrawerOpen?: boolean;
};

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}

export default function WorkspaceShell({
  children,
  right,
  rightCollapsed,
  onRightCollapsedChange,
  rightDrawerOpen = false,
}: WorkspaceShellProps) {
  const [left, center] = children;
  const [layout, setLayout] = useState<LayoutState>(() => loadLayout());
  const rootRef = useRef<HTMLDivElement>(null);
  const [containerWidth, setContainerWidth] = useState(0);

  useEffect(() => subscribeLayout(setLayout), []);

  useEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const measure = (): void => setContainerWidth(el.getBoundingClientRect().width);
    measure();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", measure);
      return () => window.removeEventListener("resize", measure);
    }
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const dynamicMax = useCallback(
    (side: "left" | "right"): number => {
      const staticMin = side === "left" ? LAYOUT_LIMITS.leftMin : LAYOUT_LIMITS.rightMin;
      const staticMax = side === "left" ? LAYOUT_LIMITS.leftMax : LAYOUT_LIMITS.rightMax;
      if (containerWidth <= 0) return staticMax;
      const other = side === "left" ? layout.rightWidth : layout.leftWidth;
      const reserved = other + 2 * SPLITTER_HIT + 4 * COLUMN_GAP;
      return clamp(containerWidth - CENTER_MIN - reserved, staticMin, staticMax);
    },
    [containerWidth, layout.leftWidth, layout.rightWidth],
  );

  const leftMax = dynamicMax("left");
  const rightMax = dynamicMax("right");
  const leftWidth = clamp(layout.leftWidth, LAYOUT_LIMITS.leftMin, leftMax);
  const rightWidth = clamp(layout.rightWidth, LAYOUT_LIMITS.rightMin, rightMax);

  const handleLeftResize = useCallback(
    (width: number) => setLayout((prev) => ({ ...prev, leftWidth: width })),
    [],
  );
  const handleRightResize = useCallback(
    (width: number) => setLayout((prev) => ({ ...prev, rightWidth: width })),
    [],
  );
  const handleLeftCommit = useCallback((width: number) => saveLayout({ leftWidth: width }), []);
  const handleRightCommit = useCallback((width: number) => saveLayout({ rightWidth: width }), []);
  const handleLeftReset = useCallback(
    () => saveLayout({ leftWidth: LAYOUT_DEFAULTS.leftWidth }),
    [],
  );
  const handleRightReset = useCallback(
    () => saveLayout({ rightWidth: LAYOUT_DEFAULTS.rightWidth }),
    [],
  );
  const handleRightToggleCollapse = useCallback(
    () => onRightCollapsedChange(!rightCollapsed),
    [onRightCollapsedChange, rightCollapsed],
  );

  const styleVars = {
    "--aj-left-w": `${leftWidth}px`,
    "--aj-right-w": `${rightWidth}px`,
  } as CSSProperties;

  return (
    <div
      ref={rootRef}
      className={`aj-layout aj-workspace${rightDrawerOpen ? " aj-workspace--right-open" : ""}`}
      style={styleVars}
    >
      <div className="aj-shell__left">{left}</div>
      <SplitPane
        side="left"
        width={leftWidth}
        min={LAYOUT_LIMITS.leftMin}
        max={leftMax}
        defaultWidth={LAYOUT_DEFAULTS.leftWidth}
        onResize={handleLeftResize}
        onCommit={handleLeftCommit}
        onReset={handleLeftReset}
      />
      <main className="aj-shell__center">{center}</main>
      <SplitPane
        side="right"
        width={rightWidth}
        min={LAYOUT_LIMITS.rightMin}
        max={rightMax}
        defaultWidth={LAYOUT_DEFAULTS.rightWidth}
        onResize={handleRightResize}
        onCommit={handleRightCommit}
        onReset={handleRightReset}
        onToggleCollapse={handleRightToggleCollapse}
      />
      <div className="aj-shell__right">{right}</div>
    </div>
  );
}

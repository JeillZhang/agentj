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
import { loadDisplayPrefs, subscribeDisplayPrefs } from "../displayPrefsStore";
import type { DisplayLineHeight, DisplayPrefs, FontScale } from "../displayPrefsStore";
import "./layout.css";

const CENTER_MIN = 480;
const SPLITTER_HIT = 7;
const COLUMN_GAP = 12;
const RIGHT_RAIL = 40; // matches .aj-workspace--right-collapsed rail width in layout.css

type FontPx = { conv: string; left: string; right: string; code: string };

const FONT_PX: Record<FontScale, FontPx> = {
  sm: { conv: "13px", left: "12px", right: "12px", code: "11px" },
  md: { conv: "15px", left: "13px", right: "13px", code: "12px" },
  lg: { conv: "17px", left: "14.5px", right: "14.5px", code: "13.5px" },
  xl: { conv: "19px", left: "16px", right: "16px", code: "15px" },
};

const LH: Record<DisplayLineHeight, string> = {
  compact: "1.4",
  default: "1.55",
  relaxed: "1.8",
};

function displayVars(display: DisplayPrefs): Record<string, string> {
  return {
    "--aj-font-conv": FONT_PX[display.fontConversation].conv,
    "--aj-font-left": FONT_PX[display.fontLeft].left,
    "--aj-font-right": FONT_PX[display.fontRight].right,
    "--aj-font-code": FONT_PX[display.fontCode].code,
    "--aj-lh-conv": LH[display.lineHeight],
  };
}

const noop = (): void => {};

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
  const [display, setDisplay] = useState<DisplayPrefs>(() => loadDisplayPrefs());
  const rootRef = useRef<HTMLDivElement>(null);
  const [containerWidth, setContainerWidth] = useState(0);

  useEffect(() => subscribeLayout(setLayout), []);

  useEffect(() => subscribeDisplayPrefs(setDisplay), []);

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
    ...displayVars(display),
  } as CSSProperties;

  const rootClass = [
    "aj-layout",
    "aj-workspace",
    rightDrawerOpen ? "aj-workspace--right-open" : "",
    rightCollapsed ? "aj-workspace--right-collapsed" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div ref={rootRef} className={rootClass} style={styleVars} data-density={display.density}>
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
        width={rightCollapsed ? RIGHT_RAIL : rightWidth}
        min={rightCollapsed ? RIGHT_RAIL : LAYOUT_LIMITS.rightMin}
        max={rightCollapsed ? RIGHT_RAIL : rightMax}
        defaultWidth={LAYOUT_DEFAULTS.rightWidth}
        collapsed={rightCollapsed}
        onResize={rightCollapsed ? noop : handleRightResize}
        onCommit={rightCollapsed ? noop : handleRightCommit}
        onReset={rightCollapsed ? noop : handleRightReset}
        onToggleCollapse={handleRightToggleCollapse}
      />
      <div className="aj-shell__right">{right}</div>
    </div>
  );
}

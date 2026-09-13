import type { ReactNode } from "react";

export type RightPanelProps = {
  collapsed: boolean;
  onToggle: () => void;
  children: ReactNode;
};

/**
 * Third-column container (运行详情). Owns the collapse rail chrome only —
 * collapse state lives in the parent so the layout can persist it.
 * Section/todo/file-change styles live in rightpanel.css (T10), shared with
 * TodoPanel (T8) and ChangeList (T9).
 */
export default function RightPanel({ collapsed, onToggle, children }: RightPanelProps) {
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
      <div className="aj-right__body" aria-hidden={collapsed || undefined}>
        {children}
      </div>
    </aside>
  );
}

import { useRef } from "react";
import type { KeyboardEvent } from "react";
import { moveSelection, tabIndexFor } from "../hooks/useRovingFocus";
import "./sidebar.css";

export type SidebarTab = "sessions" | "files";

export type SidebarTabsProps = {
  tab: SidebarTab;
  onChange: (tab: SidebarTab) => void;
};

const TABS: SidebarTab[] = ["sessions", "files"];
const LABELS: Record<SidebarTab, string> = { sessions: "会话", files: "文件" };

export default function SidebarTabs({ tab, onChange }: SidebarTabsProps) {
  const tabRefs = useRef(new Map<SidebarTab, HTMLButtonElement | null>());

  const selected = TABS.indexOf(tab);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const delta = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (delta === 0) return;
    const next = moveSelection(selected, delta, TABS.length, { wrap: true });
    if (next < 0) return;
    event.preventDefault();
    const nextTab = TABS[next];
    onChange(nextTab);
    tabRefs.current.get(nextTab)?.focus();
  };

  return (
    <div className="aj-seg" role="tablist" aria-label="侧栏切换" onKeyDown={onKeyDown}>
      {TABS.map((id, index) => (
        <button
          key={id}
          ref={(el) => {
            tabRefs.current.set(id, el);
          }}
          type="button"
          role="tab"
          data-tab={id}
          aria-selected={tab === id}
          tabIndex={tabIndexFor(index, selected)}
          className={tab === id ? "active" : ""}
          onClick={() => onChange(id)}
        >
          {LABELS[id]}
        </button>
      ))}
    </div>
  );
}

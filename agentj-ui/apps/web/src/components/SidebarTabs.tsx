import { useRef } from "react";
import type { KeyboardEvent } from "react";
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

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = TABS.indexOf(tab);
    let next = -1;
    if (event.key === "ArrowRight") next = (index + 1) % TABS.length;
    else if (event.key === "ArrowLeft") next = (index - 1 + TABS.length) % TABS.length;
    if (next < 0) return;
    event.preventDefault();
    const nextTab = TABS[next];
    onChange(nextTab);
    tabRefs.current.get(nextTab)?.focus();
  };

  return (
    <div className="aj-seg" role="tablist" aria-label="侧栏切换" onKeyDown={onKeyDown}>
      {TABS.map((id) => (
        <button
          key={id}
          ref={(el) => {
            tabRefs.current.set(id, el);
          }}
          type="button"
          role="tab"
          data-tab={id}
          aria-selected={tab === id}
          tabIndex={tab === id ? 0 : -1}
          className={tab === id ? "active" : ""}
          onClick={() => onChange(id)}
        >
          {LABELS[id]}
        </button>
      ))}
    </div>
  );
}

import "./sidebar.css";

export type SidebarTab = "sessions" | "files";

export type SidebarTabsProps = {
  tab: SidebarTab;
  onChange: (tab: SidebarTab) => void;
};

export default function SidebarTabs({ tab, onChange }: SidebarTabsProps) {
  return (
    <div className="aj-seg" role="tablist" aria-label="侧栏切换">
      <button
        type="button"
        role="tab"
        aria-selected={tab === "sessions"}
        className={tab === "sessions" ? "active" : ""}
        onClick={() => onChange("sessions")}
      >
        会话
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={tab === "files"}
        className={tab === "files" ? "active" : ""}
        onClick={() => onChange("files")}
      >
        文件
      </button>
    </div>
  );
}

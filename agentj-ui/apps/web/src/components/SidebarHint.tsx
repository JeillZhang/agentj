import { SIDEBAR_HINTS } from "./shortcutHints";

export type SidebarHintProps = {
  view: "threads" | "files";
  searching: boolean;
};

/** Contextual, display-only shortcut hint line. */
export default function SidebarHint({ view, searching }: SidebarHintProps) {
  const text = view === "threads"
    ? SIDEBAR_HINTS.threads
    : searching
      ? SIDEBAR_HINTS.filesSearching
      : SIDEBAR_HINTS.files;
  return (
    <div className="aj-sidebar__hint" data-testid="sidebar-hint" aria-hidden="true">
      {text}
    </div>
  );
}

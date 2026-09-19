// SessionListMenu.tsx — sidebar session-list overflow menu.
// Three rows: 清理所有会话… (danger, guarded by deletableCount), 过滤 › and 排序 ›
// hover flyouts mirrored to open RIGHT, persisting via sidebarPrefsStore.
// Flyout recipe mirrors TopbarMenu (120ms leave delay, Escape layering, outside
// mousedown via rootRef, ArrowUp/Down roaming).
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type {
  FocusEvent as ReactFocusEvent,
  KeyboardEvent as ReactKeyboardEvent,
  MouseEvent as ReactMouseEvent,
} from "react";
import { MoreHorizontal } from "lucide-react";
import { loadSidebarPrefs, saveSidebarPrefs, subscribeSidebarPrefs } from "../sidebarPrefsStore";
import type { SidebarFilter, SidebarPrefs, SidebarSort } from "../sidebarPrefsStore";
import "./topbarmenu.css";

type Props = {
  deletableCount: number;
  busy?: boolean;
  onCleanup: () => void;
};

type FlyoutKind = null | "filter" | "sort";

const FLYOUT_CLOSE_DELAY_MS = 120;

const FILTER_OPTIONS: ReadonlyArray<{ value: SidebarFilter; label: string }> = [
  { value: "all", label: "全部" },
  { value: "favorites", label: "仅收藏" },
];

const SORT_OPTIONS: ReadonlyArray<{ value: SidebarSort; label: string }> = [
  { value: "updated", label: "最近更新" },
  { value: "created", label: "创建时间" },
];

export default function SessionListMenu({ deletableCount, busy, onCleanup }: Props) {
  const [open, setOpen] = useState(false);
  const [flyout, setFlyout] = useState<FlyoutKind>(null);
  const [prefs, setPrefs] = useState<SidebarPrefs>(() => loadSidebarPrefs());
  // Fixed viewport coordinates for the portaled menu (trigger-relative at open).
  const [menuPos, setMenuPos] = useState({ top: 0, left: 0 });
  const rootRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const filterTriggerRef = useRef<HTMLButtonElement>(null);
  const sortTriggerRef = useRef<HTMLButtonElement>(null);
  const closeTimerRef = useRef<number | null>(null);
  const suppressToggleRef = useRef(false);
  const suppressFocusOpenRef = useRef(false);

  useEffect(() => subscribeSidebarPrefs(setPrefs), []);

  const cancelClose = useCallback(() => {
    if (closeTimerRef.current != null) {
      window.clearTimeout(closeTimerRef.current);
      closeTimerRef.current = null;
    }
  }, []);

  const scheduleClose = useCallback(() => {
    cancelClose();
    closeTimerRef.current = window.setTimeout(() => {
      closeTimerRef.current = null;
      setFlyout(null);
    }, FLYOUT_CLOSE_DELAY_MS);
  }, [cancelClose]);

  const openFlyout = useCallback(
    (kind: Exclude<FlyoutKind, null>) => {
      cancelClose();
      setFlyout(kind);
    },
    [cancelClose],
  );

  const toggleFlyout = useCallback(
    (kind: Exclude<FlyoutKind, null>) => {
      cancelClose();
      setFlyout((prev) => (prev === kind ? null : kind));
    },
    [cancelClose],
  );

  const closeAll = useCallback(() => {
    cancelClose();
    setFlyout(null);
    setOpen(false);
  }, [cancelClose]);

  useEffect(() => () => cancelClose(), [cancelClose]);

  const focusWithoutOpening = useCallback((el: HTMLButtonElement | null) => {
    suppressFocusOpenRef.current = true;
    el?.focus();
    queueMicrotask(() => {
      suppressFocusOpenRef.current = false;
    });
  }, []);

  const updateMenuPos = useCallback(() => {
    const rect = triggerRef.current?.getBoundingClientRect();
    if (!rect) return;
    setMenuPos({ top: rect.bottom + 6, left: rect.left });
  }, []);

  useLayoutEffect(() => {
    if (open) updateMenuPos();
  }, [open, updateMenuPos]);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (flyout) {
        setFlyout(null);
        focusWithoutOpening((flyout === "filter" ? filterTriggerRef : sortTriggerRef).current);
        return;
      }
      setOpen(false);
      triggerRef.current?.focus();
    };
    const onDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (rootRef.current?.contains(target) || menuRef.current?.contains(target)) return;
      closeAll();
    };
    const onResize = () => updateMenuPos();
    // Scrolling moves the trigger; close rather than let the fixed menu drift.
    const onScroll = () => closeAll();
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onDown);
    window.addEventListener("resize", onResize);
    document.addEventListener("scroll", onScroll, true);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onDown);
      window.removeEventListener("resize", onResize);
      document.removeEventListener("scroll", onScroll, true);
    };
  }, [open, flyout, focusWithoutOpening, closeAll, updateMenuPos]);

  const toggleMenu = useCallback(() => {
    cancelClose();
    setPrefs(loadSidebarPrefs());
    setFlyout(null);
    setOpen((v) => !v);
  }, [cancelClose]);

  const cleanupDisabled = deletableCount === 0 || busy === true;

  const runCleanup = useCallback(() => {
    if (cleanupDisabled) return;
    onCleanup();
    closeAll();
  }, [cleanupDisabled, onCleanup, closeAll]);

  const pickFilter = useCallback(
    (filter: SidebarFilter) => {
      saveSidebarPrefs({ filter });
      closeAll();
    },
    [closeAll],
  );

  const pickSort = useCallback(
    (sort: SidebarSort) => {
      saveSidebarPrefs({ sort });
      closeAll();
    },
    [closeAll],
  );

  const onRowBlur = useCallback((event: ReactFocusEvent<HTMLDivElement>) => {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFlyout(null);
  }, []);

  const onFlyoutKeyDown = useCallback((event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const items = Array.from(
      event.currentTarget.querySelectorAll<HTMLElement>('[role="menuitem"], [role="menuitemradio"]'),
    );
    if (items.length === 0) return;
    const idx = items.indexOf(document.activeElement as HTMLElement);
    const next =
      event.key === "ArrowDown" ? (idx + 1) % items.length : (idx - 1 + items.length) % items.length;
    event.preventDefault();
    items[next]?.focus();
  }, []);

  const onTriggerMouseDown = useCallback((event: ReactMouseEvent<HTMLButtonElement>) => {
    suppressToggleRef.current = document.activeElement !== event.currentTarget;
  }, []);

  const onTriggerClick = useCallback(
    (kind: Exclude<FlyoutKind, null>) => {
      if (suppressToggleRef.current) {
        suppressToggleRef.current = false;
        return;
      }
      toggleFlyout(kind);
    },
    [toggleFlyout],
  );

  return (
    <div className="aj-sidebar__menu-wrap" ref={rootRef}>
      <button
        ref={triggerRef}
        type="button"
        className="aj-button--ghost aj-sidebar__menu-btn"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label="会话列表菜单"
        onClick={toggleMenu}
      >
        <MoreHorizontal size={14} aria-hidden="true" />
      </button>

      {open &&
        createPortal(
          <div
            ref={menuRef}
            className="aj-menu aj-sidebar__menu"
            role="menu"
            style={{ top: menuPos.top, left: menuPos.left }}
          >
            <button
              type="button"
              role="menuitem"
              className="aj-menu__item aj-menu__item--danger"
              aria-disabled={cleanupDisabled}
              onClick={runCleanup}
            >
              清理所有会话…
            </button>

            <div className="aj-menu__sep" role="separator" />

            <div
              className="aj-menu__row"
              onMouseEnter={() => openFlyout("filter")}
              onMouseLeave={scheduleClose}
              onBlur={onRowBlur}
            >
              <button
                ref={filterTriggerRef}
                type="button"
                role="menuitem"
                className="aj-menu__item"
                aria-haspopup="menu"
                aria-expanded={flyout === "filter"}
                aria-controls="aj-session-menu-filter"
                onMouseEnter={() => openFlyout("filter")}
                onMouseLeave={scheduleClose}
                onMouseDown={onTriggerMouseDown}
                onFocus={() => {
                  if (!suppressFocusOpenRef.current) openFlyout("filter");
                }}
                onClick={() => onTriggerClick("filter")}
              >
                <span>过滤</span>
                <span className="aj-menu__chevron" aria-hidden="true">
                  ›
                </span>
              </button>
              {flyout === "filter" && (
                <div
                  id="aj-session-menu-filter"
                  className="aj-menu__flyout aj-menu__flyout--right"
                  role="menu"
                  aria-label="过滤"
                  onMouseEnter={cancelClose}
                  onMouseLeave={scheduleClose}
                  onKeyDown={onFlyoutKeyDown}
                >
                  {FILTER_OPTIONS.map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      role="menuitemradio"
                      aria-checked={prefs.filter === option.value}
                      className="aj-menu__item aj-menu__item--sub"
                      onClick={() => pickFilter(option.value)}
                    >
                      <span>{option.label}</span>
                      {prefs.filter === option.value && (
                        <span className="aj-menu__state">✓</span>
                      )}
                    </button>
                  ))}
                </div>
              )}
            </div>

            <div
              className="aj-menu__row"
              onMouseEnter={() => openFlyout("sort")}
              onMouseLeave={scheduleClose}
              onBlur={onRowBlur}
            >
              <button
                ref={sortTriggerRef}
                type="button"
                role="menuitem"
                className="aj-menu__item"
                aria-haspopup="menu"
                aria-expanded={flyout === "sort"}
                aria-controls="aj-session-menu-sort"
                onMouseEnter={() => openFlyout("sort")}
                onMouseLeave={scheduleClose}
                onMouseDown={onTriggerMouseDown}
                onFocus={() => {
                  if (!suppressFocusOpenRef.current) openFlyout("sort");
                }}
                onClick={() => onTriggerClick("sort")}
              >
                <span>排序</span>
                <span className="aj-menu__chevron" aria-hidden="true">
                  ›
                </span>
              </button>
              {flyout === "sort" && (
                <div
                  id="aj-session-menu-sort"
                  className="aj-menu__flyout aj-menu__flyout--right"
                  role="menu"
                  aria-label="排序"
                  onMouseEnter={cancelClose}
                  onMouseLeave={scheduleClose}
                  onKeyDown={onFlyoutKeyDown}
                >
                  {SORT_OPTIONS.map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      role="menuitemradio"
                      aria-checked={prefs.sort === option.value}
                      className="aj-menu__item aj-menu__item--sub"
                      onClick={() => pickSort(option.value)}
                    >
                      <span>{option.label}</span>
                      {prefs.sort === option.value && (
                        <span className="aj-menu__state">✓</span>
                      )}
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>,
          document.body,
        )}
    </div>
  );
}

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
import { firstIndex, lastIndex, tabIndexFor, useRovingFocus } from "../hooks/useRovingFocus";
import type { RovingFocus } from "../hooks/useRovingFocus";
import "./topbarmenu.css";

type Props = {
  deletableCount: number;
  busy?: boolean;
  onCleanup: () => void;
};

type FlyoutKind = null | "filter" | "sort";

const FLYOUT_CLOSE_DELAY_MS = 120;

export const MENU_MIN_WIDTH = 200;
export const MENU_EDGE_MARGIN = 8;
export const FLYOUT_GAP = 6;

export type FlyoutSide = "right" | "left";

export function clampMenuLeft(triggerLeft: number, menuWidth: number, viewportWidth: number): number {
  const maxLeft = viewportWidth - menuWidth - MENU_EDGE_MARGIN;
  return Math.max(MENU_EDGE_MARGIN, Math.min(triggerLeft, maxLeft));
}

export function chooseFlyoutSide(rowRight: number, flyoutWidth: number, viewportWidth: number): FlyoutSide {
  return rowRight + FLYOUT_GAP + flyoutWidth <= viewportWidth ? "right" : "left";
}

const FILTER_OPTIONS: ReadonlyArray<{ value: SidebarFilter; label: string }> = [
  { value: "all", label: "全部" },
  { value: "favorites", label: "仅收藏" },
];

const SORT_OPTIONS: ReadonlyArray<{ value: SidebarSort; label: string }> = [
  { value: "updated", label: "最近更新" },
  { value: "created", label: "创建时间" },
];

/** ArrowUp/Down (wrapping), Home and End roving for an open flyout. */
function roveFlyout(
  roving: RovingFocus,
  count: number,
  event: ReactKeyboardEvent<HTMLDivElement>,
  keyboardNavRef: { current: boolean },
): void {
  switch (event.key) {
    case "ArrowDown":
      event.preventDefault();
      keyboardNavRef.current = true;
      if (roving.selected < 0) roving.setSelected(firstIndex(count));
      else roving.moveBy(1);
      return;
    case "ArrowUp":
      event.preventDefault();
      keyboardNavRef.current = true;
      if (roving.selected < 0) roving.setSelected(lastIndex(count));
      else roving.moveBy(-1);
      return;
    case "Home":
      event.preventDefault();
      keyboardNavRef.current = true;
      roving.setSelected(firstIndex(count));
      return;
    case "End":
      event.preventDefault();
      keyboardNavRef.current = true;
      roving.setSelected(lastIndex(count));
      return;
    default:
      return;
  }
}

export default function SessionListMenu({ deletableCount, busy, onCleanup }: Props) {
  const [open, setOpen] = useState(false);
  const [flyout, setFlyout] = useState<FlyoutKind>(null);
  const [flyoutSide, setFlyoutSide] = useState<FlyoutSide>("right");
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
  const filterRoving = useRovingFocus({ count: FILTER_OPTIONS.length, wrap: true });
  const sortRoving = useRovingFocus({ count: SORT_OPTIONS.length, wrap: true });
  const filterItemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const sortItemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const keyboardNavRef = useRef(false);

  useEffect(() => subscribeSidebarPrefs(setPrefs), []);

  // Seed the open flyout's first row as its single tab stop without stealing
  // focus; only keyboard roving (flagged below) moves focus into the flyout.
  useEffect(() => {
    if (!flyout) return;
    keyboardNavRef.current = false;
    if (flyout === "filter") filterRoving.setSelected(0);
    else sortRoving.setSelected(0);
  }, [flyout, filterRoving.setSelected, sortRoving.setSelected]);

  useEffect(() => {
    if (!keyboardNavRef.current) return;
    keyboardNavRef.current = false;
    filterItemRefs.current[filterRoving.selected]?.focus({ preventScroll: true });
  }, [filterRoving.selected]);

  useEffect(() => {
    if (!keyboardNavRef.current) return;
    keyboardNavRef.current = false;
    sortItemRefs.current[sortRoving.selected]?.focus({ preventScroll: true });
  }, [sortRoving.selected]);

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

  const measureFlyoutSide = useCallback((kind: Exclude<FlyoutKind, null>): FlyoutSide => {
    const trigger = kind === "filter" ? filterTriggerRef.current : sortTriggerRef.current;
    const rect = trigger?.getBoundingClientRect();
    if (!rect) return "right";
    return chooseFlyoutSide(rect.right, MENU_MIN_WIDTH, window.innerWidth);
  }, []);

  const openFlyout = useCallback(
    (kind: Exclude<FlyoutKind, null>) => {
      cancelClose();
      setFlyoutSide(measureFlyoutSide(kind));
      setFlyout(kind);
    },
    [cancelClose, measureFlyoutSide],
  );

  const toggleFlyout = useCallback(
    (kind: Exclude<FlyoutKind, null>) => {
      cancelClose();
      if (flyout === kind) {
        setFlyout(null);
      } else {
        setFlyoutSide(measureFlyoutSide(kind));
        setFlyout(kind);
      }
    },
    [cancelClose, flyout, measureFlyoutSide],
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
    const menuWidth = menuRef.current?.getBoundingClientRect().width || MENU_MIN_WIDTH;
    setMenuPos({
      top: rect.bottom + 6,
      left: clampMenuLeft(rect.left, menuWidth, window.innerWidth),
    });
    if (flyout) setFlyoutSide(measureFlyoutSide(flyout));
  }, [flyout, measureFlyoutSide]);

  useLayoutEffect(() => {
    if (!open) return;
    updateMenuPos();
    if (document.activeElement === triggerRef.current) {
      menuRef.current
        ?.querySelector<HTMLElement>('[role="menuitem"]')
        ?.focus({ preventScroll: true });
    }
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
    if (!open) triggerRef.current?.focus();
    setOpen((v) => !v);
  }, [cancelClose, open]);

  const cleanupDisabled = deletableCount === 0 || busy === true;
  const flyoutClass = `aj-menu__flyout ${flyoutSide === "left" ? "aj-menu__flyout--left" : "aj-menu__flyout--right"}`;

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
                  className={flyoutClass}
                  role="menu"
                  aria-label="过滤"
                  onMouseEnter={cancelClose}
                  onMouseLeave={scheduleClose}
                  onKeyDown={(event) => roveFlyout(filterRoving, FILTER_OPTIONS.length, event, keyboardNavRef)}
                >
                  {FILTER_OPTIONS.map((option, index) => (
                    <button
                      key={option.value}
                      ref={(el) => {
                        filterItemRefs.current[index] = el;
                      }}
                      type="button"
                      role="menuitemradio"
                      aria-checked={prefs.filter === option.value}
                      tabIndex={tabIndexFor(index, filterRoving.selected)}
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
                  className={flyoutClass}
                  role="menu"
                  aria-label="排序"
                  onMouseEnter={cancelClose}
                  onMouseLeave={scheduleClose}
                  onKeyDown={(event) => roveFlyout(sortRoving, SORT_OPTIONS.length, event, keyboardNavRef)}
                >
                  {SORT_OPTIONS.map((option, index) => (
                    <button
                      key={option.value}
                      ref={(el) => {
                        sortItemRefs.current[index] = el;
                      }}
                      type="button"
                      role="menuitemradio"
                      aria-checked={prefs.sort === option.value}
                      tabIndex={tabIndexFor(index, sortRoving.selected)}
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

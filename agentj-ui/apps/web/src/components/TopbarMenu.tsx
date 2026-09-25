// TopbarMenu.tsx — topbar ⋯ overflow menu.
// Five first-level items: 项目 (hover flyout), 继续会话…, 收藏会话 (hover flyout),
// 设置… (SettingsDialog), 帮助. Sort/filter/demo/connect live inside SettingsDialog.
import { useCallback, useEffect, useRef, useState } from "react";
import type {
  FocusEvent as ReactFocusEvent,
  KeyboardEvent as ReactKeyboardEvent,
  MouseEvent as ReactMouseEvent,
} from "react";
import { loadFavorites } from "../layoutStore";
import {
  createProject,
  loadCurrentProject,
  loadProjectDefaults,
  loadProjects,
  setCurrentProject,
} from "../projectStore";
import type { Project } from "../projectStore";
import { firstIndex, lastIndex, tabIndexFor, useRovingFocus } from "../hooks/useRovingFocus";
import ModalDialog from "./ModalDialog";
import SettingsDialog from "./SettingsDialog";
import "./topbarmenu.css";

type Props = {
  onConnect: (host: string, port: string, token: string) => void;
  onContinue: (sessionId: string) => void;
};

type DialogKind = null | "continue" | "addProject" | "settings";
type FlyoutKind = null | "project" | "favorites";

const FLYOUT_CLOSE_DELAY_MS = 120;

export default function TopbarMenu({ onConnect, onContinue }: Props) {
  const [open, setOpen] = useState(false);
  const [dialog, setDialog] = useState<DialogKind>(null);
  const [flyout, setFlyout] = useState<FlyoutKind>(null);
  const [favorites, setFavorites] = useState<string[]>(() => loadFavorites());
  const [projects, setProjects] = useState<Project[]>(() => loadProjects());
  const [currentProjectId, setCurrentProjectId] = useState<string>(() => loadCurrentProject());
  const [addName, setAddName] = useState("");
  const [addWorkDir, setAddWorkDir] = useState("");
  const [continueId, setContinueId] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const projectTriggerRef = useRef<HTMLButtonElement>(null);
  const favoritesTriggerRef = useRef<HTMLButtonElement>(null);
  const closeTimerRef = useRef<number | null>(null);
  const suppressToggleRef = useRef(false);
  const suppressFocusOpenRef = useRef(false);
  const flyoutItemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const keyboardNavRef = useRef(false);

  // Roving set = every focusable flyout row (project radios + "＋ 添加项目",
  // or favorite rows). Mirrors the pre-migration arrow navigation scope.
  const flyoutItemCount =
    flyout === "project" ? projects.length + 1 : flyout === "favorites" ? favorites.length : 0;
  const { selected, setSelected, moveBy } = useRovingFocus({ count: flyoutItemCount, wrap: true });

  useEffect(() => {
    if (!flyout) return;
    keyboardNavRef.current = false;
    setSelected(0);
  }, [flyout, setSelected]);

  useEffect(() => {
    if (!keyboardNavRef.current) return;
    keyboardNavRef.current = false;
    flyoutItemRefs.current[selected]?.focus();
  }, [selected]);

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

  useEffect(() => () => cancelClose(), [cancelClose]);

  const focusWithoutOpening = useCallback((el: HTMLButtonElement | null) => {
    suppressFocusOpenRef.current = true;
    el?.focus();
    queueMicrotask(() => {
      suppressFocusOpenRef.current = false;
    });
  }, []);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (flyout) {
        setFlyout(null);
        focusWithoutOpening((flyout === "project" ? projectTriggerRef : favoritesTriggerRef).current);
        return;
      }
      setOpen(false);
      triggerRef.current?.focus();
    };
    const onDown = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onDown);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onDown);
    };
  }, [open, flyout, focusWithoutOpening]);

  const closeDialog = useCallback(() => {
    setDialog(null);
    triggerRef.current?.focus();
  }, []);

  const openMenu = useCallback(() => {
    cancelClose();
    setFavorites(loadFavorites());
    setProjects(loadProjects());
    setCurrentProjectId(loadCurrentProject());
    setFlyout(null);
    setOpen((v) => !v);
  }, [cancelClose]);

  const openDialog = useCallback((kind: Exclude<DialogKind, null>) => {
    setOpen(false);
    setFlyout(null);
    setDialog(kind);
  }, []);

  const pickFavorite = useCallback(
    (id: string) => {
      onContinue(id);
      setOpen(false);
    },
    [onContinue],
  );

  const submitContinue = useCallback(() => {
    const id = continueId.trim();
    if (id) onContinue(id);
    setContinueId("");
    setDialog(null);
  }, [continueId, onContinue]);

  const pickProject = useCallback((id: string) => {
    setCurrentProject(id);
    setCurrentProjectId(id);
    setOpen(false);
  }, []);

  const openAddProject = useCallback(() => {
    setAddName("");
    setAddWorkDir(loadProjectDefaults().workDir ?? "");
    setOpen(false);
    setDialog("addProject");
  }, []);

  const submitAddProject = useCallback(() => {
    const name = addName.trim();
    if (!name) return;
    const created = createProject(name, addWorkDir.trim());
    setCurrentProject(created.id);
    setCurrentProjectId(created.id);
    setProjects(loadProjects());
    setAddName("");
    setAddWorkDir("");
    setDialog(null);
  }, [addName, addWorkDir]);

  const onRowBlur = useCallback((event: ReactFocusEvent<HTMLDivElement>) => {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFlyout(null);
  }, []);

  const onFlyoutKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>) => {
      const { key } = event;
      if (key !== "ArrowDown" && key !== "ArrowUp" && key !== "Home" && key !== "End") return;
      if (flyoutItemCount === 0) return;
      event.preventDefault();
      keyboardNavRef.current = true;
      if (key === "ArrowDown") moveBy(1);
      else if (key === "ArrowUp") moveBy(-1);
      else if (key === "Home") setSelected(firstIndex(flyoutItemCount));
      else setSelected(lastIndex(flyoutItemCount));
    },
    [flyoutItemCount, moveBy, setSelected],
  );

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
    <div className="aj-topbar-menu" ref={rootRef}>
      <button
        ref={triggerRef}
        type="button"
        className="aj-icon-btn"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label="更多"
        onClick={openMenu}
      >
        ⋯
      </button>

      {open && (
        <div className="aj-menu" role="menu">
          <div
            className="aj-menu__row"
            onMouseEnter={() => openFlyout("project")}
            onMouseLeave={scheduleClose}
            onBlur={onRowBlur}
          >
            <button
              ref={projectTriggerRef}
              type="button"
              role="menuitem"
              className="aj-menu__item"
              aria-haspopup="menu"
              aria-expanded={flyout === "project"}
              aria-controls="aj-menu-flyout-project"
              onMouseDown={onTriggerMouseDown}
              onFocus={() => {
                if (!suppressFocusOpenRef.current) openFlyout("project");
              }}
              onClick={() => onTriggerClick("project")}
            >
              <span>项目</span>
              <span className="aj-menu__chevron" aria-hidden="true">
                ›
              </span>
            </button>
            {flyout === "project" && (
              <div
                id="aj-menu-flyout-project"
                className="aj-menu__flyout"
                role="menu"
                aria-label="项目"
                onMouseEnter={cancelClose}
                onMouseLeave={scheduleClose}
                onKeyDown={onFlyoutKeyDown}
              >
                {projects.map((p, index) => (
                  <button
                    key={p.id}
                    ref={(el) => {
                      flyoutItemRefs.current[index] = el;
                    }}
                    type="button"
                    role="menuitemradio"
                    aria-checked={p.id === currentProjectId}
                    tabIndex={tabIndexFor(index, selected)}
                    className="aj-menu__item aj-menu__item--sub"
                    onFocus={() => setSelected(index)}
                    onClick={() => pickProject(p.id)}
                  >
                    <span>{p.name}</span>
                    {p.id === currentProjectId && <span className="aj-menu__state">✓</span>}
                  </button>
                ))}
                <div className="aj-menu__sep" role="separator" />
                <button
                  ref={(el) => {
                    flyoutItemRefs.current[projects.length] = el;
                  }}
                  type="button"
                  role="menuitem"
                  tabIndex={tabIndexFor(projects.length, selected)}
                  className="aj-menu__item aj-menu__item--sub"
                  onFocus={() => setSelected(projects.length)}
                  onClick={openAddProject}
                >
                  ＋ 添加项目
                </button>
              </div>
            )}
          </div>

          <button type="button" role="menuitem" className="aj-menu__item" onClick={() => openDialog("continue")}>
            继续会话…
          </button>

          <div
            className="aj-menu__row"
            onMouseEnter={() => openFlyout("favorites")}
            onMouseLeave={scheduleClose}
            onBlur={onRowBlur}
          >
            <button
              ref={favoritesTriggerRef}
              type="button"
              role="menuitem"
              className="aj-menu__item"
              aria-haspopup="menu"
              aria-expanded={flyout === "favorites"}
              aria-controls="aj-menu-flyout-favorites"
              onMouseDown={onTriggerMouseDown}
              onFocus={() => {
                if (!suppressFocusOpenRef.current) openFlyout("favorites");
              }}
              onClick={() => onTriggerClick("favorites")}
            >
              <span>收藏会话</span>
              <span className="aj-menu__chevron" aria-hidden="true">
                ›
              </span>
            </button>
            {flyout === "favorites" && (
              <div
                id="aj-menu-flyout-favorites"
                className="aj-menu__flyout"
                role="menu"
                aria-label="收藏会话"
                onMouseEnter={cancelClose}
                onMouseLeave={scheduleClose}
                onKeyDown={onFlyoutKeyDown}
              >
                {favorites.length === 0 ? (
                  <p className="aj-menu__empty">暂无收藏</p>
                ) : (
                  favorites.map((id, index) => (
                    <button
                      key={id}
                      ref={(el) => {
                        flyoutItemRefs.current[index] = el;
                      }}
                      type="button"
                      role="menuitem"
                      tabIndex={tabIndexFor(index, selected)}
                      className="aj-menu__item aj-menu__item--sub"
                      onFocus={() => setSelected(index)}
                      onClick={() => pickFavorite(id)}
                    >
                      {id}
                    </button>
                  ))
                )}
              </div>
            )}
          </div>

          <button type="button" role="menuitem" className="aj-menu__item" onClick={() => openDialog("settings")}>
            设置…
          </button>
          <button type="button" role="menuitem" className="aj-menu__item" onClick={() => setOpen(false)}>
            帮助
          </button>
        </div>
      )}

      {dialog === "settings" && (
        <SettingsDialog onConnect={onConnect} onClose={closeDialog} />
      )}

      {dialog === "continue" && (
        <ModalDialog title="继续会话" onClose={closeDialog}>
          <div className="aj-field">
            <label htmlFor="aj-continue-id" className="aj-label">
              session id
            </label>
            <input
              id="aj-continue-id"
              className="aj-input"
              value={continueId}
              onChange={(e) => setContinueId(e.target.value)}
              placeholder="session id"
            />
          </div>
          <div className="aj-dialog__actions">
            <button type="button" className="aj-button--ghost" onClick={closeDialog}>
              取消
            </button>
            <button type="button" className="aj-button" onClick={submitContinue}>
              继续
            </button>
          </div>
        </ModalDialog>
      )}

      {dialog === "addProject" && (
        <ModalDialog title="添加项目" onClose={closeDialog}>
          <div className="aj-field">
            <label htmlFor="aj-add-project" className="aj-label">
              项目名称
            </label>
            <input
              id="aj-add-project"
              className="aj-input"
              autoFocus
              value={addName}
              onChange={(e) => setAddName(e.target.value)}
              placeholder="项目名称"
            />
          </div>
          <div className="aj-field">
            <label htmlFor="aj-add-project-dir" className="aj-label">
              工作目录
            </label>
            <input
              id="aj-add-project-dir"
              className="aj-input"
              value={addWorkDir}
              onChange={(e) => setAddWorkDir(e.target.value)}
              placeholder="/path/to/project"
            />
          </div>
          <div className="aj-dialog__actions">
            <button type="button" className="aj-button--ghost" onClick={closeDialog}>
              取消
            </button>
            <button type="button" className="aj-button" disabled={!addName.trim()} onClick={submitAddProject}>
              添加
            </button>
          </div>
        </ModalDialog>
      )}
    </div>
  );
}

// TopbarMenu.tsx — topbar overflow menu (agentj 0.14.0 three-column workbench).
// Hosts sidebar features migrated to the topbar: connect/continue dialogs,
// favorites, sort/filter prefs, demo-data switch, settings/help.
import { useCallback, useEffect, useRef, useState } from "react";
import ConnectionBar from "../ConnectionBar";
import { isDemoEnabled, setDemoEnabled, subscribeDemo } from "../demoData";
import { loadFavorites } from "../layoutStore";
import ModalDialog from "./ModalDialog";
import "./topbarmenu.css";

type Props = {
  model?: string | null;
  connectionLabel?: string;
  modules?: string[];
  onConnect: (host: string, port: string, token: string) => void;
  onContinue: (sessionId: string) => void;
};

type DialogKind = null | "connect" | "continue";
type SubmenuKind = null | "sort" | "filter";

const PREF_EVENT = "agentj:sidebar-prefs";

function dispatchPrefs(patch: { sort?: string; filter?: string }): void {
  window.dispatchEvent(new CustomEvent(PREF_EVENT, { detail: patch }));
}

export default function TopbarMenu({
  model,
  connectionLabel = "未连接",
  modules,
  onConnect,
  onContinue,
}: Props) {
  const [open, setOpen] = useState(false);
  const [dialog, setDialog] = useState<DialogKind>(null);
  const [submenu, setSubmenu] = useState<SubmenuKind>(null);
  const [demo, setDemo] = useState<boolean>(() => isDemoEnabled());
  const [favorites, setFavorites] = useState<string[]>(() => loadFavorites());
  const [continueId, setContinueId] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);

  const connected = connectionLabel.length > 0 && !connectionLabel.includes("未连接");

  useEffect(() => subscribeDemo(setDemo), []);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setOpen(false);
      setSubmenu(null);
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
  }, [open]);

  useEffect(() => {
    if (!dialog) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        setDialog(null);
        triggerRef.current?.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [dialog]);

  const openMenu = useCallback(() => {
    setFavorites(loadFavorites());
    setSubmenu(null);
    setOpen((v) => !v);
  }, []);

  const openDialog = useCallback((kind: Exclude<DialogKind, null>) => {
    setOpen(false);
    setSubmenu(null);
    setDialog(kind);
  }, []);

  const toggleDemo = useCallback(() => {
    const next = !isDemoEnabled();
    setDemoEnabled(next);
    setDemo(next);
  }, []);

  const pickPref = useCallback((patch: { sort?: string; filter?: string }) => {
    dispatchPrefs(patch);
    setSubmenu(null);
    setOpen(false);
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
          <div className="aj-menu__status" role="presentation" aria-label="连接状态">
            <span className="aj-menu__dot" data-state={connected ? "on" : "off"} />
            <span className="aj-menu__conn">{connectionLabel}</span>
            {model && <span className="aj-chip aj-chip--model">{model}</span>}
            {modules?.map((m) => (
              <span key={m} className="aj-chip">
                {m}
              </span>
            ))}
          </div>
          <div className="aj-menu__sep" role="separator" />

          <button type="button" role="menuitem" className="aj-menu__item" onClick={() => openDialog("connect")}>
            连接设置…
          </button>
          <button type="button" role="menuitem" className="aj-menu__item" onClick={() => openDialog("continue")}>
            继续会话…
          </button>

          <p className="aj-menu__label">收藏会话</p>
          {favorites.length === 0 ? (
            <p className="aj-menu__empty">暂无收藏</p>
          ) : (
            favorites.map((id) => (
              <button
                key={id}
                type="button"
                role="menuitem"
                className="aj-menu__item aj-menu__item--sub"
                onClick={() => pickFavorite(id)}
              >
                {id}
              </button>
            ))
          )}

          <button
            type="button"
            role="menuitem"
            className="aj-menu__item"
            aria-haspopup="menu"
            aria-expanded={submenu === "sort"}
            onClick={() => setSubmenu((v) => (v === "sort" ? null : "sort"))}
          >
            排序
          </button>
          {submenu === "sort" && (
            <div className="aj-menu__sub" role="group" aria-label="排序">
              <button
                type="button"
                role="menuitemradio"
                className="aj-menu__item aj-menu__item--sub"
                onClick={() => pickPref({ sort: "updated" })}
              >
                时间
              </button>
              <button
                type="button"
                role="menuitemradio"
                className="aj-menu__item aj-menu__item--sub"
                onClick={() => pickPref({ sort: "created" })}
              >
                创建时间
              </button>
            </div>
          )}

          <button
            type="button"
            role="menuitem"
            className="aj-menu__item"
            aria-haspopup="menu"
            aria-expanded={submenu === "filter"}
            onClick={() => setSubmenu((v) => (v === "filter" ? null : "filter"))}
          >
            过滤
          </button>
          {submenu === "filter" && (
            <div className="aj-menu__sub" role="group" aria-label="过滤">
              <button
                type="button"
                role="menuitemradio"
                className="aj-menu__item aj-menu__item--sub"
                onClick={() => pickPref({ filter: "all" })}
              >
                全部
              </button>
              <button
                type="button"
                role="menuitemradio"
                className="aj-menu__item aj-menu__item--sub"
                onClick={() => pickPref({ filter: "favorites" })}
              >
                收藏
              </button>
            </div>
          )}

          <button type="button" role="menuitem" className="aj-menu__item" onClick={toggleDemo}>
            <span>演示数据</span>
            <span className="aj-menu__state">{demo ? "已开启" : "已关闭"}</span>
          </button>
          <button type="button" role="menuitem" className="aj-menu__item" onClick={() => setOpen(false)}>
            设置
          </button>
          <button type="button" role="menuitem" className="aj-menu__item" onClick={() => setOpen(false)}>
            帮助
          </button>
        </div>
      )}

      {dialog === "connect" && (
        <ModalDialog title="连接设置" onClose={() => setDialog(null)}>
          <ConnectionBar
            onConnect={(host, port, token) => {
              onConnect(host, port, token);
              setDialog(null);
            }}
          />
        </ModalDialog>
      )}

      {dialog === "continue" && (
        <ModalDialog title="继续会话" onClose={() => setDialog(null)}>
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
            <button type="button" className="aj-button--ghost" onClick={() => setDialog(null)}>
              取消
            </button>
            <button type="button" className="aj-button" onClick={submitContinue}>
              继续
            </button>
          </div>
        </ModalDialog>
      )}
    </div>
  );
}

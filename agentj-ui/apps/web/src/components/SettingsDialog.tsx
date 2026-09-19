import { useCallback, useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import ConnectionBar from "../ConnectionBar";
import { isDemoEnabled, setDemoEnabled, subscribeDemo } from "../demoData";
import { loadSidebarPrefs, saveSidebarPrefs } from "../sidebarPrefsStore";
import type { SidebarFilter, SidebarSort } from "../sidebarPrefsStore";
import ModalDialog from "./ModalDialog";
import "./settingsDialog.css";

type Props = {
  onConnect: (host: string, port: string, token: string) => void;
  onClose: () => void;
};

type CategoryId = "connection" | "sessions" | "demo";
type Category = { id: CategoryId; label: string };

const CATEGORIES: Category[] = [
  { id: "connection", label: "连接" },
  { id: "sessions", label: "会话列表" },
  { id: "demo", label: "演示数据" },
];

function panelId(id: CategoryId): string {
  return `aj-settings-panel-${id}`;
}

function tabId(id: CategoryId): string {
  return `aj-settings-tab-${id}`;
}

export default function SettingsDialog({ onConnect, onClose }: Props) {
  const [active, setActive] = useState<CategoryId>("connection");
  const [prefs, setPrefs] = useState(loadSidebarPrefs);
  const [demo, setDemo] = useState(isDemoEnabled);
  const tabRefs = useRef(new Map<CategoryId, HTMLButtonElement | null>());

  useEffect(() => subscribeDemo(setDemo), []);

  const select = useCallback((id: CategoryId) => {
    setActive(id);
    tabRefs.current.get(id)?.focus();
  }, []);

  const onTabKeyDown = useCallback(
    (event: KeyboardEvent<HTMLButtonElement>) => {
      const idx = CATEGORIES.findIndex((c) => c.id === active);
      let next = -1;
      if (event.key === "ArrowDown") next = (idx + 1) % CATEGORIES.length;
      else if (event.key === "ArrowUp") next = (idx - 1 + CATEGORIES.length) % CATEGORIES.length;
      else if (event.key === "Home") next = 0;
      else if (event.key === "End") next = CATEGORIES.length - 1;
      if (next < 0) return;
      event.preventDefault();
      select(CATEGORIES[next].id);
    },
    [active, select],
  );

  const pickSort = useCallback((sort: SidebarSort) => setPrefs(saveSidebarPrefs({ sort })), []);
  const pickFilter = useCallback((filter: SidebarFilter) => setPrefs(saveSidebarPrefs({ filter })), []);

  return (
    <ModalDialog title="设置" onClose={onClose} cardClassName="aj-settings">
      <div className="aj-settings-body">
        <div className="aj-settings-nav" role="tablist" aria-orientation="vertical" aria-label="设置分类">
          {CATEGORIES.map((category) => (
            <button
              key={category.id}
              ref={(el) => {
                tabRefs.current.set(category.id, el);
              }}
              id={tabId(category.id)}
              type="button"
              role="tab"
              aria-selected={active === category.id}
              aria-controls={panelId(category.id)}
              tabIndex={active === category.id ? 0 : -1}
              className={`aj-settings-nav__item${active === category.id ? " aj-settings-nav__item--active" : ""}`}
              onClick={() => select(category.id)}
              onKeyDown={onTabKeyDown}
            >
              {category.label}
            </button>
          ))}
        </div>
        <div
          id={panelId(active)}
          role="tabpanel"
          aria-labelledby={tabId(active)}
          className="aj-settings-panel"
          tabIndex={0}
        >
          {active === "connection" && (
            <ConnectionBar
              onConnect={(host, port, token) => {
                onConnect(host, port, token);
                onClose();
              }}
            />
          )}
          {active === "sessions" && (
            <div className="aj-settings-group">
              <fieldset className="aj-settings-fieldset">
                <legend className="aj-settings-legend">排序</legend>
                <label className="aj-settings-radio">
                  <input
                    type="radio"
                    name="aj-settings-sort"
                    checked={prefs.sort === "updated"}
                    onChange={() => pickSort("updated")}
                  />
                  <span>时间</span>
                </label>
                <label className="aj-settings-radio">
                  <input
                    type="radio"
                    name="aj-settings-sort"
                    checked={prefs.sort === "created"}
                    onChange={() => pickSort("created")}
                  />
                  <span>创建时间</span>
                </label>
              </fieldset>
              <fieldset className="aj-settings-fieldset">
                <legend className="aj-settings-legend">过滤</legend>
                <label className="aj-settings-radio">
                  <input
                    type="radio"
                    name="aj-settings-filter"
                    checked={prefs.filter === "all"}
                    onChange={() => pickFilter("all")}
                  />
                  <span>全部</span>
                </label>
                <label className="aj-settings-radio">
                  <input
                    type="radio"
                    name="aj-settings-filter"
                    checked={prefs.filter === "favorites"}
                    onChange={() => pickFilter("favorites")}
                  />
                  <span>收藏</span>
                </label>
              </fieldset>
            </div>
          )}
          {active === "demo" && (
            <div className="aj-settings-toggle">
              <span>演示数据</span>
              <button
                type="button"
                role="switch"
                aria-checked={demo}
                className="aj-settings-switch"
                onClick={() => setDemoEnabled(!isDemoEnabled())}
              >
                {demo ? "已开启" : "已关闭"}
              </button>
            </div>
          )}
        </div>
      </div>
    </ModalDialog>
  );
}

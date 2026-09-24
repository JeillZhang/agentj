import { useCallback, useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import ConnectionBar from "../ConnectionBar";
import { isDemoEnabled, setDemoEnabled, subscribeDemo } from "../demoData";
import { firstIndex, lastIndex, moveSelection, tabIndexFor } from "../hooks/useRovingFocus";
import ModalDialog from "./ModalDialog";
import SettingsDisplayPanel from "./SettingsDisplayPanel";
import SettingsProjectPanel from "./SettingsProjectPanel";
import "./settingsDialog.css";

type Props = {
  onConnect: (host: string, port: string, token: string) => void;
  onClose: () => void;
};

type CategoryId = "connection" | "display" | "project" | "demo";
type Category = { id: CategoryId; label: string };

const CATEGORIES: Category[] = [
  { id: "connection", label: "连接" },
  { id: "display", label: "显示" },
  { id: "project", label: "项目" },
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
  const [demo, setDemo] = useState(isDemoEnabled);
  const tabRefs = useRef(new Map<CategoryId, HTMLButtonElement | null>());
  const activeIndex = CATEGORIES.findIndex((category) => category.id === active);

  useEffect(() => subscribeDemo(setDemo), []);

  const select = useCallback((id: CategoryId) => {
    setActive(id);
    tabRefs.current.get(id)?.focus();
  }, []);

  const onTabKeyDown = useCallback(
    (event: KeyboardEvent<HTMLButtonElement>) => {
      const count = CATEGORIES.length;
      let next: number;
      if (event.key === "ArrowDown") next = moveSelection(activeIndex, 1, count, { wrap: true });
      else if (event.key === "ArrowUp") next = moveSelection(activeIndex, -1, count, { wrap: true });
      else if (event.key === "Home") next = firstIndex(count);
      else if (event.key === "End") next = lastIndex(count);
      else return;
      if (next < 0) return;
      event.preventDefault();
      select(CATEGORIES[next].id);
    },
    [activeIndex, select],
  );

  return (
    <ModalDialog title="设置" onClose={onClose} cardClassName="aj-settings" minWidth={560}>
      <div className="aj-settings-body">
        <div className="aj-settings-nav" role="tablist" aria-orientation="vertical" aria-label="设置分类">
          {CATEGORIES.map((category, index) => (
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
              tabIndex={tabIndexFor(index, activeIndex)}
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
          {active === "display" && <SettingsDisplayPanel />}
          {active === "project" && <SettingsProjectPanel />}
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

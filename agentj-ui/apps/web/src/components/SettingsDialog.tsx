import { useCallback, useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import type { RpcClient } from "@agentj/sdk";
import ConnectionBar from "../ConnectionBar";
import { isDemoEnabled, setDemoEnabled, subscribeDemo } from "../demoData";
import { firstIndex, lastIndex, moveSelection, tabIndexFor } from "../hooks/useRovingFocus";
import ModalDialog from "./ModalDialog";
import SettingsDisplayPanel from "./SettingsDisplayPanel";
import SettingsProjectPanel from "./SettingsProjectPanel";
import SubscriptionPanel from "./SubscriptionPanel";
import type { SubscriptionStatusData } from "../hooks/useSubscriptionStatus";
import type { AuthSetApiKeyResult } from "@agentj/protocol";
import "./settingsDialog.css";

type Props = {
  onConnect: (host: string, port: string, token: string) => void;
  onClose: () => void;
  client?: RpcClient | null;
  /** Forwards a successful API-key link's account catalog to the app shell. */
  onModelsLinked?: (catalog: AuthSetApiKeyResult) => void;
  /** Forwards a confirmed credential clear, so the shell drops the linked catalog. */
  onModelsCleared?: () => void;
  /** Tab to open on mount — lets the topbar light land on 订阅 directly. */
  initialCategory?: CategoryId;
  /** Shell-owned usage snapshot; the panel renders it instead of fetching again. */
  subscription?: SubscriptionStatusData;
};

type CategoryId = "connection" | "display" | "project" | "demo" | "usage" | "subscription";
export type SettingsCategory = CategoryId;
type Category = { id: CategoryId; label: string };

const CATEGORIES: Category[] = [
  { id: "connection", label: "连接" },
  { id: "display", label: "显示" },
  { id: "project", label: "项目" },
  { id: "demo", label: "演示数据" },
  { id: "usage", label: "用量" },
  { id: "subscription", label: "订阅" },
];

function panelId(id: CategoryId): string {
  return `aj-settings-panel-${id}`;
}

function tabId(id: CategoryId): string {
  return `aj-settings-tab-${id}`;
}

export default function SettingsDialog({
  onConnect,
  onClose,
  client = null,
  onModelsLinked,
  onModelsCleared,
  initialCategory = "connection",
  subscription,
}: Props) {
  const [active, setActive] = useState<CategoryId>(initialCategory);
  // A missing/invalid credential is the one thing on the 订阅 tab worth flagging from the
  // nav: everything else there is optional.
  const credentialPending = subscription?.state === "unknown" || subscription?.state === "invalid";
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
      const from = CATEGORIES.findIndex((category) => tabRefs.current.get(category.id) === event.currentTarget);
      if (from < 0) return;
      let next: number;
      if (event.key === "ArrowDown") next = moveSelection(from, 1, count, { wrap: true });
      else if (event.key === "ArrowUp") next = moveSelection(from, -1, count, { wrap: true });
      else if (event.key === "Home") next = firstIndex(count);
      else if (event.key === "End") next = lastIndex(count);
      else return;
      if (next < 0) return;
      event.preventDefault();
      select(CATEGORIES[next].id);
    },
    [select],
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
              title={
                category.id === "subscription" && credentialPending
                  ? "订阅 · 待配置凭据"
                  : undefined
              }
              onClick={() => select(category.id)}
              onKeyDown={onTabKeyDown}
            >
              {category.label}
              {category.id === "subscription" && credentialPending && (
                // Decorative dot: the state lives in the tab's title, so the accessible
                // name stays exactly the label.
                <span
                  className="aj-settings-nav__badge"
                  data-testid="settings-nav-badge"
                  aria-hidden="true"
                />
              )}
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
          {active === "usage" &&
            (client ? (
              <SubscriptionPanel client={client} onClose={onClose} onLinked={onModelsLinked}
                onCleared={onModelsCleared} status={subscription} section="usage" />
            ) : (
              <p className="aj-hint">先连接内核</p>
            ))}
          {active === "subscription" &&
            (client ? (
              <SubscriptionPanel client={client} onClose={onClose} onLinked={onModelsLinked}
                onCleared={onModelsCleared} status={subscription} section="subscription" />
            ) : (
              <p className="aj-hint">先连接内核</p>
            ))}
        </div>
      </div>
    </ModalDialog>
  );
}

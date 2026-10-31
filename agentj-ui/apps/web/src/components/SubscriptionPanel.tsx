import type { RpcClient } from "@agentj/sdk";
import type { AuthSetApiKeyResult } from "@agentj/protocol";
import SubscriptionHistory from "./SubscriptionHistory";
import { CredentialRow } from "./SubscriptionUsage";
import type { SubscriptionStatusData } from "../hooks/useSubscriptionStatus";
import { useSubscriptionData } from "./useSubscriptionData";
import "./subscriptionPanel.css";

type Props = {
  client: RpcClient;
  onClose: () => void;
  onLinked?: (result: AuthSetApiKeyResult) => void;
  /** Fired once the kernel removed a stored credential, so the shell drops its catalog. */
  onCleared?: () => void;
  /**
   * Injected snapshot from the topbar light's hook. When present the panel renders that
   * snapshot instead of fetching usage itself: one owner for the polling contract.
   */
  status?: SubscriptionStatusData;
  /**
   * Which half of the account surface to render. Settings splits 用量 and 订阅 into two
   * tabs, but both read the same fetches, so one component owns the data and each tab
   * mounts only its own section.
   */
  section?: "all" | "usage" | "subscription";
};

export default function SubscriptionPanel({ client, onClose, onLinked, onCleared, status, section = "all" }: Props) {
  const {
    auth,
    usage,
    samples,
    historyAt,
    device,
    deviceMsg,
    key,
    setKey,
    linkMsg,
    envShadow,
    refreshUsage,
    link,
    clear,
    startDevice,
  } = useSubscriptionData(client, status, { onLinked, onCleared });

  return (
    <div className="aj-sub">
      <div className="aj-sub__head">
        <span className="aj-sub__title">OpenCode Go 订阅</span>
        <button type="button" className="aj-sub__btn" onClick={onClose}>
          关闭
        </button>
      </div>
      {section !== "usage" && auth && <CredentialRow status={auth} />}
      {section !== "subscription" && (
        <SubscriptionHistory
          usage={usage}
          samples={samples}
          historyAt={historyAt}
          onRefresh={refreshUsage}
        />
      )}
      {section !== "usage" && (
        <>
      <div className="aj-sub__link">
        <span className="aj-sub__section">链接方式</span>
        <label className="aj-sub__link-label" htmlFor="aj-sub-api-key">
          API Key
        </label>
        <div className="aj-sub__link-row">
          <input
            id="aj-sub-api-key"
            className="aj-sub__input"
            type="password"
            autoComplete="off"
            value={key}
            placeholder="sk-…"
            onChange={(e) => setKey(e.target.value)}
          />
          <button
            type="button"
            className="aj-sub__btn"
            disabled={key.trim().length === 0}
            onClick={() => void link()}
          >
            链接
          </button>
          <button type="button" className="aj-sub__btn" onClick={() => void clear()}>
            解除链接
          </button>
        </div>
        <p className="aj-sub__hint">
          API Key 只保存在所连接的内核主机（auth.json），不会写入浏览器存储。
        </p>
        {auth?.credential?.kind === "oauth" && (
          <p className="aj-sub__error">将替换现有 OAuth 凭据</p>
        )}
        {envShadow && (
          <p className="aj-sub__msg">已保存；环境变量当前优先，链接的密钥暂不生效</p>
        )}
        {linkMsg && <p className="aj-sub__msg">{linkMsg}</p>}
      </div>
      <div className="aj-sub__actions">
        <button type="button" className="aj-sub__btn" onClick={() => void startDevice()}>
          设备码登录
        </button>
      </div>
        </>
      )}
      {section !== "usage" && device && (
        <div className="aj-sub__device">
          <p>
            请在浏览器打开 <a href={device.verification_uri}>{device.verification_uri}</a>
          </p>
          <p>
            并输入设备码 <span className="aj-sub__code">{device.user_code}</span>
          </p>
        </div>
      )}
      {section !== "usage" && deviceMsg && <p className="aj-sub__msg">{deviceMsg}</p>}
    </div>
  );
}

import type { RpcClient } from "@agentj/sdk";
import type { AuthSetApiKeyResult } from "@agentj/protocol";
import SubscriptionCredentialCard from "./SubscriptionCredentialCard";
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
        <SubscriptionCredentialCard
          auth={auth}
          apiKey={key}
          setKey={setKey}
          envShadow={envShadow}
          linkMsg={linkMsg}
          device={device}
          deviceMsg={deviceMsg}
          onLink={() => void link()}
          onClear={() => void clear()}
          onStartDevice={() => void startDevice()}
        />
      )}
    </div>
  );
}

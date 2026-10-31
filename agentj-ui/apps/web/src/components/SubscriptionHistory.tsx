import type { UsageSample } from "@agentj/protocol";
import SubscriptionUsage from "./SubscriptionUsage";
import type { ProviderUsage } from "./useSubscriptionData";

/**
 * The read-only usage half of the account surface: the chart, the history sparkline and
 * the manual refresh. Split out of `SubscriptionPanel` so each half of the settings tab
 * stays under a readable size; the DOM and class names are unchanged.
 */
export default function SubscriptionHistory(props: {
  usage: ProviderUsage | null;
  samples: UsageSample[];
  historyAt: number;
  onRefresh: () => void;
}) {
  const { usage, samples, historyAt, onRefresh } = props;
  return (
    <>
      {usage ? (
        <SubscriptionUsage usage={usage} samples={samples} now={historyAt} />
      ) : (
        <p className="aj-sub__hint">正在获取用量…</p>
      )}
      <div className="aj-sub__actions aj-sub__actions--usage">
        <button type="button" className="aj-sub__btn" onClick={onRefresh}>
          刷新用量
        </button>
      </div>
    </>
  );
}

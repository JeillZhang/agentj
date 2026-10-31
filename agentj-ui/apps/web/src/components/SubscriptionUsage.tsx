// SubscriptionUsage.tsx — the read-only half of the subscription panel: the credential
// row, one row per rate-limit window and the trend sparkline. Split out of
// SubscriptionPanel so that file only keeps the fetch/link/device-code flow (and so the
// injected-snapshot mode has a single rendering path).
import { Fragment } from "react";
import { buildTrend } from "@agentj/sdk";
import type { UsageSample, UsageWindow } from "@agentj/protocol";
import UsageSparkline from "./UsageSparkline";
import { errorText, WINDOW_LABEL, WINDOW_ORDER, isWarn } from "./subscriptionModel";

type ProviderUsage = {
  windows?: UsageWindow[] | null;
  warn_percent?: number | null;
  stale?: boolean;
  error?: { kind?: string; message?: string } | null;
};
type AuthStatus = {
  authenticated?: boolean;
  credential?: { kind?: string; key_hint?: string | null; org_id?: string | null } | null;
};

function WindowRow({ item, warnPercent }: { item: UsageWindow; warnPercent?: number | null }) {
  const warn = isWarn(item, warnPercent);
  return (
    <div data-testid={`window-${item.kind}`} className={`aj-window${warn ? " aj-window--warn" : ""}`}>
      <span className="aj-window__label">{WINDOW_LABEL[item.kind] ?? item.kind}</span>
      {item.percent != null && <span className="aj-window__pct">{item.percent}%</span>}
      {item.percent !== 0 && item.resets_at && (
        <span className="aj-window__reset">重置于 {item.resets_at}</span>
      )}
    </div>
  );
}

export function CredentialRow({ status }: { status: AuthStatus }) {
  if (!status.authenticated) {
    return <p className="aj-sub__cred aj-sub__cred--muted">未认证：请使用设备码登录</p>;
  }
  const cred = status.credential ?? null;
  if (!cred) return <p className="aj-sub__cred">已认证</p>;
  return (
    <p className="aj-sub__cred">
      <span className="aj-sub__cred-kind">{cred.kind === "oauth" ? "OAuth" : "API Key"}</span>
      {cred.key_hint && <span className="aj-sub__cred-hint">{cred.key_hint}</span>}
      {cred.org_id && <span className="aj-sub__cred-org">{cred.org_id}</span>}
    </p>
  );
}

export default function SubscriptionUsage({
  usage,
  samples = [],
  now = 0,
}: {
  usage: ProviderUsage;
  samples?: UsageSample[];
  /** Epoch seconds captured when the history was loaded — never read the clock while rendering. */
  now?: number;
}) {
  const windows = Array.isArray(usage.windows) ? [...usage.windows] : [];
  windows.sort((a, b) => WINDOW_ORDER.indexOf(a.kind) - WINDOW_ORDER.indexOf(b.kind));
  const kind = usage.error?.kind;
  const sentence = errorText(kind);
  const cutoff = now > 0 ? now : Number.MAX_SAFE_INTEGER;
  return (
    <div className="aj-sub__usage">
      {usage.stale && <p className="aj-sub__stale">显示的是缓存数据</p>}
      {windows.map((item) => {
        const points = buildTrend(samples, item.kind, cutoff);
        return (
          <Fragment key={item.kind}>
            <WindowRow item={item} warnPercent={usage.warn_percent} />
            {points.length >= 2 ? (
              <UsageSparkline points={points} />
            ) : (
              <p className="aj-sub__hint">趋势需要至少两次取样</p>
            )}
          </Fragment>
        );
      })}
      {sentence && <p className="aj-sub__error">{sentence}</p>}
      {usage.error?.message && usage.error.message !== sentence && (
        <p className="aj-sub__error aj-sub__error--raw">{usage.error.message}</p>
      )}
    </div>
  );
}

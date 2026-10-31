// SubscriptionStatus.tsx — topbar subscription light with a hover/focus detail panel.
// Mirrors ConnectionStatus: the same .aj-status-* scaffolding (24px hit area, 8px dot,
// 6px bridge into the tooltip), graded by the hook's worst-window state. The usage
// numbers themselves stay in the settings panel (SubscriptionPanel).
import { useState } from "react";
import { SUBSCRIPTION_FRESH_MS } from "../hooks/useSubscriptionStatus";
import type { SubscriptionStatusData } from "../hooks/useSubscriptionStatus";
import { ERROR_TEXT, STATE_TEXT, WINDOW_LABEL, formatUsageAge, sortWindows } from "./subscriptionModel";
import "./subscriptionPanel.css";

type Props = {
  status: SubscriptionStatusData;
  /** Opens the settings dialog on the 订阅 tab (offered when no credential is linked). */
  onOpenSettings?: () => void;
};

export default function SubscriptionStatus({ status, onOpenSettings }: Props) {
  const [open, setOpen] = useState(false);
  // The age label is only meaningful while the tooltip is open, so the clock is read
  // in this handler — never during render.
  const [openedAt, setOpenedAt] = useState(0);

  const show = (next: boolean) => {
    setOpen(next);
    if (next) setOpenedAt(Date.now());
  };

  const windows = sortWindows(status.windows);
  const age = open && openedAt > 0 && status.fetchedAt > 0 ? openedAt - status.fetchedAt : null;
  const lagging = status.stale || (age !== null && age > SUBSCRIPTION_FRESH_MS);
  const needsCredential = status.state === "unknown" || status.state === "invalid";
  // A known kind has an actionable sentence; the raw RPC text stays below it so the cause is
  // still diagnosable. Unknown kinds/free-form errors render the raw text alone.
  const reason = status.errorKind ? ERROR_TEXT[status.errorKind] : undefined;

  return (
    <div
      className="aj-status-hover aj-sub-status"
      data-testid="subscription-status"
      onMouseEnter={() => show(true)}
      onMouseLeave={() => show(false)}
      // Focus leaving the trigger for the panel (Tab, or mousedown on the CTA below) must
      // not unmount the panel — that would swallow the click before it lands.
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) show(false);
      }}
    >
      <button
        type="button"
        className="aj-status-trigger"
        data-testid="sub-trigger"
        aria-label={`订阅用量：${STATE_TEXT[status.state]}`}
        aria-haspopup="true"
        aria-expanded={open}
        onFocus={() => show(true)}
      >
        <span
          className="aj-status-light aj-sub-status__dot"
          data-testid="sub-dot"
          data-state={status.state}
          data-stale={lagging ? "true" : "false"}
          aria-hidden="true"
        />
      </button>
      {open && (
        <div className="aj-status-panel aj-sub-status__panel" role="tooltip" aria-label="订阅用量">
          <span className="aj-status-row">
            <span className="aj-status-dot" data-state={status.state} aria-hidden="true" />
            <span className="aj-status-text">{STATE_TEXT[status.state]}</span>
          </span>
          {windows.map((item) => {
            const warn =
              typeof item.percent === "number" &&
              typeof status.warnPercent === "number" &&
              item.percent >= status.warnPercent;
            return (
              <span
                key={item.kind}
                className="aj-status-row aj-sub-status__row"
                data-testid={`sub-row-${item.kind}`}
                data-state={warn ? "warn" : undefined}
              >
                <span className="aj-status-text">{WINDOW_LABEL[item.kind] ?? item.kind}</span>
                {typeof item.percent === "number" && <span>{item.percent}%</span>}
              </span>
            );
          })}
          {age !== null && (
            <span className="aj-status-row aj-sub-status__age" data-testid="sub-age">
              {`${lagging ? "~" : ""}${formatUsageAge(age)}`}
            </span>
          )}
          {status.error && (
            <span className="aj-status-row aj-sub-status__error" data-testid="sub-error">
              {reason ?? status.error}
            </span>
          )}
          {status.error && reason && (
            <span className="aj-status-row aj-sub-status__error aj-sub-status__error--raw">
              {status.error}
            </span>
          )}
          {needsCredential && (
            <>
            <button
              type="button"
              className="aj-sub__btn aj-sub-status__link"
              onClick={() => {
                onOpenSettings?.();
                show(false);
              }}
            >
              设置 → 订阅
            </button>
            <span className="aj-sub-status__keys" data-testid="sub-shortcut-hint">
              <kbd>u</kbd> 用量设置
            </span>
            </>
          )}
        </div>
      )}
    </div>
  );
}

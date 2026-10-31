// subscriptionModel.ts — pure helpers shared by the panel, the topbar light and their
// tests. Kept out of the .tsx files so component modules only export components
// (oxlint only-export-components) and the formatters stay DOM-free.
import type { UsageWindow } from "@agentj/protocol";
import type { SubscriptionState } from "../hooks/useSubscriptionStatus";

/** Human labels for the three rate-limit windows. */
export const WINDOW_LABEL: Record<string, string> = {
  rolling: "5 小时",
  weekly: "本周",
  monthly: "本月",
};

/** Display order of the windows (upstream sends them in whatever order it likes). */
export const WINDOW_ORDER = ["rolling", "weekly", "monthly"];

/** One sentence per upstream error kind; the panel and the light share this table. */
export const ERROR_TEXT: Record<string, string> = {
  no_credential: "尚未登录：点击下方按钮用设备码登录",
  expired: "订阅凭据已失效：请重新登录",
  unauthorized: "密钥无效或已被撤销：请重新登录",
  entitlement: "该工作区没有 OpenCode Go 订阅",
  unavailable: "用量暂时无法获取（上游响应无法识别）",
  // Not an upstream verdict: the request to the kernel itself failed (RPC error, connection
  // gone, rejected params). Kept apart from `unavailable` so a broken request never reads as
  // "upstream response unrecognised".
  probe_failed: "无法从内核读取订阅状态：请检查连接后重试",
  network: "网络不可达，已显示缓存数据",
};

/**
 * The mapped sentence for a wire error kind, or `undefined` when the kind is unknown.
 *
 * Own-property check on purpose: the kind comes straight off the wire, so a value like
 * `"toString"` or `"constructor"` must not resolve to an inherited `Object.prototype`
 * member (that would hand a function to React as a child). Unknown kinds fall back to the
 * raw RPC message at the call sites instead of disappearing.
 */
export function errorText(kind: string | null | undefined): string | undefined {
  return kind && Object.hasOwn(ERROR_TEXT, kind) ? ERROR_TEXT[kind] : undefined;
}

/** One sentence per graded/trouble state; `unknown` is the "no credential" case. */
export const STATE_TEXT: Record<SubscriptionState, string> = {
  ok: "额度充足",
  warn: "接近上限",
  error: "额度用尽或获取失败",
  unknown: "未链接凭据",
  invalid: "凭据无效：请重新订阅",
};

/**
 * One sentence per device-code poll outcome; `null` means "keep polling". Moved out of the
 * panel so the literals live with the other user-facing text.
 */
export function deviceStateLabel(state: string): string | null {
  if (state === "authorized") return "登录成功";
  if (state === "expired") return "设备码已过期，请重新登录";
  if (state === "denied") return "设备码登录被拒绝";
  if (state === "pending" || state === "slow_down") return null;
  return "设备码登录未完成，请重试";
}

/**
 * Coarse, human age of a usage payload. Deliberately coarse: the freshness contract
 * only distinguishes "just now" from "minutes" from "hours", so no clock is needed
 * beyond the caller's own read.
 */
export function formatUsageAge(ms: number): string {
  const minutes = Math.floor(Math.max(0, ms) / 60_000);
  if (minutes < 1) return "<1 分钟";
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours} 小时前`;
  return `${Math.floor(hours / 24)} 天前`;
}

/** Sort a window list into the canonical display order (unknown kinds last, stable). */
export function sortWindows<T extends { kind: string }>(windows: T[]): T[] {
  const rank = (kind: string) => {
    const index = WINDOW_ORDER.indexOf(kind);
    return index === -1 ? WINDOW_ORDER.length : index;
  };
  return [...windows].sort((a, b) => rank(a.kind) - rank(b.kind));
}

/** A window is "hot" when upstream rate-limited it or it crossed the warn threshold. */
export function isWarn(item: UsageWindow, warnPercent: number | null | undefined): boolean {
  if (item.status === "rate-limited") return true;
  return typeof warnPercent === "number" && (item.percent ?? 0) >= warnPercent;
}

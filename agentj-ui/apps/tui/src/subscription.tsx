import { Fragment, useEffect, useRef, useState } from "react";
import { useKeyboard } from "@opentui/react";
import { METHODS, buildTrend, sparkline } from "@agentj/sdk";
import type { RpcClient, UsageSampleLike } from "@agentj/sdk";

/** Upstream window keys → the human label shown on each row (mirrors the Web panel). */
export const WINDOW_LABEL: Record<string, string> = {
  rolling: "5 小时",
  weekly: "本周",
  monthly: "本月",
};

/** Domain error kinds → an actionable sentence; keeps parity with the Web panel. */
export const ERROR_TEXT: Record<string, string> = {
  no_credential: "尚未登录：按 l 用设备码登录",
  expired: "订阅凭据已失效：请重新登录",
  unauthorized: "密钥无效或已被撤销：请重新登录",
  entitlement: "该工作区没有 OpenCode Go 订阅",
  unavailable: "用量暂时无法获取（上游响应无法识别）",
  network: "网络不可达，已显示缓存数据",
};

/** Link outcome → the sentence shown under the key input; mirrors the Web panel. */
export const OUTCOME_TEXT: Record<string, string> = {
  verified: "已链接：密钥已保存并校验通过",
  unverified: "网络不可达，已保存但未校验",
};

type UsageWindow = { kind: string; status?: string; percent?: number };
type UsageError = { kind: string; message?: string };
type UsageResult = {
  stale?: boolean;
  warn_percent?: number;
  windows?: UsageWindow[];
  error?: UsageError | null;
};
type AuthStatus = {
  authenticated?: boolean;
  credential?: { key_hint?: string; expiring_soon?: boolean } | null;
};
/** `auth/setApiKey` result projection; never carries the full key (see CredentialInfo). */
type LinkResult = {
  saved?: boolean;
  outcome?: string;
  effective_source?: string;
  error?: UsageError | null;
};
type DeviceStart = {
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in?: number;
  interval?: number;
};
type DevicePoll =
  | { state: "pending" }
  | { state: "slow_down"; interval?: number }
  | { state: "authorized"; credential?: unknown }
  | { state: "expired" | "denied"; message?: string };

export type SubscriptionOverlayProps = {
  client: RpcClient;
  onClose: () => void;
  /**
   * Fired once per successful credential change (link saved / credential cleared / device
   * login), so the host can re-read the served model catalog. Never fired for a rejected link.
   */
  onCredentialChange?: () => void;
  /** Injectable delay so the polling cadence is deterministic in tests (defaults to setTimeout). */
  schedule?: (fn: () => void, ms: number) => void;
};

function errorSentence(err: UsageError | null | undefined, fallback: string): string {
  if (err && ERROR_TEXT[err.kind]) return ERROR_TEXT[err.kind];
  if (err && typeof err.message === "string" && err.message.length > 0) return err.message;
  return fallback;
}

function outcomeSentence(res: LinkResult): string {
  if (res.outcome === "rejected") {
    // A rejection never saves; keep the actionable reason from the shared table.
    const known = res.error?.kind ? ERROR_TEXT[res.error.kind] : undefined;
    return `未保存：${known ?? res.error?.message ?? "API Key 无效或已被撤销"}`;
  }
  return OUTCOME_TEXT[res.outcome ?? ""] ?? "链接结果未知：请刷新查看";
}

function windowLine(w: UsageWindow): string {
  const label = WINDOW_LABEL[w.kind] ?? w.kind;
  // `percent` is optional on the wire (upstream may omit it or send a non-integer);
  // never fabricate a number — render an explicit unknown marker instead.
  const pct = typeof w.percent === "number" ? `${w.percent}%` : "—";
  const limited = w.status === "rate-limited" ? " 已限流" : "";
  return `${label}  ${pct}${limited}`;
}

/** Glyph budget of the trend line; the text pane is narrow, so cap it hard. */
const TREND_WIDTH = 16;

/**
 * One-line trend for a single window kind: `kind  ▁▃  10%→30%`. Blank below two samples —
 * a lone point is not a trend. Ranking and glyph bands come from the SDK helpers so the
 * TUI, the Web panel and the kernel agree on what a sample means.
 */
export function trendLine(kind: string, samples: UsageSampleLike[]): string {
  const points = buildTrend(samples, kind, Date.now(), TREND_WIDTH);
  if (points.length < 2) return "";
  const glyphs = sparkline(
    points.map((p) => p.percent),
    TREND_WIDTH,
  );
  const first = points[0].percent;
  const last = points[points.length - 1].percent;
  return `${kind}  ${glyphs}  ${first}%→${last}%`;
}

export function SubscriptionOverlay(props: SubscriptionOverlayProps): React.ReactNode {
  const { client, onClose, onCredentialChange } = props;
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const schedule =
    props.schedule ??
    ((fn: () => void, ms: number) => {
      timerRef.current = setTimeout(fn, ms);
    });
  const [usage, setUsage] = useState<UsageResult | null>(null);
  const [samples, setSamples] = useState<UsageSampleLike[]>([]);
  const [status, setStatus] = useState<AuthStatus | null>(null);
  const [device, setDevice] = useState<{ user_code: string; verification_uri: string } | null>(null);
  const [deviceMsg, setDeviceMsg] = useState<string | null>(null);
  const [linkMode, setLinkMode] = useState(false);
  const [keyBuf, setKeyBuf] = useState("");
  const [linkMsg, setLinkMsg] = useState<string | null>(null);
  const [envShadow, setEnvShadow] = useState(false);
  const cancelled = useRef(false);

  // The server defaults `since` to now - retention_days, so the window is never
  // hardcoded here; a failed history read just means no trend line is drawn.
  const loadHistory = async (): Promise<void> => {
    try {
      const h = (await client.request(METHODS.providerUsageHistory, {})) as
        | { samples?: UsageSampleLike[] }
        | undefined;
      if (!cancelled.current) setSamples(h?.samples ?? []);
    } catch {
      if (!cancelled.current) setSamples([]);
    }
  };

  const refreshAll = async (): Promise<void> => {
    try {
      const s = (await client.request(METHODS.authStatus)) as AuthStatus;
      if (!cancelled.current) setStatus(s);
    } catch {}
    try {
      await client.request(METHODS.modelList, { refresh: true });
    } catch {}
    try {
      const u = (await client.request(METHODS.providerUsage, { refresh: true })) as UsageResult;
      if (!cancelled.current) setUsage(u);
    } catch {}
    await loadHistory();
  };

  useEffect(() => {
    cancelled.current = false;
    void (async () => {
      try {
        const s = (await client.request(METHODS.authStatus)) as AuthStatus;
        if (cancelled.current) return;
        setStatus(s);
      } catch {}
      try {
        const u = (await client.request(METHODS.providerUsage, { refresh: false })) as UsageResult;
        if (cancelled.current) return;
        setUsage(u);
      } catch {}
      await loadHistory();
    })();
    return () => {
      cancelled.current = true;
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, [client]);

  // Each tick re-schedules itself; `cancelled` (set by the unmount cleanup) stops
  // a pending timer from issuing further auth/devicePoll requests.
  const startPoll = (code: string, interval: number): void => {
    const tick = async (): Promise<void> => {
      if (cancelled.current) return;
      let reply: DevicePoll;
      try {
        reply = (await client.request(METHODS.authDevicePoll, { device_code: code })) as DevicePoll;
      } catch (e) {
        if (!cancelled.current) setDeviceMsg(e instanceof Error ? e.message : String(e));
        return;
      }
      if (cancelled.current) return;
      if (reply.state === "pending") {
        schedule(tick, interval * 1000);
        return;
      }
      if (reply.state === "slow_down") {
        schedule(tick, (reply.interval ?? interval + 5) * 1000);
        return;
      }
      if (reply.state === "authorized") {
        await refreshAll();
        if (!cancelled.current) {
          // A device login replaces the credential just like a link does.
          onCredentialChange?.();
          setDevice(null);
        }
        return;
      }
      if (reply.state === "expired") setDeviceMsg("设备码已过期：请重新登录");
      else if (reply.state === "denied") setDeviceMsg("登录被拒绝：请重试");
      else setDeviceMsg(reply.message ?? "设备码登录失败");
    };
    schedule(tick, interval * 1000);
  };

  const startDevice = async (): Promise<void> => {
    setDeviceMsg(null);
    try {
      const d = (await client.request(METHODS.authDeviceStart)) as DeviceStart;
      if (cancelled.current) return;
      setDevice({ user_code: d.user_code, verification_uri: d.verification_uri });
      startPoll(d.device_code, d.interval ?? 5);
    } catch (e) {
      if (!cancelled.current) setDeviceMsg(e instanceof Error ? e.message : String(e));
    }
  };

  // The typed key never enters the render tree: it is dropped before the request,
  // so only the mask below is ever visible in a frame.
  const submitKey = async (): Promise<void> => {
    const apiKey = keyBuf;
    if (apiKey.trim().length === 0) {
      setLinkMsg("请输入 API Key");
      return;
    }
    setKeyBuf("");
    setLinkMode(false);
    let res: LinkResult;
    try {
      res = (await client.request(METHODS.authSetApiKey, {
        provider: "opencode-go",
        api_key: apiKey,
        label: "tui",
      })) as LinkResult;
    } catch (e) {
      if (!cancelled.current) setLinkMsg(e instanceof Error ? e.message : String(e));
      return;
    }
    if (cancelled.current) return;
    // A rejection never saves, so only a saved credential can change the served catalog.
    if (res.saved === true) onCredentialChange?.();
    setLinkMsg(outcomeSentence(res));
    setEnvShadow(res.saved === true && res.effective_source === "env");
  };

  const clearCredential = async (): Promise<void> => {
    try {
      await client.request(METHODS.authClearCredential, { provider: "opencode-go" });
    } catch (e) {
      if (!cancelled.current) setLinkMsg(e instanceof Error ? e.message : String(e));
      return;
    }
    await refreshAll();
    if (!cancelled.current) onCredentialChange?.();
  };

  useKeyboard((key) => {
    const name = (key.name ?? "").toLowerCase();
    // Link mode owns the keys first: Esc cancels the input instead of closing the overlay.
    if (linkMode) {
      if (name === "escape" || name === "esc") {
        setLinkMode(false);
        setKeyBuf("");
        return;
      }
      if (name === "return" || name === "enter" || key.sequence === "\r") {
        void submitKey();
        return;
      }
      if (name === "backspace") {
        setKeyBuf((v) => v.slice(0, -1));
        return;
      }
      const ch = key.sequence ?? "";
      if (ch.length === 1 && ch >= " " && !key.ctrl && !key.meta && !key.super) {
        setKeyBuf((v) => v + ch);
      }
      return;
    }
    if (name === "escape") {
      onClose();
      return;
    }
    if (name === "l") void startDevice();
    else if (name === "i") {
      setLinkMsg(null);
      setEnvShadow(false);
      setLinkMode(true);
    } else if (name === "c") void clearCredential();
  });

  const errorLine = usage?.error ? errorSentence(usage.error, "用量错误") : null;
  return (
    <box flexDirection="column" border padding={1}>
      <text>订阅</text>
      {status?.credential?.key_hint ? <text>{status.credential.key_hint}</text> : null}
      {(usage?.windows ?? []).map((w, i) => {
        const trend = trendLine(w.kind, samples);
        return (
          <Fragment key={i}>
            <text>{windowLine(w)}</text>
            {trend.length > 0 ? <text>{trend}</text> : null}
          </Fragment>
        );
      })}
      {usage?.stale ? <text>（缓存数据）</text> : null}
      {errorLine ? <text>{errorLine}</text> : null}
      {device ? <text>{device.user_code}</text> : null}
      {device ? <text>{device.verification_uri}</text> : null}
      {deviceMsg ? <text>{deviceMsg}</text> : null}
      {linkMode ? <text>{`API Key: ${"•".repeat(keyBuf.length)}`}</text> : null}
      {linkMsg ? <text>{linkMsg}</text> : null}
      {envShadow ? <text>环境变量当前优先，链接的密钥暂不生效</text> : null}
      <text>i 链接 API Key · c 解除链接 · l 设备码登录 · Esc 关闭</text>
      <text>链接的 API Key 只保存在所连接内核主机的 auth.json 中</text>
    </box>
  );
}

import { useEffect, useRef, useState } from "react";
import { useKeyboard } from "@opentui/react";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";

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
  /** Injectable delay so the polling cadence is deterministic in tests (defaults to setTimeout). */
  schedule?: (fn: () => void, ms: number) => void;
};

function errorSentence(err: UsageError | null | undefined, fallback: string): string {
  if (err && ERROR_TEXT[err.kind]) return ERROR_TEXT[err.kind];
  if (err && typeof err.message === "string" && err.message.length > 0) return err.message;
  return fallback;
}

function windowLine(w: UsageWindow): string {
  const label = WINDOW_LABEL[w.kind] ?? w.kind;
  // `percent` is optional on the wire (upstream may omit it or send a non-integer);
  // never fabricate a number — render an explicit unknown marker instead.
  const pct = typeof w.percent === "number" ? `${w.percent}%` : "—";
  const limited = w.status === "rate-limited" ? " 已限流" : "";
  return `${label}  ${pct}${limited}`;
}

export function SubscriptionOverlay(props: SubscriptionOverlayProps): React.ReactNode {
  const { client, onClose } = props;
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const schedule =
    props.schedule ??
    ((fn: () => void, ms: number) => {
      timerRef.current = setTimeout(fn, ms);
    });
  const [usage, setUsage] = useState<UsageResult | null>(null);
  const [status, setStatus] = useState<AuthStatus | null>(null);
  const [device, setDevice] = useState<{ user_code: string; verification_uri: string } | null>(null);
  const [deviceMsg, setDeviceMsg] = useState<string | null>(null);
  const cancelled = useRef(false);

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
        if (!cancelled.current) setDevice(null);
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

  useKeyboard((key) => {
    const name = (key.name ?? "").toLowerCase();
    if (name === "escape") {
      onClose();
      return;
    }
    if (name === "l") void startDevice();
  });

  const errorLine = usage?.error ? errorSentence(usage.error, "用量错误") : null;
  return (
    <box flexDirection="column" border padding={1}>
      <text>订阅</text>
      {status?.credential?.key_hint ? <text>{status.credential.key_hint}</text> : null}
      {(usage?.windows ?? []).map((w, i) => (
        <text key={i}>{windowLine(w)}</text>
      ))}
      {usage?.stale ? <text>（缓存数据）</text> : null}
      {errorLine ? <text>{errorLine}</text> : null}
      {device ? <text>{device.user_code}</text> : null}
      {device ? <text>{device.verification_uri}</text> : null}
      {deviceMsg ? <text>{deviceMsg}</text> : null}
      <text>l 设备码登录 · Esc 关闭</text>
    </box>
  );
}

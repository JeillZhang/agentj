import { useEffect, useRef, useState } from "react";
import { METHODS, RpcClient } from "@agentj/sdk";
import "./subscriptionPanel.css";

type Props = { client: RpcClient; onClose: () => void };

type UsageWindow = { kind: string; status?: string; percent?: number | null; resets_at?: string | null };
type ProviderUsage = {
  windows?: UsageWindow[] | null;
  warn_percent?: number | null;
  stale?: boolean;
  error?: { kind?: string; message?: string } | null;
};
type CredentialInfo = {
  kind?: string;
  key_hint?: string | null;
  org_id?: string | null;
};
type AuthStatus = { authenticated?: boolean; credential?: CredentialInfo | null };
type DeviceStart = { device_code: string; user_code: string; verification_uri: string; interval: number };
type DevicePoll = { state: string; interval?: number | null };
type DeviceView = { user_code: string; verification_uri: string };

const ERROR_TEXT: Record<string, string> = {
  no_credential: "尚未登录：点击下方按钮用设备码登录",
  expired: "订阅凭据已失效：请重新登录",
  unauthorized: "密钥无效或已被撤销：请重新登录",
  entitlement: "该工作区没有 OpenCode Go 订阅",
  unavailable: "用量暂时无法获取（上游响应无法识别）",
  network: "网络不可达，已显示缓存数据",
};
const WINDOW_LABEL: Record<string, string> = { rolling: "5 小时", weekly: "本周", monthly: "本月" };
const WINDOW_ORDER = ["rolling", "weekly", "monthly"];

function isWarn(item: UsageWindow, warnPercent: number | null | undefined): boolean {
  if (item.status === "rate-limited") return true;
  return typeof warnPercent === "number" && (item.percent ?? 0) >= warnPercent;
}

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

function CredentialRow({ status }: { status: AuthStatus }) {
  if (!status.authenticated) {
    return <p className="aj-sub__cred aj-sub__cred--muted">未认证：请使用设备码登录</p>;
  }
  const cred = status.credential ?? null;
  if (!cred) return <p className="aj-sub__cred">已认证</p>;
  return (
    <p className="aj-sub__cred">
      <span className="aj-sub__cred-kind">{cred.kind === "o_auth" ? "OAuth" : "API Key"}</span>
      {cred.key_hint && <span className="aj-sub__cred-hint">{cred.key_hint}</span>}
      {cred.org_id && <span className="aj-sub__cred-org">{cred.org_id}</span>}
    </p>
  );
}

function UsageBlock({ usage }: { usage: ProviderUsage }) {
  const windows = Array.isArray(usage.windows) ? [...usage.windows] : [];
  windows.sort((a, b) => WINDOW_ORDER.indexOf(a.kind) - WINDOW_ORDER.indexOf(b.kind));
  const kind = usage.error?.kind;
  const sentence = kind ? ERROR_TEXT[kind] : undefined;
  return (
    <div className="aj-sub__usage">
      {usage.stale && <p className="aj-sub__stale">显示的是缓存数据</p>}
      {windows.map((item) => (
        <WindowRow key={item.kind} item={item} warnPercent={usage.warn_percent} />
      ))}
      {sentence && <p className="aj-sub__error">{sentence}</p>}
    </div>
  );
}

export default function SubscriptionPanel({ client, onClose }: Props) {
  const [status, setStatus] = useState<AuthStatus | null>(null);
  const [usage, setUsage] = useState<ProviderUsage | null>(null);
  const [device, setDevice] = useState<DeviceView | null>(null);
  const [deviceMsg, setDeviceMsg] = useState<string | null>(null);
  const aliveRef = useRef(true);
  const timerRef = useRef<number | null>(null);

  const clearTimer = () => {
    if (timerRef.current != null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  };

  const loadUsage = async (refresh: boolean) => {
    try {
      const res = (await client.request(METHODS.providerUsage, { refresh })) as ProviderUsage;
      if (aliveRef.current) setUsage(res);
    } catch {
      /* keep the previous snapshot on a transport failure */
    }
  };

  const loadStatus = async () => {
    try {
      const res = (await client.request(METHODS.authStatus, {})) as AuthStatus;
      if (aliveRef.current) setStatus(res);
    } catch {
      /* keep the previous status on a transport failure */
    }
  };

  const schedulePoll = (code: string, seconds: number) => {
    clearTimer();
    timerRef.current = window.setTimeout(() => {
      timerRef.current = null;
      void poll(code, seconds);
    }, Math.max(1, seconds) * 1000);
  };

  const poll = async (code: string, seconds: number) => {
    let res: DevicePoll;
    try {
      res = (await client.request(METHODS.authDevicePoll, { device_code: code })) as DevicePoll;
    } catch {
      if (aliveRef.current) setDeviceMsg("设备码轮询失败，请重试");
      return;
    }
    if (!aliveRef.current) return;
    if (res.state === "pending") return schedulePoll(code, seconds);
    if (res.state === "slow_down") return schedulePoll(code, res.interval ?? seconds);
    setDevice(null);
    if (res.state === "authorized") {
      setDeviceMsg("登录成功");
      void loadStatus();
      void client.request(METHODS.modelList, { refresh: true });
      void loadUsage(true);
      return;
    }
    if (res.state === "expired") setDeviceMsg("设备码已过期，请重新登录");
    else if (res.state === "denied") setDeviceMsg("设备码登录被拒绝");
    else setDeviceMsg("设备码登录未完成，请重试");
  };

  const startDevice = async () => {
    setDeviceMsg(null);
    let start: DeviceStart;
    try {
      start = (await client.request(METHODS.authDeviceStart, {})) as DeviceStart;
    } catch {
      if (aliveRef.current) setDeviceMsg("无法启动设备码登录，请重试");
      return;
    }
    if (!aliveRef.current) return;
    setDevice({ user_code: start.user_code, verification_uri: start.verification_uri });
    schedulePoll(start.device_code, start.interval);
  };

  useEffect(() => {
    aliveRef.current = true;
    void loadStatus();
    void loadUsage(false);
    return () => {
      aliveRef.current = false;
      clearTimer();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="aj-sub">
      <div className="aj-sub__head">
        <span className="aj-sub__title">OpenCode Go 订阅</span>
        <button type="button" className="aj-sub__btn" onClick={onClose}>
          关闭
        </button>
      </div>
      {status && <CredentialRow status={status} />}
      {usage ? <UsageBlock usage={usage} /> : <p className="aj-sub__hint">正在获取用量…</p>}
      <div className="aj-sub__actions">
        <button type="button" className="aj-sub__btn" onClick={() => void startDevice()}>
          设备码登录
        </button>
        <button type="button" className="aj-sub__btn" onClick={() => void loadUsage(true)}>
          刷新用量
        </button>
      </div>
      {device && (
        <div className="aj-sub__device">
          <p>
            请在浏览器打开 <a href={device.verification_uri}>{device.verification_uri}</a>
          </p>
          <p>
            并输入设备码 <span className="aj-sub__code">{device.user_code}</span>
          </p>
        </div>
      )}
      {deviceMsg && <p className="aj-sub__msg">{deviceMsg}</p>}
    </div>
  );
}

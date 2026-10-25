import { Fragment, useEffect, useRef, useState } from "react";
import { METHODS, RpcClient, buildTrend } from "@agentj/sdk";
import type { AuthSetApiKeyResult, ProviderUsageHistoryResult, UsageSample, UsageWindow } from "@agentj/protocol";
import UsageSparkline from "./UsageSparkline";
import "./subscriptionPanel.css";

type Props = {
  client: RpcClient;
  onClose: () => void;
  onLinked?: (result: AuthSetApiKeyResult) => void;
  /** Fired once the kernel removed a stored credential, so the shell drops its catalog. */
  onCleared?: () => void;
};

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
const LINK_OUTCOME_TEXT: Record<string, string> = {
  verified: "已链接：密钥已保存并验证通过",
  unverified: "密钥已保存，但未校验：上游暂时无法确认，首次使用时才会暴露问题",
};

// A rejected link carries the upstream reason as a Kind the shared ERROR_TEXT table already maps.
function linkResultText(res: AuthSetApiKeyResult): string {
  if (res.outcome !== "rejected") {
    return LINK_OUTCOME_TEXT[res.outcome] ?? "链接结果未知，请查看用量与凭据状态";
  }
  const why = res.error?.kind ? ERROR_TEXT[res.error.kind] : undefined;
  return `未保存：${why ?? "API Key 被上游拒绝"}`;
}

function linkClearText(cleared: boolean): string {
  return cleared ? "已解除链接；模型列表回退到公开目录" : "当前没有已保存的凭据";
}

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
      <span className="aj-sub__cred-kind">{cred.kind === "oauth" ? "OAuth" : "API Key"}</span>
      {cred.key_hint && <span className="aj-sub__cred-hint">{cred.key_hint}</span>}
      {cred.org_id && <span className="aj-sub__cred-org">{cred.org_id}</span>}
    </p>
  );
}

function UsageBlock({
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
  const sentence = kind ? ERROR_TEXT[kind] : undefined;
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
    </div>
  );
}

export default function SubscriptionPanel({ client, onClose, onLinked, onCleared }: Props) {
  const [status, setStatus] = useState<AuthStatus | null>(null);
  const [usage, setUsage] = useState<ProviderUsage | null>(null);
  const [samples, setSamples] = useState<UsageSample[]>([]);
  const [historyAt, setHistoryAt] = useState(0);
  const [device, setDevice] = useState<DeviceView | null>(null);
  const [deviceMsg, setDeviceMsg] = useState<string | null>(null);
  const [key, setKey] = useState("");
  const [linkMsg, setLinkMsg] = useState<string | null>(null);
  const [envShadow, setEnvShadow] = useState(false);
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

  // An empty params object means "default provider, full retention window": the server
  // defaults `since` from its own retention (T03), so the panel must not hardcode a window.
  const loadHistory = async () => {
    let res: ProviderUsageHistoryResult | undefined;
    try {
      res = (await client.request(METHODS.providerUsageHistory,
        {})) as ProviderUsageHistoryResult | undefined;
    } catch {
      if (aliveRef.current) setSamples([]);
      return;
    }
    if (!aliveRef.current) return;
    setSamples(Array.isArray(res?.samples) ? res.samples : []);
    // `UsageSample.at` is epoch seconds, so the cutoff uses the same unit.
    setHistoryAt(Math.floor(Date.now() / 1000));
  };

  const refreshUsage = () => {
    // Serialise the two calls: the live fetch may write a fresh sample, and the history read
    // has to see it or the chart lags one refresh behind.
    void loadUsage(true).then(() => loadHistory());
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
      refreshUsage();
      return;
    }
    if (res.state === "expired") setDeviceMsg("设备码已过期，请重新登录");
    else if (res.state === "denied") setDeviceMsg("设备码登录被拒绝");
    else setDeviceMsg("设备码登录未完成，请重试");
  };

  const link = async () => {
    const apiKey = key.trim();
    if (apiKey.length === 0) return;
    let res: AuthSetApiKeyResult;
    try {
      res = (await client.request(METHODS.authSetApiKey,
        { provider: "opencode-go", api_key: apiKey, label: "web" })) as AuthSetApiKeyResult;
    } catch {
      if (aliveRef.current) {
        setKey("");
        setLinkMsg("链接失败：请检查内核连接后重试");
      }
      return;
    }
    if (!aliveRef.current) return;
    setKey("");
    setLinkMsg(linkResultText(res));
    setEnvShadow(res.saved && res.effective_source === "env");
    if (!res.saved) return;
    void loadStatus();
    if (Array.isArray(res.models) && res.models.length > 0) onLinked?.(res);
  };

  const clear = async () => {
    let cleared = false;
    try {
      const res = (await client.request(METHODS.authClearCredential,
        { provider: "opencode-go" })) as { cleared?: boolean };
      cleared = res.cleared === true;
    } catch {
      if (aliveRef.current) setLinkMsg("解除链接失败：请检查内核连接后重试");
      return;
    }
    if (aliveRef.current) {
      setLinkMsg(linkClearText(cleared));
      setEnvShadow(false);
    }
    // Reported even if the panel unmounted mid-request: the kernel did drop the credential,
    // so the shell must not keep serving the account catalog.
    if (cleared) onCleared?.();
    void loadStatus();
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
    void loadHistory();
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
      {usage ? (
        <UsageBlock usage={usage} samples={samples} now={historyAt} />
      ) : (
        <p className="aj-sub__hint">正在获取用量…</p>
      )}
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
        {status?.credential?.kind === "oauth" && (
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
        <button type="button" className="aj-sub__btn" onClick={refreshUsage}>
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

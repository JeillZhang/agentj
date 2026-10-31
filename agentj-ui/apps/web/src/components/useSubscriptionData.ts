import { useEffect, useRef, useState } from "react";
import { METHODS, RpcClient } from "@agentj/sdk";
import type {
  AuthSetApiKeyResult,
  ProviderUsageHistoryResult,
  UsageSample,
  UsageWindow,
} from "@agentj/protocol";
import { ERROR_TEXT, deviceStateLabel } from "./subscriptionModel";
import type { SubscriptionStatusData } from "../hooks/useSubscriptionStatus";

/**
 * The account surface's data layer: credential status, the usage snapshot (own or
 * injected), the retention history, the device-code flow and the two write paths.
 * `SubscriptionPanel` renders it; the shapes here are exported because the display
 * components take them as props (`SubscriptionHistory`, `SubscriptionCredentialCard`).
 */
export type ProviderUsage = {
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
export type AuthStatus = { authenticated?: boolean; credential?: CredentialInfo | null };
type DeviceStart = {
  device_code: string;
  user_code: string;
  verification_uri: string;
  interval: number;
};
type DevicePoll = { state: string; interval?: number | null };
export type DeviceView = { user_code: string; verification_uri: string };

const LINK_OUTCOME_TEXT: Record<string, string> = {
  verified: "已链接：密钥已保存并验证通过",
  unverified: "密钥已保存，但未校验：上游暂时无法确认，首次使用时才会暴露问题",
};

/** Project the shell's snapshot into the shape the read-only usage block renders. */
function usageFromStatus(status: SubscriptionStatusData): ProviderUsage {
  return {
    windows: status.windows,
    warn_percent: status.warnPercent,
    stale: status.stale,
    error: status.error ? { kind: status.errorKind ?? "unavailable", message: status.error } : null,
  };
}

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

export type SubscriptionDataCallbacks = {
  /** Fired with the result only when the link saved an account catalog (panel :186). */
  onLinked?: (result: AuthSetApiKeyResult) => void;
  /** Fired after the kernel removed a credential — even if the panel already unmounted. */
  onCleared?: () => void;
};

export function useSubscriptionData(
  client: RpcClient,
  status?: SubscriptionStatusData,
  callbacks?: SubscriptionDataCallbacks,
) {
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  const [ownUsage, setOwnUsage] = useState<ProviderUsage | null>(null);
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
      if (aliveRef.current) setOwnUsage(res);
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
    if (status) {
      void status.refresh().then(() => loadHistory());
      return;
    }
    void loadUsage(true).then(() => loadHistory());
  };

  const loadStatus = async () => {
    try {
      const res = (await client.request(METHODS.authStatus, {})) as AuthStatus;
      if (aliveRef.current) setAuth(res);
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
    const label = deviceStateLabel(res.state);
    if (label !== null) setDeviceMsg(label);
    if (res.state === "authorized") {
      void loadStatus();
      void client.request(METHODS.modelList, { refresh: true });
      refreshUsage();
    }
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
    if (Array.isArray(res.models) && res.models.length > 0) callbacks?.onLinked?.(res);
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
    if (cleared) callbacks?.onCleared?.();
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
    // Injected mode: the shell's hook already owns the usage fetch, so asking twice would
    // double the upstream call on every settings open.
    if (!status) void loadUsage(false);
    void loadHistory();
    return () => {
      aliveRef.current = false;
      clearTimer();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return {
    auth,
    usage: status ? usageFromStatus(status) : ownUsage,
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
  };
}

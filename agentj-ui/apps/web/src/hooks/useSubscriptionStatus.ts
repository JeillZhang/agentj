// useSubscriptionStatus.ts — subscription/usage state for the topbar indicator and the
// settings panel. The freshness contract is explicit (see docs/guide/subscription.md):
// no interval, no timeout — a fetch happens on mount, on window focus once the data is
// older than SUBSCRIPTION_FRESH_MS, and on an explicit `refresh()`.
import { useCallback, useEffect, useRef, useState } from "react";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";
import type { UsageWindow } from "@agentj/protocol";

/** Age after which the numbers are labelled stale (5 min; matches the kernel-side cache). */
export const SUBSCRIPTION_FRESH_MS = 300_000;
/** Worst-window percentage that turns the light into a warning. */
export const SUBSCRIPTION_WARN_PERCENT = 70;
/** Worst-window percentage that reads as exhausted. */
export const SUBSCRIPTION_ERROR_PERCENT = 90;

/**
 * `ok`/`warn`/`error` grade the worst window, `unknown` means "no credential linked",
 * `invalid` means the linked credential was rejected. `invalid` is deliberately not
 * `error`: a bad key is a different problem from an unreachable usage endpoint.
 */
export type SubscriptionState = "ok" | "warn" | "error" | "unknown" | "invalid";

/** Redacted credential projection (never carries the secret). */
export type SubscriptionCredential = {
  kind?: string;
  key_hint?: string | null;
  org_id?: string | null;
};

export type SubscriptionStatusData = {
  state: SubscriptionState;
  windows: UsageWindow[];
  /** Highest numeric percent across the windows, `null` when no window reports one. */
  worstPercent: number | null;
  /** Epoch ms of the last successful usage fetch, `0` when there is none yet. */
  fetchedAt: number;
  credential: SubscriptionCredential | null;
  warnPercent: number | null;
  /** Upstream flagged the payload as cached. */
  stale: boolean;
  errorKind: string | null;
  error: string | null;
  loading: boolean;
  /** Re-runs auth/status + provider/usage and resolves once the snapshot settled. */
  refresh: () => Promise<void>;
};

type AuthStatus = { authenticated?: boolean; credential?: SubscriptionCredential | null };
type UsageResult = {
  windows?: UsageWindow[] | null;
  warn_percent?: number | null;
  stale?: boolean;
  error?: { kind?: string; message?: string } | null;
};

type Snapshot = {
  auth: "checking" | "linked";
  credential: SubscriptionCredential | null;
  windows: UsageWindow[];
  warnPercent: number | null;
  stale: boolean;
  fetchedAt: number;
  errorKind: string | null;
  error: string | null;
  loading: boolean;
};

const INITIAL: Snapshot = {
  auth: "checking",
  credential: null,
  windows: [],
  warnPercent: null,
  stale: false,
  fetchedAt: 0,
  errorKind: null,
  error: null,
  loading: false,
};

/** Credential-domain kinds ("your key is no good") as opposed to fetch failures. */
const INVALID_KINDS = ["unauthorized", "expired", "no_credential"];

function reason(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Highest numeric percent across the windows; `null` when none reports one. */
export function worstPercentOf(windows: UsageWindow[]): number | null {
  let worst: number | null = null;
  for (const item of windows) {
    const raw = item.percent;
    if (typeof raw !== "number" || !Number.isFinite(raw)) continue;
    if (worst === null || raw > worst) worst = raw;
  }
  return worst;
}

/** Thresholds shared by the light, the TUI suffix and the panel rows. */
export function severityOf(worst: number | null): "ok" | "warn" | "error" {
  if (worst === null) return "ok";
  if (worst >= SUBSCRIPTION_ERROR_PERCENT) return "error";
  if (worst >= SUBSCRIPTION_WARN_PERCENT) return "warn";
  return "ok";
}

/**
 * Live credential + usage snapshot. Every clock read and every request lives in an
 * effect or a handler, so rendering the consumer never touches `Date.now()`.
 */
export function useSubscriptionStatus(client: RpcClient | null): SubscriptionStatusData {
  const [snapshot, setSnapshot] = useState<Snapshot>(INITIAL);
  const fetchedAtRef = useRef(0);
  const seqRef = useRef(0);
  const aliveRef = useRef(true);

  const fetchAll = useCallback(async () => {
    if (!client) return;
    const seq = (seqRef.current += 1);
    const current = () => aliveRef.current && seqRef.current === seq;

    let auth: AuthStatus | null;
    try {
      auth = (await client.request(METHODS.authStatus, {})) as AuthStatus;
    } catch (e) {
      // A failing auth probe must not read as "healthy": surface it as the generic
      // error state (the credential itself is unknown, so `invalid` would be a guess).
      if (current()) {
        setSnapshot((prev) => ({
          ...prev,
          loading: false,
          errorKind: "unavailable",
          error: reason(e),
        }));
      }
      return;
    }
    if (!current()) return;

    if (!auth?.authenticated) {
      fetchedAtRef.current = 0;
      setSnapshot((prev) => ({
        ...prev,
        credential: null,
        windows: [],
        warnPercent: null,
        stale: false,
        fetchedAt: 0,
        errorKind: "no_credential",
        error: null,
        loading: false,
      }));
      return;
    }

    setSnapshot((prev) => ({
      ...prev,
      auth: "linked",
      credential: auth.credential ?? null,
      loading: true,
    }));
    try {
      const res = (await client.request(METHODS.providerUsage, { refresh: false })) as UsageResult;
      if (!current()) return;
      const stamp = Date.now();
      fetchedAtRef.current = stamp;
      setSnapshot((prev) => ({
        ...prev,
        windows: Array.isArray(res?.windows) ? res.windows : [],
        warnPercent: typeof res?.warn_percent === "number" ? res.warn_percent : null,
        stale: res?.stale === true,
        fetchedAt: stamp,
        errorKind: res?.error?.kind ?? null,
        error: res?.error?.message ?? null,
        loading: false,
      }));
    } catch (e) {
      // Keep the last windows/stamp: a broken connection is not "zero usage".
      if (!current()) return;
      setSnapshot((prev) => ({
        ...prev,
        errorKind: "network",
        error: reason(e),
        loading: false,
      }));
    }
  }, [client]);

  useEffect(() => {
    aliveRef.current = true;
    // fetchAll awaits auth/status before touching state, so nothing is set synchronously
    // here; the rule cannot see through the async boundary (same pattern as RunPanel's
    // history effect).
    // eslint-disable-next-line react/set-state-in-effect
    void fetchAll();
    return () => {
      aliveRef.current = false;
    };
  }, [fetchAll]);

  useEffect(() => {
    const onFocus = () => {
      if (Date.now() - fetchedAtRef.current < SUBSCRIPTION_FRESH_MS) return;
      void fetchAll();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [fetchAll]);

  const refresh = useCallback(() => fetchAll(), [fetchAll]);

  const worstPercent = worstPercentOf(snapshot.windows);
  const graded = severityOf(worstPercent);
  const state: SubscriptionState =
    snapshot.auth !== "linked"
      ? "unknown"
      : snapshot.errorKind === "no_credential"
        ? "unknown"
        : snapshot.errorKind !== null && INVALID_KINDS.includes(snapshot.errorKind)
          ? "invalid"
          : snapshot.errorKind !== null
            ? "error"
            : graded;

  return {
    state,
    windows: snapshot.windows,
    worstPercent,
    fetchedAt: snapshot.fetchedAt,
    credential: snapshot.credential,
    warnPercent: snapshot.warnPercent,
    stale: snapshot.stale,
    errorKind: snapshot.errorKind,
    error: snapshot.error,
    loading: snapshot.loading,
    refresh,
  };
}

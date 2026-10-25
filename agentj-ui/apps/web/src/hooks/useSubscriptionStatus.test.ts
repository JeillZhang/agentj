import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Mock } from "vitest";
import { act, renderHook } from "@testing-library/react";
import type { RpcClient } from "@agentj/sdk";
import { METHODS } from "@agentj/sdk";
import { useSubscriptionStatus, SUBSCRIPTION_FRESH_MS } from "./useSubscriptionStatus";

type UsageResult = {
  windows?: Array<{ kind: string; percent?: number }>;
  warn_percent?: number | null;
  stale?: boolean;
  error?: { kind?: string; message?: string } | null;
};

type MockClient = RpcClient & { request: Mock; calls: (m: string) => number };

function makeClient(handlers: Record<string, () => unknown>): MockClient {
  const request = vi.fn(async (method: string) => {
    const handler = handlers[method];
    return handler ? handler() : null;
  });
  const calls = (method: string) =>
    request.mock.calls.filter(([m]) => m === method).length;
  return { request, calls } as unknown as MockClient;
}

const linked = () => ({
  provider: "opencode-go",
  authenticated: true,
  models_count: 3,
  credential: { kind: "api_key", source: "file", key_hint: "…1234", expiring_soon: false },
});

const usage = (windows: Array<{ kind: string; percent?: number }>): UsageResult => ({
  warn_percent: 70,
  stale: false,
  windows,
});

/** Flush the mount fetch (one microtask per awaited request). */
async function settle(): Promise<void> {
  await act(async () => {
    for (let i = 0; i < 6; i += 1) await Promise.resolve();
  });
}

describe("useSubscriptionStatus", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("never polls on a timer", async () => {
    const client = makeClient({
      [METHODS.authStatus]: linked,
      [METHODS.providerUsage]: () => usage([{ kind: "rolling", percent: 12 }]),
    });
    renderHook(() => useSubscriptionStatus(client));
    await settle();
    const before = client.calls(METHODS.providerUsage);
    expect(before).toBe(1);
    await act(async () => {
      vi.advanceTimersByTime(600_000);
    });
    expect(client.calls(METHODS.providerUsage)).toBe(before);
  });

  it("refetches on focus only once the data is older than the freshness window", async () => {
    const client = makeClient({
      [METHODS.authStatus]: linked,
      [METHODS.providerUsage]: () => usage([{ kind: "rolling", percent: 12 }]),
    });
    const { result } = renderHook(() => useSubscriptionStatus(client));
    await settle();
    const first = result.current.fetchedAt;
    expect(first).toBeGreaterThan(0);

    vi.advanceTimersByTime(SUBSCRIPTION_FRESH_MS - 1_000);
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await Promise.resolve();
    });
    expect(client.calls(METHODS.providerUsage)).toBe(1);

    vi.advanceTimersByTime(2_000);
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      for (let i = 0; i < 6; i += 1) await Promise.resolve();
    });
    expect(client.calls(METHODS.providerUsage)).toBe(2);
    expect(result.current.fetchedAt).toBeGreaterThan(first);
  });

  it("keeps the last windows and stamp when a refetch fails", async () => {
    let fail = false;
    const client = makeClient({
      [METHODS.authStatus]: linked,
      [METHODS.providerUsage]: () => {
        if (fail) throw new Error("socket closed");
        return usage([{ kind: "rolling", percent: 40 }]);
      },
    });
    const { result } = renderHook(() => useSubscriptionStatus(client));
    await settle();
    expect(result.current.windows).toHaveLength(1);
    const stamp = result.current.fetchedAt;

    fail = true;
    vi.advanceTimersByTime(SUBSCRIPTION_FRESH_MS + 1_000);
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      for (let i = 0; i < 6; i += 1) await Promise.resolve();
    });
    expect(result.current.state).toBe("error");
    expect(result.current.error).toContain("socket closed");
    expect(result.current.windows).toHaveLength(1);
    expect(result.current.fetchedAt).toBe(stamp);
  });

  it("never asks for usage when no credential is linked", async () => {
    const client = makeClient({
      [METHODS.authStatus]: () => ({
        provider: "opencode-go",
        authenticated: false,
        models_count: 0,
      }),
    });
    const { result } = renderHook(() => useSubscriptionStatus(client));
    await settle();
    expect(client.calls(METHODS.providerUsage)).toBe(0);
    expect(result.current.state).toBe("unknown");
    expect(result.current.errorKind).toBe("no_credential");
  });

  it("separates a rejected credential from a failed fetch", async () => {
    const rejected = makeClient({
      [METHODS.authStatus]: linked,
      [METHODS.providerUsage]: () => ({
        ...usage([]),
        error: { kind: "unauthorized", message: "401" },
      }),
    });
    const invalid = renderHook(() => useSubscriptionStatus(rejected));
    await settle();
    expect(invalid.result.current.state).toBe("invalid");
    expect(invalid.result.current.errorKind).toBe("unauthorized");

    const broken = makeClient({
      [METHODS.authStatus]: linked,
      [METHODS.providerUsage]: () => ({
        ...usage([]),
        error: { kind: "unavailable", message: "upstream" },
      }),
    });
    const failed = renderHook(() => useSubscriptionStatus(broken));
    await settle();
    expect(failed.result.current.state).toBe("error");
  });

  it("grades the worst window and exposes a manual refresh", async () => {
    const client = makeClient({
      [METHODS.authStatus]: linked,
      [METHODS.providerUsage]: () => usage([{ kind: "rolling", percent: 74 }]),
    });
    const { result } = renderHook(() => useSubscriptionStatus(client));
    await settle();
    expect(result.current.state).toBe("warn");
    expect(result.current.worstPercent).toBe(74);
    await act(async () => {
      result.current.refresh();
      for (let i = 0; i < 6; i += 1) await Promise.resolve();
    });
    expect(client.calls(METHODS.providerUsage)).toBe(2);
  });

  it("reads the clock in effects, never during render", async () => {
    const client = makeClient({
      [METHODS.authStatus]: linked,
      [METHODS.providerUsage]: () => usage([{ kind: "rolling", percent: 12 }]),
    });
    const { rerender } = renderHook(() => useSubscriptionStatus(client));
    await settle();
    const now = vi.spyOn(Date, "now");
    rerender();
    expect(now).not.toHaveBeenCalled();
  });
});

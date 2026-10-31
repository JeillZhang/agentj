import "@testing-library/jest-dom/vitest";
import { describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { METHODS, type RpcClient } from "@agentj/sdk";
import type { SubscriptionStatusData } from "../hooks/useSubscriptionStatus";
import { useSubscriptionData } from "./useSubscriptionData";

const statusFixture = {
  state: "ok" as const,
  windows: [{ kind: "five_hour", percent: 12 }],
  worstPercent: 12,
  fetchedAt: 1,
  credential: null,
  warnPercent: null,
  stale: false,
  errorKind: null,
  error: null,
  loading: false,
  refresh: vi.fn(async () => {}),
} as unknown as SubscriptionStatusData;

describe("useSubscriptionData", () => {
  it("does not fetch usage itself when a status snapshot is injected", async () => {
    const request = vi.fn(async (method: string) => {
      if (method === METHODS.providerUsageHistory) return { samples: [] };
      if (method === METHODS.authStatus) return { authenticated: false };
      return {};
    });
    const client = { request } as unknown as RpcClient;
    renderHook(() => useSubscriptionData(client, statusFixture));
    await waitFor(() => expect(request).toHaveBeenCalled());
    const methods = request.mock.calls.map((call) => call[0]);
    expect(methods).not.toContain(METHODS.providerUsage);
    expect(methods).toContain(METHODS.providerUsageHistory);
    expect(methods).toContain(METHODS.authStatus);
  });

  it("stops polling the device flow after unmount", async () => {
    vi.useFakeTimers();
    const request = vi.fn(async (method: string) => {
      if (method === METHODS.authDeviceStart) {
        return {
          device_code: "dc-1",
          user_code: "UC-1",
          verification_uri: "https://example.test/device",
          interval: 5,
        };
      }
      if (method === METHODS.authDevicePoll) return { state: "pending" };
      if (method === METHODS.providerUsageHistory) return { samples: [] };
      return {};
    });
    const client = { request } as unknown as RpcClient;
    const { result, unmount } = renderHook(() => useSubscriptionData(client));
    const polls = () =>
      request.mock.calls.filter((call) => call[0] === METHODS.authDevicePoll).length;

    await act(async () => {
      await result.current.startDevice();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    expect(polls()).toBeGreaterThan(0);
    const before = polls();

    unmount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(polls()).toBe(before);
    vi.useRealTimers();
  });
});

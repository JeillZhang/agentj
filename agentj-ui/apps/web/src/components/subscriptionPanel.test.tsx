import { expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, act } from "@testing-library/react";
import type { RpcClient } from "@agentj/sdk";
import { METHODS } from "@agentj/sdk";
import SubscriptionPanel from "./SubscriptionPanel";

const client = (responses: Record<string, unknown>, sequences: Record<string, unknown[]> = {}) => {
  const requests: string[] = [];
  const request = vi.fn(async (method: string) => {
    requests.push(method);
    if (sequences[method]?.length) return sequences[method].shift();
    return responses[method];
  });
  return { request, requests } as unknown as RpcClient & { requests: string[] };
};

it("renders three windows, marks the exhausted one and shows the key hint", async () => {
  const c = client({
    [METHODS.authStatus]: { provider: "opencode-go", authenticated: true,
      credential: { kind: "api_key", source: "file", key_hint: "…1234", expiring_soon: false }, models_count: 44 },
    [METHODS.providerUsage]: { provider: "opencode-go", fetched_at: 1, stale: false, warn_percent: 80,
      windows: [{ kind: "rolling", status: "ok", percent: 4, resets_at: "2026-08-13T16:27:38Z" },
                { kind: "weekly", status: "ok", percent: 3 },
                { kind: "monthly", status: "rate-limited", percent: 100 }] },
  });
  render(<SubscriptionPanel client={c} onClose={() => {}} />);
  expect(await screen.findByText("5 小时")).toBeInTheDocument();
  expect(screen.getByText("…1234")).toBeInTheDocument();
  expect(screen.getByTestId("window-monthly")).toHaveClass("aj-window--warn");
});

it("lists each domain error without inventing percentages", async () => {
  for (const [kind, text] of [["entitlement", /没有 OpenCode Go 订阅/], ["unauthorized", /密钥无效/],
                              ["no_credential", /尚未登录/]] as const) {
    const c = client({
      [METHODS.authStatus]: { provider: "opencode-go", authenticated: kind !== "no_credential" },
      [METHODS.providerUsage]: { provider: "opencode-go", fetched_at: 1, stale: false, warn_percent: 80,
        windows: [], error: { kind, message: "m" } },
    });
    const { unmount } = render(<SubscriptionPanel client={c} onClose={() => {}} />);
    expect(await screen.findByText(text)).toBeInTheDocument();
    unmount();
  }
});

it("shows cached windows next to the stale notice for an unavailable error", async () => {
  const c = client({
    [METHODS.authStatus]: { provider: "opencode-go", authenticated: true },
    [METHODS.providerUsage]: { provider: "opencode-go", fetched_at: 1, stale: true, warn_percent: 80,
      windows: [{ kind: "rolling", status: "ok", percent: 4 }],
      error: { kind: "unavailable", message: "m" } },
  });
  render(<SubscriptionPanel client={c} onClose={() => {}} />);
  expect(await screen.findByText("5 小时")).toBeInTheDocument();
  expect(screen.getByText(/缓存/)).toBeInTheDocument();
});

it("polls the device flow, honours slow_down, refreshes and stops", async () => {
  vi.useFakeTimers();
  try {
    const c = client({
      [METHODS.authStatus]: { provider: "opencode-go", authenticated: false },
      [METHODS.providerUsage]: { provider: "opencode-go", fetched_at: 1, stale: false, warn_percent: 80, windows: [] },
      [METHODS.authDeviceStart]: { provider: "opencode-go", device_code: "dc", user_code: "ABCD-1234",
        verification_uri: "https://opencode.ai/console/device?user_code=ABCD-1234", expires_in: 600, interval: 5 },
      [METHODS.modelList]: { provider: "opencode-go", default_model: "m", source: "account", fetched_at: 1, models: [] },
    }, { [METHODS.authDevicePoll]: [{ state: "pending" }, { state: "slow_down", interval: 10 },
                                    { state: "authorized", credential: { kind: "oauth", source: "file", expiring_soon: false } }] });

    render(<SubscriptionPanel client={c} onClose={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: /设备码登录/ }));
    await act(async () => {});
    expect(screen.getByText("ABCD-1234")).toBeInTheDocument();

    await act(async () => { await vi.advanceTimersByTimeAsync(5_000); });
    await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
    await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
    expect(c.requests.filter((m) => m === METHODS.authDevicePoll)).toHaveLength(3);
    expect(c.requests).toContain(METHODS.modelList);

    const before = c.requests.length;
    await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
    expect(c.requests.length).toBe(before);
  } finally {
    vi.useRealTimers();
  }
});

it("clears the pending poll timer on unmount", async () => {
  vi.useFakeTimers();
  try {
    const c = client({
      [METHODS.authStatus]: { provider: "opencode-go", authenticated: false },
      [METHODS.providerUsage]: { provider: "opencode-go", fetched_at: 1, stale: false, warn_percent: 80, windows: [] },
      [METHODS.authDeviceStart]: { provider: "opencode-go", device_code: "dc", user_code: "ABCD-1234",
        verification_uri: "https://opencode.ai/console/device", expires_in: 600, interval: 5 },
    }, { [METHODS.authDevicePoll]: [{ state: "pending" }, { state: "pending" }] });

    const { unmount } = render(<SubscriptionPanel client={c} onClose={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: /设备码登录/ }));
    await act(async () => {});
    expect(screen.getByText("ABCD-1234")).toBeInTheDocument();
    await act(async () => { await vi.advanceTimersByTimeAsync(5_000); });
    const polls = c.requests.filter((m) => m === METHODS.authDevicePoll).length;
    unmount();
    await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
    expect(c.requests.filter((m) => m === METHODS.authDevicePoll)).toHaveLength(polls);
  } finally {
    vi.useRealTimers();
  }
});

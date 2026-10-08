import { expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, act, waitFor } from "@testing-library/react";
import type { RpcClient } from "@agentj/sdk";
import { METHODS } from "@agentj/sdk";
import SubscriptionPanel, { type AuthSetApiKeyResult } from "./SubscriptionPanel";

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

const linkClient = (result: unknown, status: unknown = { authenticated: false }) =>
  client({
    [METHODS.authStatus]: status,
    [METHODS.providerUsage]: { windows: [] },
    [METHODS.authSetApiKey]: result,
    [METHODS.authClearCredential]: { provider: "opencode-go", cleared: true },
  });

const submitKey = async (value: string) => {
  const input = screen.getByLabelText(/API Key/);
  fireEvent.change(input, { target: { value } });
  fireEvent.click(screen.getByRole("button", { name: "链接" }));
  return input;
};

it("links with a masked api key and never echoes it", async () => {
  const c = { request: vi.fn() };
  c.request.mockImplementation(async (m: string) => {
    if (m === METHODS.authStatus) return { authenticated: false };
    if (m === METHODS.providerUsage) return { windows: [] };
    if (m === METHODS.authSetApiKey) {
      return {
        provider: "opencode-go", saved: true, outcome: "verified",
        effective_source: "file", fetched_at: 1,
        models: [{ id: "opencode-go/kimi-k3", verified: true, format: "anthropic" }],
        credential: { kind: "api_key", key_hint: "…k3" },
      } as AuthSetApiKeyResult;
    }
    return {};
  });
  const onLinked = vi.fn();
  render(<SubscriptionPanel client={c as unknown as RpcClient} onClose={() => {}} onLinked={onLinked} />);
  const input = screen.getByLabelText(/API Key/);
  expect(input).toHaveAttribute("type", "password");
  fireEvent.change(input, { target: { value: "sk-secret-value" } });
  fireEvent.click(screen.getByRole("button", { name: "链接" }));
  await waitFor(() => expect(c.request).toHaveBeenCalledWith(METHODS.authSetApiKey,
    { provider: "opencode-go", api_key: "sk-secret-value", label: "web" }));
  await waitFor(() => expect(screen.getByText(/已链接/)).toBeInTheDocument());
  // the input is the leak surface that matters; a cleared masked field + clean storage is the proof
  expect((input as HTMLInputElement).value).toBe("");
  expect(screen.queryByText(/sk-secret-value/)).toBeNull();
  expect(window.localStorage.length).toBe(0);
  expect(onLinked).toHaveBeenCalledTimes(1);
  expect(onLinked.mock.calls[0][0]).toMatchObject({ models: [{ id: "opencode-go/kimi-k3" }] });
});

it("keeps the link button disabled until a key is typed", () => {
  const c = linkClient({ provider: "opencode-go", saved: true, outcome: "verified",
    effective_source: "file", fetched_at: 1, models: [] });
  render(<SubscriptionPanel client={c} onClose={() => {}} />);
  const input = screen.getByLabelText(/API Key/);
  expect(input).toHaveAttribute("type", "password");
  expect(input).toHaveAttribute("autocomplete", "off");
  const button = screen.getByRole("button", { name: "链接" });
  expect(button).toBeDisabled();
  fireEvent.change(input, { target: { value: "sk-x" } });
  expect(button).toBeEnabled();
  fireEvent.change(input, { target: { value: "   " } });
  expect(button).toBeDisabled();
});

it("warns before replacing an existing OAuth credential", async () => {
  const c = linkClient(
    { provider: "opencode-go", saved: true, outcome: "verified",
      effective_source: "file", fetched_at: 1, models: [] },
    { authenticated: true, credential: { kind: "oauth", org_id: "org-1" } });
  render(<SubscriptionPanel client={c} onClose={() => {}} />);
  expect(await screen.findByText(/将替换现有 OAuth 凭据/)).toBeInTheDocument();
});

it("says the key lives on the kernel host and tells when the environment wins", async () => {
  const c = linkClient({ provider: "opencode-go", saved: true, outcome: "verified",
    effective_source: "env", fetched_at: 1, models: [] });
  render(<SubscriptionPanel client={c} onClose={() => {}} />);
  expect(screen.getByText(/内核主机/)).toBeInTheDocument();
  await submitKey("sk-x");
  expect(await screen.findByText(/环境变量/)).toBeInTheDocument();
  expect(screen.getByText(/优先/)).toBeInTheDocument();
  expect(screen.getByText(/已链接/)).toBeInTheDocument();
});

it("marks an unverified save as unchecked", async () => {
  const c = linkClient({ provider: "opencode-go", saved: true, outcome: "unverified",
    effective_source: "file", fetched_at: 1, models: [],
    error: { kind: "unavailable", message: "m" } });
  render(<SubscriptionPanel client={c} onClose={() => {}} />);
  await submitKey("sk-x");
  expect(await screen.findByText(/已保存/)).toBeInTheDocument();
  expect(screen.getByText(/未校验/)).toBeInTheDocument();
});

it("maps a rejected outcome through ERROR_TEXT and reports nothing was saved", async () => {
  const c = linkClient({ provider: "opencode-go", saved: false, outcome: "rejected",
    effective_source: "file", fetched_at: 1, models: [],
    error: { kind: "unauthorized", message: "m" } });
  render(<SubscriptionPanel client={c} onClose={() => {}} />);
  await submitKey("sk-x");
  expect(await screen.findByText(/未保存/)).toBeInTheDocument();
  expect(screen.getByText(/密钥无效或已被撤销/)).toBeInTheDocument();
  expect((screen.getByLabelText(/API Key/) as HTMLInputElement).value).toBe("");
});

it("clears the stored credential and reloads the status", async () => {
  const c = linkClient({ provider: "opencode-go", saved: true, outcome: "verified",
    effective_source: "file", fetched_at: 1, models: [] },
    { authenticated: true, credential: { kind: "api_key", key_hint: "…1234" } });
  render(<SubscriptionPanel client={c} onClose={() => {}} />);
  expect(await screen.findByText("…1234")).toBeInTheDocument();
  const before = c.requests.filter((m) => m === METHODS.authStatus).length;
  fireEvent.click(screen.getByRole("button", { name: "解除链接" }));
  await waitFor(() => expect(c.request).toHaveBeenCalledWith(
    METHODS.authClearCredential, { provider: "opencode-go" }));
  await waitFor(() => expect(
    c.requests.filter((m) => m === METHODS.authStatus).length).toBe(before + 1));
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

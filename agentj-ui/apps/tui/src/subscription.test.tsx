import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import type { TestRendererSetup } from "@opentui/core/testing";
import { act } from "react";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";
import { SubscriptionOverlay, trendLine } from "./subscription.tsx";

function getInput(setup: TestRendererSetup) {
  const input = setup.mockInput as unknown as { press?: (k: string) => void };
  if (!input.press) {
    input.press = (k: string) => {
      if (k === "escape" || k === "esc") setup.mockInput.pressEscape();
      else if (k === "enter" || k === "return") setup.mockInput.pressEnter();
      else setup.mockInput.pressKey(k);
    };
  }
  return input as { press: (k: string) => void };
}

type RecordedCall = { method: string; params: Record<string, unknown> };

type FakeClient = RpcClient & {
  calls: string[];
  params: RecordedCall[];
  queue: (method: string, replies: unknown[]) => void;
};

// Static per-method replies with an optional FIFO queue per method, matching the
// Web panel's test client (subscriptionPanel.test.tsx).
function fakeClient(responses: Record<string, unknown>): FakeClient {
  const calls: string[] = [];
  const params: RecordedCall[] = [];
  const queues = new Map<string, unknown[]>();
  const client = {
    calls,
    params,
    queue(method: string, replies: unknown[]) {
      queues.set(method, [...replies]);
    },
    async request(method: string, p: Record<string, unknown> = {}): Promise<unknown> {
      calls.push(method);
      params.push({ method, params: p });
      const q = queues.get(method);
      if (q && q.length > 0) return q.shift();
      return responses[method];
    },
  };
  return client as unknown as FakeClient;
}

describe("SubscriptionOverlay", () => {
  test("renders the usage windows and an entitlement error", async () => {
    const client = fakeClient({
      [METHODS.authStatus]: {
        provider: "opencode-go",
        authenticated: true,
        credential: { kind: "api_key", source: "file", key_hint: "…1234", expiring_soon: false },
      },
      [METHODS.providerUsage]: {
        provider: "opencode-go",
        fetched_at: 1,
        stale: false,
        warn_percent: 80,
        windows: [{ kind: "rolling", status: "rate-limited", percent: 100 }],
        error: { kind: "entitlement", message: "OpenCode Go subscription required." },
      },
    });
    const setup = await testRender(
      <SubscriptionOverlay client={client} onClose={() => {}} />,
      {},
    );
    await act(async () => {
      await setup.flush();
    });
    await setup.waitFor(() => setup.captureCharFrame().includes("5 小时"));
    const frame = setup.captureCharFrame();
    expect(frame).toContain("已限流");
    expect(frame).toContain("该工作区没有 OpenCode Go 订阅");
    act(() => setup.renderer.destroy());
  });

  test("never fabricates a percentage for a window whose percent is absent", async () => {
    const client = fakeClient({
      [METHODS.authStatus]: { provider: "opencode-go", authenticated: true },
      [METHODS.providerUsage]: {
        provider: "opencode-go",
        fetched_at: 1,
        stale: false,
        warn_percent: 80,
        windows: [{ kind: "rolling", status: "ok" }],
      },
    });
    const setup = await testRender(
      <SubscriptionOverlay client={client} onClose={() => {}} />,
      {},
    );
    await act(async () => {
      await setup.flush();
    });
    await setup.waitFor(() => setup.captureCharFrame().includes("5 小时"));
    const frame = setup.captureCharFrame();
    expect(frame).toContain("—");
    expect(frame).not.toContain("0%");
    act(() => setup.renderer.destroy());
  });

  test("stops polling after authorization and follows the upstream interval", async () => {
    const scheduled: Array<{ ms: number; run: () => void }> = [];
    const client = fakeClient({
      [METHODS.authStatus]: { provider: "opencode-go", authenticated: false },
      [METHODS.providerUsage]: {
        provider: "opencode-go",
        fetched_at: 1,
        stale: false,
        warn_percent: 80,
        windows: [],
      },
      [METHODS.authDeviceStart]: {
        provider: "opencode-go",
        device_code: "dc",
        user_code: "ABCD-1234",
        verification_uri: "https://opencode.ai/console/device?user_code=ABCD-1234",
        expires_in: 600,
        interval: 5,
      },
      [METHODS.modelList]: {
        provider: "opencode-go",
        default_model: "m",
        source: "account",
        fetched_at: 1,
        models: [],
      },
    });
    client.queue(METHODS.authDevicePoll, [
      { state: "pending" },
      { state: "slow_down", interval: 10 },
      { state: "authorized", credential: { kind: "oauth", source: "file", expiring_soon: false } },
    ]);
    const setup = await testRender(
      <SubscriptionOverlay
        client={client}
        onClose={() => {}}
        schedule={(fn, ms) => scheduled.push({ ms, run: fn })}
      />,
      {},
    );

    const input = getInput(setup);
    await act(async () => {
      await input.press("l");
      await setup.flush();
    });
    // first poll at the upstream interval
    expect(scheduled.map((s) => s.ms)).toEqual([5_000]);

    await act(async () => {
      scheduled.shift()!.run(); // → pending, reschedule
      await setup.flush();
    });
    await act(async () => {
      scheduled.shift()!.run(); // → slow_down, sleep 10s
      await setup.flush();
    });
    expect(scheduled.map((s) => s.ms)).toEqual([10_000]);
    await act(async () => {
      scheduled.shift()!.run(); // → authorized
      await setup.flush();
    });
    // no further polling scheduled
    expect(scheduled).toHaveLength(0);
    expect(client.calls.filter((m) => m === METHODS.authDevicePoll)).toHaveLength(3);
    expect(client.calls).toContain(METHODS.modelList);
    act(() => setup.renderer.destroy());
  });

  test("clears the pending poll on unmount so no request is issued", async () => {
    const scheduled: Array<{ ms: number; run: () => void }> = [];
    const client = fakeClient({
      [METHODS.authStatus]: { provider: "opencode-go", authenticated: false },
      [METHODS.providerUsage]: {
        provider: "opencode-go",
        fetched_at: 1,
        stale: false,
        warn_percent: 80,
        windows: [],
      },
      [METHODS.authDeviceStart]: {
        provider: "opencode-go",
        device_code: "dc",
        user_code: "ABCD-1234",
        verification_uri: "https://opencode.ai/console/device?user_code=ABCD-1234",
        expires_in: 600,
        interval: 5,
      },
    });
    const setup = await testRender(
      <SubscriptionOverlay
        client={client}
        onClose={() => {}}
        schedule={(fn, ms) => scheduled.push({ ms, run: fn })}
      />,
      {},
    );
    const input = getInput(setup);
    await act(async () => {
      await input.press("l");
      await setup.flush();
    });
    expect(scheduled).toHaveLength(1);
    act(() => setup.renderer.destroy());
    await act(async () => {
      scheduled[0].run();
      await setup.flush();
    });
    expect(client.calls.filter((m) => m === METHODS.authDevicePoll)).toHaveLength(0);
  });
});

describe("SubscriptionOverlay link mode", () => {
  function linkClient(overrides: Record<string, unknown> = {}): FakeClient {
    return fakeClient({
      [METHODS.authStatus]: { provider: "opencode-go", authenticated: false },
      [METHODS.providerUsage]: { provider: "opencode-go", fetched_at: 1, windows: [] },
      ...overrides,
    });
  }

  /** Enter link mode with `i` and type the key one character at a time. */
  async function typeKey(setup: TestRendererSetup, text: string): Promise<void> {
    await act(async () => {
      getInput(setup).press("i");
      await setup.flush();
    });
    await act(async () => {
      for (const ch of text) setup.mockInput.pressKey(ch);
      await setup.flush();
    });
  }

  async function submit(setup: TestRendererSetup): Promise<void> {
    await act(async () => {
      getInput(setup).press("enter");
      await setup.flush();
    });
  }

  // Esc is dispatched only after the stdin parser's escape-sequence timeout, so
  // give it the same settle window app.test.tsx uses in its pressEscape helper.
  async function pressEscape(setup: TestRendererSetup): Promise<void> {
    await act(async () => {
      setup.mockInput.pressEscape();
      await setup.flush();
      await new Promise((resolve) => setTimeout(resolve, 30));
      await setup.flush();
    });
  }

  // A keyboard-driven state change lands without a new frame; force one (app.test.tsx
  // repaint helper) before asserting on captured text.
  async function repaint(setup: TestRendererSetup): Promise<void> {
    setup.resize(setup.renderer.width, setup.renderer.height);
    await act(async () => {
      await setup.flush();
    });
  }

  const verified = {
    provider: "opencode-go",
    saved: true,
    outcome: "verified",
    effective_source: "file",
    fetched_at: 1,
    models: [],
  };

  test("masks the api key and links on enter", async () => {
    const client = linkClient({ [METHODS.authSetApiKey]: verified });
    const setup = await testRender(<SubscriptionOverlay client={client} onClose={() => {}} />, {});
    await typeKey(setup, "sk-secret");
    await setup.waitFor(() => setup.captureCharFrame().includes("•••••••••"));
    expect(setup.captureCharFrame()).not.toContain("sk-secret");
    await submit(setup);
    await setup.waitFor(() => client.calls.includes(METHODS.authSetApiKey));
    await repaint(setup);
    const frame = setup.captureCharFrame();
    expect(frame).not.toContain("sk-secret");
    // submit clears the buffer, so the mask is gone too
    expect(frame).not.toContain("•");
    expect(frame).toContain("已链接");
    expect(client.params.filter((c) => c.method === METHODS.authSetApiKey)).toEqual([
      {
        method: METHODS.authSetApiKey,
        params: { provider: "opencode-go", api_key: "sk-secret", label: "tui" },
      },
    ]);
    act(() => setup.renderer.destroy());
  });

  test("renders the saved-but-unverified sentence", async () => {
    const client = linkClient({
      [METHODS.authSetApiKey]: { ...verified, outcome: "unverified", effective_source: "overlay" },
    });
    const setup = await testRender(<SubscriptionOverlay client={client} onClose={() => {}} />, {});
    await typeKey(setup, "sk-x");
    await submit(setup);
    await repaint(setup);
    const frame = setup.captureCharFrame();
    expect(frame).toContain("未校验");
    expect(frame).toContain("已保存");
    expect(frame).not.toContain("sk-x");
    act(() => setup.renderer.destroy());
  });

  test("renders the not-saved sentence and the actionable reason for a rejection", async () => {
    const client = linkClient({
      [METHODS.authSetApiKey]: {
        ...verified,
        saved: false,
        outcome: "rejected",
        error: { kind: "unauthorized", message: "upstream said 401" },
      },
    });
    const setup = await testRender(<SubscriptionOverlay client={client} onClose={() => {}} />, {});
    await typeKey(setup, "sk-bad");
    await submit(setup);
    await repaint(setup);
    const frame = setup.captureCharFrame();
    expect(frame).toContain("未保存");
    expect(frame).toContain("密钥无效或已被撤销");
    expect(client.calls).not.toContain(METHODS.authDeviceStart);
    act(() => setup.renderer.destroy());
  });

  test("states that the environment variable currently wins", async () => {
    const client = linkClient({
      [METHODS.authSetApiKey]: { ...verified, effective_source: "env" },
    });
    const setup = await testRender(<SubscriptionOverlay client={client} onClose={() => {}} />, {});
    await typeKey(setup, "sk-env");
    await submit(setup);
    await repaint(setup);
    const frame = setup.captureCharFrame();
    expect(frame).toContain("优先");
    expect(frame).toContain("环境变量");
    act(() => setup.renderer.destroy());
  });

  test("states that the key is stored on the connected kernel host", async () => {
    const client = linkClient();
    const setup = await testRender(<SubscriptionOverlay client={client} onClose={() => {}} />, {});
    await setup.waitFor(() => setup.captureCharFrame().includes("内核主机"));
    const frame = setup.captureCharFrame();
    expect(frame).toContain("i 链接 API Key");
    expect(frame).toContain("c 解除链接");
    act(() => setup.renderer.destroy());
  });

  test("clears the stored credential and reloads the status", async () => {
    const client = linkClient({
      [METHODS.authClearCredential]: { provider: "opencode-go", cleared: true },
    });
    client.queue(METHODS.authStatus, [
      {
        provider: "opencode-go",
        authenticated: true,
        credential: { kind: "api_key", source: "file", key_hint: "…1234" },
      },
      { provider: "opencode-go", authenticated: false },
    ]);
    const setup = await testRender(<SubscriptionOverlay client={client} onClose={() => {}} />, {});
    await setup.waitFor(() => setup.captureCharFrame().includes("…1234"));
    await act(async () => {
      getInput(setup).press("c");
      await setup.flush();
    });
    await setup.waitFor(() => client.calls.includes(METHODS.authClearCredential));
    // the reload is the observable proof that the status was re-read
    await setup.waitFor(
      () => client.calls.filter((m) => m === METHODS.authStatus).length >= 2,
    );
    await repaint(setup);
    expect(setup.captureCharFrame()).not.toContain("…1234");
    act(() => setup.renderer.destroy());
  });

  /** Counts `onCredentialChange` calls so one-shot reporting is observable. */
  function changeCounter(): { count: () => number; onChange: () => void } {
    let changes = 0;
    return {
      count: () => changes,
      onChange: () => {
        changes += 1;
      },
    };
  }

  test("reports one credential change after a saved link", async () => {
    const client = linkClient({ [METHODS.authSetApiKey]: verified });
    const counter = changeCounter();
    const setup = await testRender(
      <SubscriptionOverlay
        client={client}
        onClose={() => {}}
        onCredentialChange={counter.onChange}
      />,
      {},
    );
    await typeKey(setup, "sk-secret");
    await submit(setup);
    await setup.waitFor(() => client.calls.includes(METHODS.authSetApiKey));
    await repaint(setup);
    expect(counter.count()).toBe(1);
    act(() => setup.renderer.destroy());
  });

  test("does not report a credential change for a rejected link", async () => {
    const client = linkClient({
      [METHODS.authSetApiKey]: {
        ...verified,
        saved: false,
        outcome: "rejected",
        error: { kind: "unauthorized" },
      },
    });
    const counter = changeCounter();
    const setup = await testRender(
      <SubscriptionOverlay
        client={client}
        onClose={() => {}}
        onCredentialChange={counter.onChange}
      />,
      {},
    );
    await typeKey(setup, "sk-bad");
    await submit(setup);
    await setup.waitFor(() => client.calls.includes(METHODS.authSetApiKey));
    await repaint(setup);
    expect(counter.count()).toBe(0);
    act(() => setup.renderer.destroy());
  });

  test("reports a credential change after clearing the stored key", async () => {
    const client = linkClient({
      [METHODS.authClearCredential]: { provider: "opencode-go", cleared: true },
      [METHODS.modelList]: { models: [] },
      [METHODS.providerUsageHistory]: { samples: [] },
    });
    const counter = changeCounter();
    const setup = await testRender(
      <SubscriptionOverlay
        client={client}
        onClose={() => {}}
        onCredentialChange={counter.onChange}
      />,
      {},
    );
    await act(async () => {
      await setup.flush();
    });
    await act(async () => {
      getInput(setup).press("c");
      await setup.flush();
    });
    await setup.waitFor(() => client.calls.includes(METHODS.authClearCredential));
    await setup.waitFor(() => counter.count() > 0).catch(() => {});
    expect(counter.count()).toBe(1);
    act(() => setup.renderer.destroy());
  });

  test("keeps the device flow off while link mode owns the keys", async () => {
    const client = linkClient();
    const setup = await testRender(<SubscriptionOverlay client={client} onClose={() => {}} />, {});
    // `l` is the device-login binding outside link mode; here it is just key material
    await typeKey(setup, "skl");
    await repaint(setup);
    expect(setup.captureCharFrame()).toContain("•••");
    expect(client.calls).not.toContain(METHODS.authDeviceStart);
    act(() => setup.renderer.destroy());
  });

  test("trendLine renders a sparkline once two samples exist", () => {
    const samples = [
      { at: 1, windows: [{ kind: "rolling", percent: 10 }] },
      { at: 2, windows: [{ kind: "rolling", percent: 30 }] },
    ];
    expect(trendLine("rolling", samples)).toBe("rolling  ▁▃  10%→30%");
    expect(trendLine("rolling", samples.slice(0, 1))).toBe("");
    expect(trendLine("weekly", samples)).toBe("");
  });

  test("draws the trend line under the window it belongs to", async () => {
    const client = fakeClient({
      [METHODS.authStatus]: { provider: "opencode-go", authenticated: true },
      [METHODS.providerUsage]: {
        provider: "opencode-go",
        fetched_at: 3,
        windows: [{ kind: "rolling", status: "ok", percent: 30 }],
      },
      [METHODS.providerUsageHistory]: {
        provider: "opencode-go",
        retention_days: 30,
        samples: [
          { at: 1, windows: [{ kind: "rolling", status: "ok", percent: 10 }] },
          { at: 2, windows: [{ kind: "rolling", status: "ok", percent: 30 }] },
        ],
      },
    });
    const setup = await testRender(<SubscriptionOverlay client={client} onClose={() => {}} />, {});
    await act(async () => {
      await setup.flush();
    });
    await setup.waitFor(() => setup.captureCharFrame().includes("10%→30%"));
    const lines = setup.captureCharFrame().split("\n");
    const row = lines.findIndex((line) => line.includes("5 小时"));
    expect(row).toBeGreaterThanOrEqual(0);
    expect(lines[row + 1]).toContain("rolling");
    expect(lines[row + 1]).toContain("▁▃");
    expect(client.calls).toContain(METHODS.providerUsageHistory);
    act(() => setup.renderer.destroy());
  });

  test("leaves the overlay unchanged when the history request fails", async () => {
    const client = fakeClient({
      [METHODS.authStatus]: { provider: "opencode-go", authenticated: true },
      [METHODS.providerUsage]: {
        provider: "opencode-go",
        fetched_at: 1,
        windows: [{ kind: "rolling", status: "ok", percent: 30 }],
      },
    });
    const setup = await testRender(<SubscriptionOverlay client={client} onClose={() => {}} />, {});
    await act(async () => {
      await setup.flush();
    });
    await setup.waitFor(() => setup.captureCharFrame().includes("5 小时"));
    const frame = setup.captureCharFrame();
    expect(frame).toContain("5 小时  30%");
    expect(frame).not.toContain("rolling");
    expect(frame).not.toContain("→");
    act(() => setup.renderer.destroy());
  });

  test("cancels link mode with Esc without sending the key", async () => {
    const client = linkClient();
    let closed = false;
    const setup = await testRender(
      <SubscriptionOverlay client={client} onClose={() => { closed = true; }} />,
      {},
    );
    await typeKey(setup, "sk-secret");
    await setup.waitFor(() => setup.captureCharFrame().includes("•••••••••"));
    await pressEscape(setup);
    await repaint(setup);
    expect(setup.captureCharFrame()).not.toContain("•");
    expect(client.calls).not.toContain(METHODS.authSetApiKey);
    expect(closed).toBe(false);
    expect(setup.captureCharFrame()).toContain("订阅");
    act(() => setup.renderer.destroy());
  });
});

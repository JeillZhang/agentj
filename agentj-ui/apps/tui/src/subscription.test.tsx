import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import type { TestRendererSetup } from "@opentui/core/testing";
import { act } from "react";
import { METHODS } from "@agentj/sdk";
import type { RpcClient } from "@agentj/sdk";
import { SubscriptionOverlay } from "./subscription.tsx";

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

type FakeClient = RpcClient & {
  calls: string[];
  queue: (method: string, replies: unknown[]) => void;
};

// Static per-method replies with an optional FIFO queue per method, matching the
// Web panel's test client (subscriptionPanel.test.tsx).
function fakeClient(responses: Record<string, unknown>): FakeClient {
  const calls: string[] = [];
  const queues = new Map<string, unknown[]>();
  const client = {
    calls,
    queue(method: string, replies: unknown[]) {
      queues.set(method, [...replies]);
    },
    async request(method: string): Promise<unknown> {
      calls.push(method);
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

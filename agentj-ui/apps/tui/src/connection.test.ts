import { describe, expect, test, mock } from "bun:test";
import {
  getBackoffDelay,
  restoreTerminal,
  setupGracefulShutdown,
  ConnectionManager,
} from "./connection.ts";
import type { TransportHandle } from "./connection.ts";
import { METHODS } from "@agentj/sdk";
import type { ResponseRouter } from "@agentj/sdk";

describe("restoreTerminal", () => {
  test("leaves a clean terminal state", () => {
    expect(() => restoreTerminal()).not.toThrow();
    expect(process.exitCode ?? 0).toBe(0);
  });
});

describe("getBackoffDelay", () => {
  test("exponential backoff capped at 30s", () => {
    expect(getBackoffDelay(0)).toBe(1000);
    expect(getBackoffDelay(1)).toBe(2000);
    expect(getBackoffDelay(4)).toBe(16000);
    expect(getBackoffDelay(5)).toBe(30000);
    expect(getBackoffDelay(10)).toBe(30000);
    expect(getBackoffDelay(6, 500)).toBe(30000);
  });
});

describe("ConnectionManager", () => {
  function createMockTransport(
    onCalls: (lines: string[]) => void,
  ): TransportHandle {
    const lines: string[] = [];
    const handle = {
      close() {},
      send: async (line: string) => {
        lines.push(line);
        onCalls(lines);
        const req = JSON.parse(line) as { id: number; method: string };
        return JSON.stringify({ jsonrpc: "2.0", id: req.id, result: {} });
      },
    } as unknown as TransportHandle;
    return handle;
  }

  test("keeps activeSessionId across disconnect and resubscribes on open", async () => {
    const seen: string[][] = [];
    let current: TransportHandle | null = null;
    const create = mock((_: string, __: ResponseRouter) => {
      const h = createMockTransport((lines) => seen.push([...lines]));
      current = h;
      return h;
    });
    const mgr = new ConnectionManager("ws://test/ws", create as unknown as typeof create);
    mgr.setActiveSessionId("sess-123");
    mgr.connect();
    await new Promise((r) => setTimeout(r, 5));
    const t = current as unknown as TransportHandle;
    // simulate open
    t.onStatusChange?.("open");
    await new Promise((r) => setTimeout(r, 20));
    const client = mgr.getClient();
    expect(client).not.toBeNull();
    const allLines = seen.flat().join(" ");
    expect(allLines).toContain(METHODS.sessionSubscribe);
    expect(allLines).toContain("sess-123");
    expect(mgr.getActiveSessionId()).toBe("sess-123");
    // simulate disconnect then reconnect
    t.onStatusChange?.("closed");
    expect(mgr.getActiveSessionId()).toBe("sess-123");
    // after backoff, a new transport should be created - wait for timer (base 1000, attempt 0 => 1000)
    // we speed up by directly calling connect again
    mgr.connect();
    await new Promise((r) => setTimeout(r, 5));
    const t2 = current as unknown as TransportHandle;
    t2.onStatusChange?.("open");
    await new Promise((r) => setTimeout(r, 20));
    const all2 = seen.flat().join(" ");
    const count = (all2.match(/session\/subscribe/g) ?? []).length;
    expect(count).toBeGreaterThanOrEqual(2);
    mgr.destroy();
  });

  test("does not resubscribe when no activeSessionId (returns to SessionList)", async () => {
    const lines: string[] = [];
    let current: TransportHandle | null = null;
    const create = mock((_: string, __: ResponseRouter) => {
      const h = {
        close() {},
        send: async (line: string) => {
          lines.push(line);
          const req = JSON.parse(line) as { id: number };
          return JSON.stringify({ jsonrpc: "2.0", id: req.id, result: {} });
        },
      } as unknown as TransportHandle;
      current = h;
      return h;
    });
    const mgr = new ConnectionManager("ws://test/ws", create as unknown as typeof create);
    mgr.connect();
    await new Promise((r) => setTimeout(r, 5));
    (current as unknown as TransportHandle).onStatusChange?.("open");
    await new Promise((r) => setTimeout(r, 20));
    expect(lines.join(" ")).not.toContain(METHODS.sessionSubscribe);
    mgr.destroy();
  });

  test("onStatusChange forwards status", async () => {
    const statuses: string[] = [];
    let current: TransportHandle | null = null;
    const create = mock((_: string, __: ResponseRouter) => {
      const h = { close() {} } as unknown as TransportHandle;
      current = h;
      return h;
    });
    const mgr = new ConnectionManager("ws://test/ws", create as unknown as typeof create);
    mgr.onStatusChange = (s) => statuses.push(s);
    mgr.connect();
    await new Promise((r) => setTimeout(r, 5));
    (current as unknown as TransportHandle).onStatusChange?.("open");
    (current as unknown as TransportHandle).onStatusChange?.("closed");
    expect(statuses).toContain("open");
    expect(statuses).toContain("closed");
    mgr.destroy();
  });
});

describe("graceful shutdown", () => {
  test("SIGINT restores terminal and exits 0", async () => {
    const origExit = process.exit;
    let code: number | null = null;
    const restore = mock(() => {});
    // @ts-expect-error mock
    process.exit = ((c?: number) => {
      code = c ?? 0;
      throw new Error(`exit:${c}`);
    }) as unknown as typeof process.exit;
    const cleanup = setupGracefulShutdown(restore as unknown as () => void);
    expect(() => process.emit("SIGINT" as unknown as string)).toThrow("exit:0");
    expect(restore).toHaveBeenCalled();
    expect(code).toBe(0);
    cleanup();
    process.exit = origExit;
  });
});

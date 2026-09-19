import { describe, expect, it } from "vitest";
import { RpcClient } from "../client.js";
import { ResponseRouter } from "../router.js";
import { WebSocketTransport } from "../transports/websocket.js";

class MockWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static last: MockWebSocket | null = null;
  sent: string[] = [];
  readyState = MockWebSocket.CONNECTING;
  onmessage: ((ev: { data: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onopen: (() => void) | null = null;
  close = () => {
    this.readyState = MockWebSocket.CLOSED;
  };
  send = (data: string) => {
    if (this.readyState !== MockWebSocket.OPEN) {
      throw new DOMException("WebSocket is not open", "InvalidStateError");
    }
    this.sent.push(data);
  };
  url: string;
  constructor(url: string) {
    this.url = url;
    MockWebSocket.last = this;
  }
  emit(data: string) {
    this.onmessage?.({ data });
  }
  emitError() {
    this.onerror?.();
  }
  emitClose() {
    this.onclose?.();
  }
  emitOpen() {
    this.readyState = MockWebSocket.OPEN;
    this.onopen?.();
  }
}

describe("RpcClient", () => {
  it("sends a request and parses the response", async () => {
    const client = new RpcClient({
      send: async () =>
        JSON.stringify({ jsonrpc: "2.0", id: 1, result: { session_id: "s1" } }),
    });
    const result = await client.request("session/new", {});
    expect(result).toEqual({ session_id: "s1" });
  });

  it("rejects a non-string method before sending anything", async () => {
    let sent = false;
    const client = new RpcClient({
      send: async () => {
        sent = true;
        return JSON.stringify({ jsonrpc: "2.0", id: 1, result: {} });
      },
    });
    await expect(client.request(undefined as never)).rejects.toThrow("invalid RPC method");
    expect(sent).toBe(false);
  });

  it("rejects an empty method before sending anything", async () => {
    let sent = false;
    const client = new RpcClient({
      send: async () => {
        sent = true;
        return JSON.stringify({ jsonrpc: "2.0", id: 1, result: {} });
      },
    });
    await expect(client.request("")).rejects.toThrow("invalid RPC method");
    expect(sent).toBe(false);
  });

  it("rejects a whitespace-only method before sending anything", async () => {
    let sent = false;
    const client = new RpcClient({
      send: async () => {
        sent = true;
        return JSON.stringify({ jsonrpc: "2.0", id: 1, result: {} });
      },
    });
    await expect(client.request("   ")).rejects.toThrow("invalid RPC method");
    expect(sent).toBe(false);
  });

  it("feeds notifications to the registered handler instead of dropping them", () => {
    const router = new ResponseRouter();
    const seen: Array<[string, unknown]> = [];
    router.setNotificationHandler((method, params) => seen.push([method, params]));
    router.feed(JSON.stringify({ jsonrpc: "2.0", method: "$/progress", params: { p: 1 } }) + "\n");
    expect(seen).toEqual([["$/progress", { p: 1 }]]);
  });

  it("correlates responses by id when a notification precedes the response", async () => {
    const router = new ResponseRouter();
    const client = new RpcClient({
      send: (line: string) => {
        const id = router.requestId(line);
        const response = new Promise<string>((resolve, reject) => {
          router.register(id, { resolve, reject });
        });
        queueMicrotask(() => {
          router.feed(
            JSON.stringify({
              jsonrpc: "2.0",
              method: "$/progress",
              params: { token: id, value: { kind: "text", delta: "x" } },
            }) + "\n",
          );
          router.feed(
            JSON.stringify({ jsonrpc: "2.0", id, result: { version: "0.4.0" } }) + "\n",
          );
        });
        return response;
      },
    });
    const result = await client.request("kernel/status", {});
    expect(result).toEqual({ version: "0.4.0" });
  });

  it("notification with null id forks to handler, not to pending", async () => {
    const router = new ResponseRouter();
    const seen: string[] = [];
    router.setNotificationHandler((m) => void seen.push(m));
    router.feed('{"jsonrpc":"2.0","method":"$/progress","params":{}}\n');
    expect(seen).toEqual(["$/progress"]);
    expect(router.pendingCount()).toBe(0);

    // also test explicit null id notification (BUG-005)
    const seen2: Array<[string, unknown]> = [];
    router.setNotificationHandler((method, params) => seen2.push([method, params]));
    // Simulate a pending request with id 1, then feed a notification with id:null
    // to ensure it does not resolve the pending entry.
    const pendingPromise = new Promise<string>((resolve, reject) => {
      router.register(1, { resolve, reject });
    });
    let pendingResolved = false;
    pendingPromise.then(() => {
      pendingResolved = true;
    });
    router.feed('{"jsonrpc":"2.0","id":null,"method":"$/progress","params":{"p":2}}\n');
    expect(seen2).toEqual([["$/progress", { p: 2 }]]);
    expect(router.pendingCount()).toBe(1);
    expect(pendingResolved).toBe(false);
    // Now resolve the pending correctly via id 1
    router.feed('{"jsonrpc":"2.0","id":1,"result":{"ok":true}}\n');
    await expect(pendingPromise).resolves.toEqual('{"jsonrpc":"2.0","id":1,"result":{"ok":true}}');
    expect(router.pendingCount()).toBe(0);
  });
});

describe("WebSocketTransport", () => {
  it("sends NDJSON and routes responses through the router", async () => {
    const router = new ResponseRouter();
    const transport = new WebSocketTransport(
      "ws://127.0.0.1:4096/ws",
      router,
      (url) => new MockWebSocket(url) as unknown as WebSocket,
    );
    MockWebSocket.last!.emitOpen();
    const promise = transport.send('{"jsonrpc":"2.0","id":1,"method":"kernel/status"}');
    expect(MockWebSocket.last!.sent).toEqual([
      '{"jsonrpc":"2.0","id":1,"method":"kernel/status"}',
    ]);
    MockWebSocket.last!.emit('{"jsonrpc":"2.0","id":1,"result":{"modules":[]}}');
    await expect(promise).resolves.toEqual('{"jsonrpc":"2.0","id":1,"result":{"modules":[]}}');
    transport.close();
  });

  it("queues sends while CONNECTING and flushes them on open (BUG-002)", async () => {
    const router = new ResponseRouter();
    const transport = new WebSocketTransport(
      "ws://127.0.0.1:4096/ws",
      router,
      (url) => new MockWebSocket(url) as unknown as WebSocket,
    );
    const promise = transport.send('{"jsonrpc":"2.0","id":1,"method":"kernel/status"}');
    expect(MockWebSocket.last!.sent).toEqual([]);
    MockWebSocket.last!.emitOpen();
    expect(MockWebSocket.last!.sent).toEqual([
      '{"jsonrpc":"2.0","id":1,"method":"kernel/status"}',
    ]);
    MockWebSocket.last!.emit('{"jsonrpc":"2.0","id":1,"result":{"modules":[]}}');
    await expect(promise).resolves.toEqual('{"jsonrpc":"2.0","id":1,"result":{"modules":[]}}');
    transport.close();
  });

  it("rejects sends while the socket is not opening (BUG-002)", async () => {
    const router = new ResponseRouter();
    const transport = new WebSocketTransport(
      "ws://127.0.0.1:4096/ws",
      router,
      (url) => new MockWebSocket(url) as unknown as WebSocket,
    );
    MockWebSocket.last!.emitOpen();
    transport.close();
    await expect(
      transport.send('{"jsonrpc":"2.0","id":2,"method":"kernel/status"}'),
    ).rejects.toThrow("websocket not open");
  });

  it("feeds notifications into the router handler", () => {
    const router = new ResponseRouter();
    const seen: Array<[string, unknown]> = [];
    router.setNotificationHandler((method, params) => seen.push([method, params]));
    const transport = new WebSocketTransport(
      "ws://127.0.0.1:4096/ws",
      router,
      (url) => new MockWebSocket(url) as unknown as WebSocket,
    );
    MockWebSocket.last!.emit('{"jsonrpc":"2.0","method":"$/progress","params":{"p":1}}');
    expect(seen).toEqual([["$/progress", { p: 1 }]]);
    transport.close();
  });

  it("rejects pending on error/close and reports status", async () => {
    const router = new ResponseRouter();
    const statuses: string[] = [];
    const transport = new WebSocketTransport(
      "ws://127.0.0.1:4096/ws",
      router,
      (url) => new MockWebSocket(url) as unknown as WebSocket,
    );
    transport.onStatusChange = (s) => statuses.push(s);
    MockWebSocket.last!.emitOpen();
    const promise = transport.send('{"jsonrpc":"2.0","id":1,"method":"kernel/status"}');
    MockWebSocket.last!.emitClose();
    await expect(promise).rejects.toThrow("websocket closed");
    expect(statuses).toContain("open");
    expect(statuses).toContain("closed");
    transport.close();
  });
});

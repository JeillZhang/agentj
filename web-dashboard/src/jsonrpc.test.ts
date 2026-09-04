import { describe, expect, it } from 'vitest';
import { RpcClient, ResponseRouter } from './jsonrpc';
import { WebSocketTransport } from './transport/websocket';

class MockWebSocket {
  static last: MockWebSocket | null = null;
  sent: string[] = [];
  onmessage: ((ev: { data: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onopen: (() => void) | null = null;
  close = () => {};
  send = (data: string) => {
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
    this.onopen?.();
  }
}

describe('RpcClient', () => {
  it('sends a request and parses the response', async () => {
    const client = new RpcClient({
      send: async () =>
        JSON.stringify({ jsonrpc: '2.0', id: 1, result: { session_id: 's1' } }),
    });
    const result = await client.request('session/new', {});
    expect(result).toEqual({ session_id: 's1' });
  });

  it('feeds notifications to the registered handler instead of dropping them', () => {
    const router = new ResponseRouter();
    const seen: Array<[string, unknown]> = [];
    router.setNotificationHandler((method, params) => seen.push([method, params]));
    router.feed(JSON.stringify({ jsonrpc: '2.0', method: '$/progress', params: { p: 1 } }) + '\n');
    expect(seen).toEqual([['$/progress', { p: 1 }]]);
  });

  it('correlates responses by id when a notification precedes the response', async () => {
    const router = new ResponseRouter();
    const client = new RpcClient({
      send: (line: string) => {
        const id = router.requestId(line);
        const response = new Promise<string>((resolve, reject) => {
          router.register(id, { resolve, reject });
        });
        // Simulate out-of-order arrival: an id-less $/progress notification
        // lands before the response matching the request id.
        queueMicrotask(() => {
          router.feed(
            JSON.stringify({
              jsonrpc: '2.0',
              method: '$/progress',
              params: { token: id, value: { kind: 'text', delta: 'x' } },
            }) + '\n',
          );
          router.feed(
            JSON.stringify({ jsonrpc: '2.0', id, result: { version: '0.4.0' } }) +
              '\n',
          );
        });
        return response;
      },
    });
    const result = await client.request('kernel/status', {});
    expect(result).toEqual({ version: '0.4.0' });
  });
});

describe('WebSocketTransport', () => {
  it('sends NDJSON and routes responses through the router', async () => {
    const router = new ResponseRouter();
    const transport = new WebSocketTransport(
      'ws://127.0.0.1:4096/ws',
      router,
      (url) => new MockWebSocket(url) as unknown as WebSocket,
    );
    const promise = transport.send('{"jsonrpc":"2.0","id":1,"method":"kernel/status"}');
    expect(MockWebSocket.last!.sent).toEqual([
      '{"jsonrpc":"2.0","id":1,"method":"kernel/status"}',
    ]);
    MockWebSocket.last!.emit('{"jsonrpc":"2.0","id":1,"result":{"modules":[]}}');
    await expect(promise).resolves.toEqual('{"jsonrpc":"2.0","id":1,"result":{"modules":[]}}');
    transport.close();
  });

  it('feeds notifications into the router handler', () => {
    const router = new ResponseRouter();
    const seen: Array<[string, unknown]> = [];
    router.setNotificationHandler((method, params) => seen.push([method, params]));
    const transport = new WebSocketTransport(
      'ws://127.0.0.1:4096/ws',
      router,
      (url) => new MockWebSocket(url) as unknown as WebSocket,
    );
    MockWebSocket.last!.emit('{"jsonrpc":"2.0","method":"$/progress","params":{"p":1}}');
    expect(seen).toEqual([['$/progress', { p: 1 }]]);
    transport.close();
  });

  it('rejects pending on error/close and reports status', async () => {
    const router = new ResponseRouter();
    const statuses: string[] = [];
    const transport = new WebSocketTransport(
      'ws://127.0.0.1:4096/ws',
      router,
      (url) => new MockWebSocket(url) as unknown as WebSocket,
    );
    transport.onStatusChange = (s) => statuses.push(s);
    MockWebSocket.last!.emitOpen();
    const promise = transport.send('{"jsonrpc":"2.0","id":1,"method":"kernel/status"}');
    MockWebSocket.last!.emitClose();
    await expect(promise).rejects.toThrow('websocket closed');
    expect(statuses).toContain('open');
    expect(statuses).toContain('closed');
    transport.close();
  });
});

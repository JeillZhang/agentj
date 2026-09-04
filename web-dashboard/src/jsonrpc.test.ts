import { describe, expect, it } from 'vitest';
import { RpcClient, ResponseRouter } from './jsonrpc';

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

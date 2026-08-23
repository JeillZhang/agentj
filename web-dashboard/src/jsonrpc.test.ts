import { describe, expect, it } from 'vitest';
import { RpcClient } from './jsonrpc';

describe('RpcClient', () => {
  it('sends a request and parses the response', async () => {
    const client = new RpcClient({
      send: async () =>
        JSON.stringify({ jsonrpc: '2.0', id: 1, result: { session_id: 's1' } }),
    });
    const result = await client.request('session/new', {});
    expect(result).toEqual({ session_id: 's1' });
  });
});
